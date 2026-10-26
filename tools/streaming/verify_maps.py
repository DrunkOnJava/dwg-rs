#!/usr/bin/env python3
"""Black-box acceptance for the R2004-family page map and section map walk
(§4.1, §4.3-§4.5), never cargo test.
Usage: verify_maps.py <stream_r2004_maps binary> [<streaming_schedules binary>]"""
import hashlib
import json
from pathlib import Path
import random
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zlib

SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
CORRUPTED = ('synthetic_2004.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')


def line(value):
    return json.dumps(value, separators=(',', ':'))


def lcg(n):
    out, seed = bytearray(), 1
    for _ in range(n):
        seed = (seed * 0x343fd + 0x269ec3) & 0xffffffff
        out.append((seed >> 16) & 0xff)
    return bytes(out)


MAGIC = lcg(0x100)


def checksum(seed, data):
    """§4.2 section page checksum."""
    s1, s2 = seed & 0xffff, seed >> 16
    for i in range(0, len(data), 0x15b0):
        for b in data[i:i + 0x15b0]:
            s1 += b
            s2 += s1
        s1 %= 0xfff1
        s2 %= 0xfff1
    return (s2 << 16) | (s1 & 0xffff)


def inflate(data):
    """Independent §4.7 decoder that also maps each output byte to the input
    byte it was copied from as a literal (None for back-reference output)."""
    out, src, at = bytearray(), [], 0

    def take():
        nonlocal at
        at += 1
        return data[at - 1]

    def run(total):
        while (b := take()) == 0:
            total += 0xff
        return total + b

    def length_or_opcode():
        b = take()
        if b == 0:
            return run(0x0f) + 3, None
        if b & 0xf0:
            return 0, b
        return b + 3, None

    def literal(n):
        nonlocal at
        out.extend(data[at:at + n])
        src.extend(range(at, at + n))
        at += n

    n, op = length_or_opcode()
    literal(n)
    while True:
        if op is None:
            op = take()
        if op == 0x11:
            return bytes(out), src
        if op in (0x10, 0x20):
            count = run(0) + (9 if op == 0x10 else 0x21)
        elif op < 0x40:
            mask = 0x07 if op < 0x20 else 0x1f
            count = (op & mask if op & mask else run(0) + mask) + 2
        else:
            count = (op >> 4) - 1
        if op >= 0x40:
            distance, lit = ((take() << 2) | ((op & 0x0c) >> 2)) + 1, op & 0x03
        else:
            first, second = take(), take()
            base, lit = (first >> 2) | (second << 6), first & 0x03
            distance = base + 0x4000 if op == 0x10 else ((((op & 0x08) << 11) | base) + 0x4000 if op < 0x20 else base + 1)
        op = None
        if lit == 0:
            lit, op = length_or_opcode()
        for _ in range(count):
            out.append(out[-distance])
            src.append(None)
        literal(lit)


def layout(data):
    k = bytes(a ^ b for a, b in zip(data[0x80:0xec], MAGIC))
    u32 = lambda b, o: struct.unpack_from('<I', b, o)[0]
    pm, pmid, smid = struct.unpack_from('<Q', k, 0x54)[0] + 0x100, u32(k, 0x50), u32(k, 0x5c)
    return pm, pmid, smid


def system(data, at):
    _, dec, comp, _, _ = struct.unpack_from('<5I', data, at)
    return dec, comp, at + 0x14


def oracle(data):
    """The exact stdout a correct walk prints."""
    pm, pmid, smid = layout(data)
    out = [line({'header': data[:6].decode(), 'page_map_id': pmid, 'page_map_address': pm, 'section_map_id': smid})]
    dec, comp, body = system(data, pm)
    out.append(line({'system_page': 'page_map', 'address': pm, 'compressed': comp, 'decompressed': dec}))
    raw, _ = inflate(data[body:body + comp])
    assert len(raw) == dec
    address, i, sm = 0x100, 0, None
    while i < len(raw):
        number, size = struct.unpack_from('<iI', raw, i)
        i += 24 if number < 0 else 8
        out.append(line({'page': number, 'size': size, 'address': address}))
        if number == smid:
            sm = address
        address += size
    dec, comp, body = system(data, sm)
    out.append(line({'system_page': 'section_map', 'address': sm, 'compressed': comp, 'decompressed': dec}))
    raw, _ = inflate(data[body:body + comp])
    count = struct.unpack_from('<I', raw, 0)[0]
    out.append(line({'sections': count}))
    j = 20
    for index in range(count):
        size, pages, maximum, _, compressed, sid, encrypted = struct.unpack_from('<QIIIIII', raw, j)
        name = raw[j + 32:j + 96].split(b'\0')[0].decode('ascii')
        j += 96
        out.append(line({'section': index, 'name': name, 'size': size, 'page_count': pages, 'max_page_size': maximum,
                         'compressed': compressed, 'section_id': sid, 'encrypted': encrypted}))
        for _ in range(pages):
            page, size, start = struct.unpack_from('<IIQ', raw, j)
            j += 16
            out.append(line({'section_page': page, 'section_id': sid, 'data_size': size, 'start_offset': start}))
    assert j == len(raw)
    out.append(line({'finished': True}))
    return out


def patch(data, at, output_index, value):
    """Change one decoded byte of the system page at `at` by rewriting the
    literal input byte it came from, then reseal the page checksum."""
    bad = bytearray(data)
    _, comp, body = system(bad, at)
    _, src = inflate(bytes(bad[body:body + comp]))
    assert src[output_index] is not None, 'output byte did not come from a literal'
    bad[body + src[output_index]] = value
    header = bytearray(bad[at:at + 0x14])
    header[16:20] = bytes(4)
    struct.pack_into('<I', bad, at + 16, checksum(checksum(0, header), bytes(bad[body:body + comp])))
    return bytes(bad)


def fetch(url, size, digest):
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=30) as src:
                data = src.read(size + 1)
            break
        except OSError:
            if attempt == 2:
                raise
            time.sleep(5 * (attempt + 1))
    assert len(data) == size and hashlib.sha256(data).hexdigest() == digest, url
    return data


def main():
    binary = str(Path(sys.argv[1]).resolve())
    schedules = sys.argv[2] if len(sys.argv) > 2 else None
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-maps-') as tmp:
        tmp = Path(tmp)

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=600)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        corpus = [(f'synthetic_{v}.dwg', Path(f'tests/fixtures/canonical/synthetic_{v}.dwg').read_bytes())
                  for v in (2004, 2010, 2013, 2018)]
        manifest = json.loads(Path('tools/streaming/lz77-corpus.json').read_text())
        for item in manifest['files']:
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((item['name'], fetch(url, item['bytes'], item['sha256'])))

        totals = {'pages': 0, 'sections': 0, 'section_pages': 0}
        for name, data in corpus:
            expected = oracle(data)
            base = run(data)
            assert base[0] == 0 and base[1].splitlines() == expected, (name, base[2], 'walk differs from the independent parse')
            legacy = run(data, '--legacy')
            assert legacy[0] == 0, (name, legacy[2])
            records = [l for l in expected if l.startswith(('{"page"', '{"section"', '{"section_page"'))]
            assert legacy[1].splitlines() == records, (name, 'legacy differs')
            for args in ((1,), (3,), (64,), ('--chunks', '1,7,256,3', '--pending')):
                assert run(data, *args) == base, (name, args)
            counts = {key: sum(1 for l in expected if l.startswith('{"' + key + '"')) for key in ('page', 'section', 'section_page')}
            totals['pages'] += counts['page']
            totals['sections'] += counts['section']
            totals['section_pages'] += counts['section_page']
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'records': counts,
                       'oracles': ['independent Python parse', 'legacy section_map'], 'fixed_schedules': 5}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'map-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=1800)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)

        # Fresh-process resume at every step of the fixture and sampled steps
        # of real files: reads and output concatenate to the uninterrupted run.
        checkpoint, full_trace, first, second = tmp / 'state.bin', tmp / 'full.jsonl', tmp / 'first.jsonl', tmp / 'second.jsonl'
        saved = {}
        for name, data in corpus:
            if name not in RESUMED:
                continue
            identity = hashlib.sha256(data).hexdigest()
            full = run(data, 7, '--trace', full_trace, name=name)
            reads = full_trace.read_text().splitlines()
            steps = 0
            while True:
                probe = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', steps + 1, name=name)
                if '"checkpoint"' not in probe[1]:
                    break
                steps += 1
            stops = range(steps + 1) if name in ('synthetic_2018.dwg', 'line_2004.dwg') else \
                sorted({0, 1, 2, steps, *random.Random(name).sample(range(steps), 45)})
            for stop in stops:
                paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop, '--trace', first, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                state = checkpoint.read_bytes()
                cursor = 7 + struct.unpack_from('<H', state, 5)[0]
                cursor += 4 + struct.unpack_from('<I', state, cursor)[0]
                saved.setdefault((name, state[cursor]), (stop, state))
                resumed = run(data, 7, '--identity', identity, '--resume', checkpoint, '--trace', second, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == full[1].splitlines(), (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
            receipts.append({'file': name, 'steps': steps, 'checkpoint_stops': len(stops), 'fresh_process_resume': True,
                             'source_rereads_beyond_design': 0, 'duplicate_output': 0})

        # Corrupt, truncated and forged checkpoints are rejected before use.
        name, data = next(c for c in corpus if c[0] == 'line_2004.dwg')
        identity = hashlib.sha256(data).hexdigest()
        stages = sorted(stage for (file, stage) in saved if file == name)
        assert stages == [0, 1, 2, 3, 4], stages

        def restore(state, digest=identity):
            checkpoint.write_bytes(state)
            (tmp / 'state.bin.caller').write_text('0\n')
            return run(data, 7, '--identity', digest, '--resume', checkpoint, name=name)
        _, small = saved[(name, 2)]
        for at in range(len(small)):
            bad = bytearray(small)
            bad[at] ^= 0x01
            result = restore(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in range(len(small)):
            assert restore(small[:length])[0] != 0, ('truncated checkpoint accepted', length)
        result = restore(small, digest='f' * 64)
        assert result[0] != 0 and 'SourceMismatch' in result[2], result

        def resealed(state, edits):
            bad = bytearray(state)
            cursor = 7 + struct.unpack_from('<H', bad, 5)[0]
            cursor += 4 + struct.unpack_from('<I', bad, cursor)[0]
            for at, value in edits:
                bad[cursor + at:cursor + at + len(value)] = value
            bad[-4:] = struct.pack('<I', zlib.crc32(bytes(bad[:-4])))
            return bytes(bad)
        _, decoding = saved[(name, 3)]
        forgeries = [
            (small, [(0, b'\x07')]),                         # unknown stage
            (small, [(1, b'\x02')]),                         # unknown kind
            (small, [(34, struct.pack('<I', 1 << 30))]),     # checked past the page
            (small, [(22, struct.pack('<Q', 1 << 62))]),     # page outside the source
            (small, [(258, struct.pack('<I', 1))]),          # reserved field
            (decoding, [(120, b'\x32')]),                    # record size no record has
            (decoding, [(54, b'\xc8')]),                     # more carried bytes than fit
            (decoding, [(119, b'\x60')]),                    # record longer than it needs
            (decoding, [(34, struct.pack('<I', 1 << 30))]),  # read past the page
            (decoding, [(229, b'\x02')]),                    # flag that is not 0 or 1
        ]
        rejected = 0
        for state, edits in forgeries:
            result = restore(resealed(state, edits))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2], (edits, result)
            rejected += 1
        receipts.append({'file': name, 'checkpoint_stages': stages, 'bit_flips_rejected': len(small),
                         'truncations_rejected': len(small), 'identity_mismatch': 'rejected', 'resealed_forgeries_rejected': rejected})

        # Every check, on each release band: checksums, then record checks
        # reached by rewriting a literal byte and resealing the checksum.
        for name, data in corpus:
            if name not in CORRUPTED:
                continue
            expected = oracle(data)
            pm, _, smid = layout(data)
            pages = [json.loads(l) for l in expected if l.startswith('{"page"')]
            sm = next(p['address'] for p in pages if p['page'] == smid)
            _, _, pm_body = system(data, pm)
            _, _, sm_body = system(data, sm)
            raw_pm, src_pm = inflate(data[pm_body:pm_body + system(data, pm)[1]])
            raw_sm, src_sm = inflate(data[sm_body:sm_body + system(data, sm)[1]])
            flipped_pm, flipped_sm, tag = bytearray(data), bytearray(data), bytearray(data)
            flipped_pm[pm_body + 1] ^= 0x01
            flipped_sm[sm_body + 1] ^= 0x01
            tag[pm] ^= 0x01

            def literal_at(src, wanted):
                return next(i for i in wanted if src[i] is not None)
            size_byte = literal_at(src_pm, [8 * e + 4 for e in range(len(raw_pm) // 8)])
            self_index = next(e for e, p in enumerate(pages) if p['address'] == pm)
            earlier = literal_at(src_pm, [8 * e + 5 for e in range(self_index)])
            starts, j = [], 20
            for _ in range(struct.unpack_from('<I', raw_sm, 0)[0]):
                starts.append(j)
                j += 96 + 16 * struct.unpack_from('<I', raw_sm, j + 8)[0]
            # The first literal byte of a description field each check rejects:
            # a compression or encryption flag of 7, or a non-graphic name byte.
            wanted = []
            for d, start in enumerate(starts):
                name_len = raw_sm[start + 32:start + 96].index(0) if 0 in raw_sm[start + 32:start + 96] else 64
                wanted += [(start + 20, d, 7), (start + 28, d, 7)]
                wanted += [(start + 32 + c, d, 0x01) for c in range(name_len)]
            flags, described, poison = next(w for w in wanted if src_sm[w[0]] is not None)
            constant = literal_at(src_sm, range(4, 16))
            cases = [
                ('page map checksum', bytes(flipped_pm), f'SystemPageChecksum {{ address: {pm},'),
                ('section map checksum', bytes(flipped_sm), f'SystemPageChecksum {{ address: {sm},'),
                ('page map type', bytes(tag), f'SystemPage({pm})'),
                ('page size off 0x20', patch(data, pm, size_byte, raw_pm[size_byte] | 0x01), f'PageEntry({size_byte // 8})'),
                ('page map not at its own entry', patch(data, pm, earlier, (raw_pm[earlier] + 1) & 0xff), 'PageMapSelf'),
                ('section map header', patch(data, sm, constant, raw_sm[constant] ^ 0x01), 'SectionMapHeader'),
                ('section description', patch(data, sm, flags, poison), f'SectionEntry({described})'),
                # Real files keep their second header after the page map, so
                # the cut also fails the §4.1 range check on it.
                ('file cut inside the page map', data[:pm_body + 8], (f'SystemPage({pm})', 'R2004Field(180)')),
            ]
            labels = []
            for label, bad, error in cases:
                result = run(bad, name=name)
                errors = error if isinstance(error, tuple) else (error,)
                assert result[0] != 0 and any(e in result[2] for e in errors) and 'panicked' not in result[2], (name, label, error, result[2])
                printed = result[1].splitlines()
                # Moving the page map shifts the address of every page after
                # the patched size, and those pages are committed before the
                # self-entry check; every other case must print a prefix.
                if label != 'page map not at its own entry':
                    assert printed == expected[:len(printed)], (name, label, 'output is not a valid prefix')
                labels.append(label)
            receipts.append({'file': name, 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 sections 4.1 and 4.3-4.5: R2004-family file header, section page map and section map, '
                       'with system-page checksums. No data-page walk, object decode or performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'files': len(corpus),
              'records': totals, 'cases': receipts, 'status': 'passed'}
    Path('map-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
