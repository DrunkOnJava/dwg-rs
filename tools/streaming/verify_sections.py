#!/usr/bin/env python3
"""Black-box acceptance for the R2004-family data-page walk and section assembly
(§4.5, §4.6), never cargo test.
Usage: verify_sections.py <stream_r2004_sections binary> [<streaming_schedules binary>] [--local DIR ...]
--local adds every R2004-family *.dwg under DIR, and takes a pinned file from DIR
instead of the network when its sha256 matches."""
import hashlib
import json
from pathlib import Path
import random
import struct
import subprocess
import sys
import tempfile
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_maps as vm  # noqa: E402

SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
CORRUPTED = ('synthetic_2004.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
FAMILY = (b'AC1018', b'AC1024', b'AC1027', b'AC1032')
MASK = 0x4164536b
PAGE_TYPE = 0x4163043b

# Byte offsets in a DWGS checkpoint (src/streaming/checkpoint.rs).
STAGE, LENGTH, DIGEST, INDEX, SIZE = 5, 6, 14, 46, 50
PAGE_COUNT, MAX_PAGE, COMPRESSED, SECTION_ID, ENCRYPTED = 58, 62, 66, 70, 74
PAGE_NUMBER, PAGE_SIZE, PAGE_ADDRESS = 78, 82, 86
REF_PAGE, REF_DATA, REF_START = 94, 98, 102
HEAD, DONE, CARRY_LEN = 110, 142, 158
CURSOR, REFS_DONE, LAST_START, FIRST_REF, LZ_LEN = 223, 231, 235, 243, 244


def line(value):
    return json.dumps(value, separators=(',', ':'))


def u32(data, at):
    return struct.unpack_from('<I', data, at)[0]


def masked(header, at):
    """§4.6 Sec_Mask over the 0x20 header bytes; its own inverse."""
    mask = (MASK ^ at) & 0xffffffff
    return bytearray(b''.join(struct.pack('<I', u32(header, 4 * i) ^ mask) for i in range(8)))


def header_at(data, at):
    return masked(data[at:at + 0x20], at)


def seal_header(header):
    """The header checksum: §4.2 over the header, its own field zeroed, seeded with the data checksum."""
    header[0x18:0x1c] = bytes(4)
    struct.pack_into('<I', header, 0x18, vm.checksum(u32(header, 0x1c), bytes(header)))


def edit_page(data, at, stored=None, fields=(), data_sum=True, header_sum=True):
    """Rewrite a data page: `stored` overwrites the start of its stored bytes,
    `fields` is (header offset, struct format, value), and each checksum is
    resealed unless told not to."""
    bad = bytearray(data)
    header = header_at(bad, at)
    size = u32(header, 8)
    if stored is not None:
        assert len(stored) <= size
        bad[at + 0x20:at + 0x20 + len(stored)] = stored
    for offset, fmt, value in fields:
        struct.pack_into(fmt, header, offset, value)
    if data_sum:
        struct.pack_into('<I', header, 0x1c, vm.checksum(0, bytes(bad[at + 0x20:at + 0x20 + size])))
    if header_sum:
        seal_header(header)
    bad[at:at + 0x20] = masked(header, at)
    return bytes(bad)


def assemble(data):
    """The exact stdout a correct walk prints, each section's bytes, and the
    pages in walk order, from the independent map parse and an independent
    decoder."""
    records = [json.loads(l) for l in vm.oracle(data)]
    pages, sections = {}, []
    for r in records:
        if 'page' in r and 'size' in r:
            pages.setdefault(r['page'], r)
        elif 'section' in r and 'name' in r:
            sections.append(dict(r, refs=[]))
        elif 'section_page' in r:
            sections[-1]['refs'].append(r)
    out, blobs, plan = [], [], []
    for s in sections:
        sid, size, mx = s['section_id'], s['size'], s['max_page_size']
        buf, cursor = bytearray(), 0

        def hole(to):
            nonlocal cursor
            if to > cursor:
                out.append(line({'hole': {'section': sid, 'offset': cursor, 'len': to - cursor}}))
                buf.extend(bytes(to - cursor))
                cursor = to
        for ref in s['refs']:
            page = pages[ref['section_page']]
            at, start = page['address'], ref['start_offset']
            h = header_at(data, at)
            stored = bytes(data[at + 0x20:at + 0x20 + u32(h, 8)])
            assert (u32(h, 0), u32(h, 4), struct.unpack_from('<Q', h, 0x10)[0], u32(h, 8), u32(h, 12)) == \
                (PAGE_TYPE, sid, start, ref['data_size'], page['size']), (at, 'header differs from the maps')
            assert vm.checksum(0, stored) == u32(h, 0x1c), (at, 'data checksum')
            zeroed = bytearray(h)
            zeroed[0x18:0x1c] = bytes(4)
            assert vm.checksum(u32(h, 0x1c), bytes(zeroed)) == u32(h, 0x18), (at, 'header checksum')
            produced = vm.inflate(stored)[0] if s['compressed'] == 2 else stored
            room = max(size - start, 0)
            assert min(mx, room) <= len(produced) <= mx, (at, 'page length')
            emitted = min(len(produced), room)
            out.append(line({'page': {'section': sid, 'number': ref['section_page'], 'address': at, 'start': start,
                                      'data': ref['data_size'], 'page_size': page['size']}}))
            hole(min(start, size))
            buf.extend(produced[:emitted])
            cursor += emitted
            out.append(line({'page_end': {'section': sid, 'number': ref['section_page'], 'decompressed': len(produced),
                                          'emitted': emitted, 'crc32': zlib.crc32(produced[:emitted])}}))
            plan.append({'index': s['section'], 'id': sid, 'compressed': s['compressed'], 'number': ref['section_page'],
                         'address': at, 'start': start, 'data': ref['data_size'], 'page_size': page['size'],
                         'size': size, 'max': mx})
        hole(size)
        assert len(buf) == size
        out.append(line({'section_done': {'index': s['section'], 'name': s['name'], 'size': size,
                                          'pages': len(s['refs']), 'crc32': zlib.crc32(bytes(buf))}}))
        blobs.append(bytes(buf))
    return out, blobs, plan


# A file built here, so that shapes real files do not have (holes, zero pages,
# a section with no pages) are walked too. Every page is checksummed as
# §4.2/§4.6 say; compression is literal runs only.

def lz_literal(raw):
    n = len(raw)
    assert n >= 4
    if n <= 18:
        head = bytes([n - 3])
    else:
        rest = n - 18
        zeros = (rest - 1) // 255
        head = b'\0' + bytes(zeros) + bytes([rest - 255 * zeros])
    return head + raw + b'\x11'


def lcg(seed, n):
    out = bytearray()
    for _ in range(n):
        seed = (seed * 0x343fd + 0x269ec3) & 0xffffffff
        out.append((seed >> 16) & 0xff)
    return bytes(out)


def system_page(tag, raw):
    body = lz_literal(raw) + b'\0\0'
    zeroed = struct.pack('<5I', tag, len(raw), len(body), 2, 0)
    header = struct.pack('<5I', tag, len(raw), len(body), 2, vm.checksum(vm.checksum(0, zeroed), body))
    page = header + body
    return page + bytes(-len(page) % 0x20)


def build(sections):
    """sections: dicts with name, id, compressed, mx, size, encrypted and pages
    of (start, raw bytes). A stored page's raw bytes are its whole stored data."""
    pages, number = [], 0
    for s in sections:
        s['refs'] = []
        for start, raw in s['pages']:
            number += 1
            data = lz_literal(raw) + b'\0\0' if s['compressed'] == 2 else raw
            page_size = (0x20 + len(data) + 0x1f) & ~0x1f
            pages.append((number, s, start, data, page_size))
            s['refs'].append((number, len(data), start))
    pm_number, sm_number = number + 1, number + 2
    entries = [(n, size) for n, _, _, _, size in pages]
    raw_pm = b''.join(struct.pack('<iI', n, size) for n, size in entries + [(pm_number, 0), (sm_number, 0)])
    pm_size = len(system_page(0x41630e3b, raw_pm))
    raw_sm = struct.pack('<5I', len(sections), 2, 0x7400, 0, len(sections))
    for s in sections:
        raw_sm += struct.pack('<QIIIIII', s['size'], len(s['refs']), s['mx'], 0, s['compressed'], s['id'], s['encrypted'])
        raw_sm += s['name'].encode().ljust(64, b'\0')
        for n, size, start in s['refs']:
            raw_sm += struct.pack('<IIQ', n, size, start)
    sm_size = len(system_page(0x4163003b, raw_sm))
    raw_pm = b''.join(struct.pack('<iI', n, size) for n, size in entries + [(pm_number, pm_size), (sm_number, sm_size)])
    body, at = bytearray(), 0x100
    for n, s, start, data, page_size in pages:
        stored = data
        zeroed = struct.pack('<IIIIQII', PAGE_TYPE, s['id'], len(stored), page_size, start, 0, 0)
        header = bytearray(zeroed)
        struct.pack_into('<I', header, 0x1c, vm.checksum(0, stored))
        seal_header(header)
        page = bytes(masked(header, at)) + stored
        body += page + bytes(page_size - len(page))
        at += page_size
    pm_at = at
    body += system_page(0x41630e3b, raw_pm) + system_page(0x4163003b, raw_sm)
    block = bytearray(0x6c)
    block[:12] = b'AcFssFcAJMB\0'
    for offset, value in ((0x0c, 0), (0x10, 0x6c), (0x14, 4), (0x44, 0x20), (0x48, 0x80), (0x4c, 0x40),
                          (0x50, pm_number), (0x5c, sm_number)):
        struct.pack_into('<I', block, offset, value)
    struct.pack_into('<Q', block, 0x54, pm_at - 0x100)
    struct.pack_into('<I', block, 0x68, zlib.crc32(bytes(block)))
    head = bytearray(0x100)
    head[:6] = b'AC1018'
    struct.pack_into('<I', head, 0x28, 0x80)
    head[0x80:0xec] = bytes(a ^ b for a, b in zip(block, vm.MAGIC))
    head[0xec:] = vm.MAGIC[0xec:]
    return bytes(head) + bytes(body)


def constructed(encrypted=0):
    mx = 0x400
    return build([
        # holes before both pages: [0, mx) and [2 mx, 3 mx)
        {'name': 'AcDb:Sparse', 'id': 1, 'compressed': 2, 'mx': mx, 'size': 3 * mx + 600, 'encrypted': 0,
         'pages': [(mx, lcg(1, mx)), (3 * mx, lcg(2, 600))]},
        # a zero page past the end, and the hole before it
        {'name': 'AcDb:Stored', 'id': 2, 'compressed': 1, 'mx': 256, 'size': 1000, 'encrypted': 0,
         'pages': [(0, lcg(3, 256)), (256, lcg(4, 256)), (1024, lcg(5, 256))]},
        {'name': 'AcDb:NoPages', 'id': 3, 'compressed': 2, 'mx': mx, 'size': 40, 'encrypted': 0, 'pages': []},
        # the last page decodes to a full page and the section keeps 100 bytes of it
        {'name': 'AcDb:Padded', 'id': 4, 'compressed': 2, 'mx': mx, 'size': 100, 'encrypted': 0,
         'pages': [(0, lcg(6, mx))]},
        {'name': 'AcDb:StoredLast', 'id': 5, 'compressed': 1, 'mx': 512, 'size': 700, 'encrypted': 0,
         'pages': [(0, lcg(7, 512)), (512, lcg(8, 512))]},
        {'name': 'AcDb:ZeroSize', 'id': 6, 'compressed': 2, 'mx': mx, 'size': 0, 'encrypted': 0,
         'pages': [(0, lcg(9, 8))]},
        {'name': 'AcDb:Exact', 'id': 7, 'compressed': 2, 'mx': mx, 'size': mx + 500, 'encrypted': encrypted,
         'pages': [(0, lcg(10, mx)), (mx, lcg(11, 500))]},
    ])


def main():
    argv = sys.argv[1:]
    local = []
    while '--local' in argv:
        at = argv.index('--local')
        local.append(Path(argv[at + 1]))
        del argv[at:at + 2]
    binary = str(Path(argv[0]).resolve())
    schedules = argv[1] if len(argv) > 1 else None
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-sections-') as tmp:
        tmp = Path(tmp)
        counter = [0]

        def fresh():
            counter[0] += 1
            path = tmp / f'out{counter[0]}'
            path.mkdir()
            return path

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=300)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        local_files = {}
        for directory in local:
            for path in sorted(directory.rglob('*.dwg')):
                data = path.read_bytes()
                if data[:6] in FAMILY:
                    local_files.setdefault(hashlib.sha256(data).hexdigest(), (path.name, data))
        corpus = [(f'synthetic_{v}.dwg', Path(f'tests/fixtures/canonical/synthetic_{v}.dwg').read_bytes())
                  for v in (2004, 2010, 2013, 2018)]
        manifest = json.loads(Path('tools/streaming/lz77-corpus.json').read_text())
        for item in manifest['files']:
            if item['sha256'] in local_files:
                corpus.append((item['name'], local_files.pop(item['sha256'])[1]))
                continue
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((item['name'], vm.fetch(url, item['bytes'], item['sha256'])))
        pinned = len(corpus)
        extra = []
        for digest, (name, data) in sorted(local_files.items(), key=lambda kv: kv[1][0]):
            extra.append((f'local_{name}', data))
        corpus += extra
        built = constructed()
        corpus.append(('constructed.dwg', built))

        totals = {'files': 0, 'sections': 0, 'pages': 0, 'bytes': 0, 'holes': 0, 'zero_pages': 0}
        expectations = {}
        for name, data in corpus:
            expected, blobs, plan = assemble(data)
            expectations[name] = (expected, blobs, plan)
            out = fresh()
            plain = run(data)
            assert plain[0] == 0 and plain[1].splitlines() == expected, (name, plain[2], 'walk differs from the independent assembly')
            written = run(data, '--out', out)
            assert written[:2] == plain[:2]
            for i, blob in enumerate(blobs):
                assert (out / f'{i}.bin').read_bytes() == blob, (name, i, 'assembled bytes differ from the independent assembly')
            legacy_out = fresh()
            legacy = run(data, '--legacy', '--out', legacy_out)
            assert legacy[0] == 0, (name, legacy[2])
            assert legacy[1].splitlines() == [l for l in expected if l.startswith('{"section_done"')], (name, 'legacy differs')
            for i, blob in enumerate(blobs):
                assert (legacy_out / f'{i}.bin').read_bytes() == blob, (name, i, 'legacy bytes differ')
            for args in ((1,), (3,), (64,), ('--chunks', '1,7,256,3', '--pending')):
                assert run(data, *args) == plain, (name, args)
            holes = sum(1 for l in expected if l.startswith('{"hole"'))
            zero_pages = sum(1 for p in plan if p['start'] >= p['size'])
            totals['files'] += 1
            totals['sections'] += len(blobs)
            totals['pages'] += len(plan)
            totals['bytes'] += sum(len(b) for b in blobs)
            totals['holes'] += holes
            totals['zero_pages'] += zero_pages
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'sections': len(blobs), 'pages': len(plan),
                       'holes': holes, 'zero_pages': zero_pages,
                       'oracles': ['independent Python assembly', 'legacy section_map::read_section_payload'],
                       'fixed_schedules': 5}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'sections-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=1800)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)
        assert totals['holes'] > 0 and totals['zero_pages'] > 0

        # Real files with a data page moved: the last page of a section goes a
        # page further, past the section's end, so a hole and a zero page
        # follow the section's real pages. The section map's start offset is
        # rewritten through a literal byte of its LZ77 stream (as verify_maps
        # does) and the page's own header resealed.
        moved = []
        for name, data in corpus:
            if name not in CORRUPTED or name == 'constructed.dwg':
                continue
            _, _, plan = expectations[name]
            pm, _, smid = vm.layout(data)
            sm = next(p['address'] for p in (json.loads(l) for l in vm.oracle(data) if l.startswith('{"page"')) if p['page'] == smid)
            dec, comp, body = vm.system(data, sm)
            raw, src = vm.inflate(data[body:body + comp])
            j = 20
            for s in (json.loads(l) for l in vm.oracle(data) if l.startswith('{"section"') and '"name"' in l):
                count = u32(raw, j + 8)
                refs = [p for p in plan if p['index'] == s['section']]
                if count >= 2 and s['size'] > 0:
                    k = count - 1
                    off = j + 96 + 16 * k + 8
                    old = refs[k]['start']
                    new = old + s['max_page_size']
                    differing = [i for i in range(8) if struct.pack('<Q', old)[i] != struct.pack('<Q', new)[i]]
                    if old < s['size'] <= new and all(src[off + i] is not None for i in differing):
                        bad = data
                        for i in differing:
                            bad = vm.patch(bad, sm, off + i, struct.pack('<Q', new)[i])
                        bad = edit_page(bad, refs[k]['address'], fields=[(0x10, '<Q', new)])
                        expected, blobs, _ = assemble(bad)
                        plain = run(bad)
                        assert plain[0] == 0 and plain[1].splitlines() == expected, (name, plain[2])
                        out = fresh()
                        run(bad, '--out', out)
                        legacy_out = fresh()
                        legacy = run(bad, '--legacy', '--out', legacy_out)
                        assert legacy[1].splitlines() == [l for l in expected if l.startswith('{"section_done"')], (name, 'legacy differs')
                        for i, blob in enumerate(blobs):
                            assert (out / f'{i}.bin').read_bytes() == blob == (legacy_out / f'{i}.bin').read_bytes(), (name, i)
                        assert any(l.startswith('{"hole"') for l in expected)
                        moved.append({'file': name, 'section': s['name'], 'page_index': k, 'start': [old, new]})
                        break
                j += 96 + 16 * count
        assert moved, 'no section page could be moved by a literal byte'
        receipts.append({'moved_pages_match_oracles': moved})

        # Fresh-process resume. Stops are counted in steps of the data-page
        # walk (the map walk before it is not interrupted): every step of
        # the canonical R2018 fixture, and around every page, hole and
        # section end of real files, plus sampled steps.
        checkpoint, first, second, full_trace = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl', tmp / 'full.jsonl'
        saved = {}
        for name, data in corpus:
            if name not in RESUMED and name != 'constructed.dwg':
                continue
            expected, blobs, plan = expectations[name]
            identity = hashlib.sha256(data).hexdigest()
            full = run(data, 7, '--trace', full_trace, '--report-steps', name=name)
            reads = full_trace.read_text().splitlines()
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = report[-1]['steps']
            events = [r['step'] for r in report[:-1]]
            if name == 'synthetic_2018.dwg':
                stops = range(steps + 1)
            else:
                around = {0, 1, 2, steps - 1, steps}
                for e in events:
                    around.update((e - 1, e, e + 1))
                around = sorted(s for s in around if 0 <= s <= steps)
                pick = random.Random(name).sample(around, min(len(around), 120 if name == 'constructed.dwg' else 70))
                stops = sorted(set(pick) | {0, steps} | set(random.Random(name + 'x').sample(range(steps), 12)))
            for stop in stops:
                out = fresh()
                paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop,
                             '--trace', first, '--out', out, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                state = checkpoint.read_bytes()
                kind = (state[STAGE], u32(state, LZ_LEN) > 0 if state[STAGE] == 3 else None)
                saved.setdefault((name, kind), (stop, state))
                resumed = run(data, 7, '--identity', identity, '--resume', checkpoint, '--trace', second, '--out', out, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == full[1].splitlines(), (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
                for i, blob in enumerate(blobs):
                    assert (out / f'{i}.bin').read_bytes() == blob, (name, stop, i, 'section bytes after resume')
            receipts.append({'file': name, 'steps': steps, 'checkpoint_stops': len(stops), 'fresh_process_resume': True,
                             'source_rereads_beyond_design': 0, 'duplicate_output': 0,
                             'note': 'each data page is read twice by design: a checksum pass, then a decoding pass'})
        kinds = {kind for (_, kind) in saved}
        assert kinds >= {(0, None), (1, None), (2, None), (3, True), (3, False), (4, None)}, kinds

        # Corrupt, truncated and forged checkpoints are rejected before use.
        name, data = next(c for c in corpus if c[0] == 'line_2004.dwg')
        identity = hashlib.sha256(data).hexdigest()

        # A sidecar for the same file sits beside every restore; the
        # checkpoint is read before the sidecar's index is used.
        stop_check, _ = saved[(name, (2, None))]
        run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop_check, name=name)
        sidecar = Path(str(checkpoint) + '.caller').read_text()

        def attempt(state, digest=identity, caller=None):
            checkpoint.write_bytes(state)
            Path(str(checkpoint) + '.caller').write_text(caller if caller is not None else sidecar)
            return run(data, 7, '--identity', digest, '--resume', checkpoint, name=name)
        check_state = checkpoint.read_bytes()
        assert check_state[STAGE] == 2
        for at in range(len(check_state)):
            bad = bytearray(check_state)
            bad[at] ^= 0x01
            result = attempt(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in range(len(check_state)):
            assert attempt(check_state[:length])[0] != 0, ('truncated checkpoint accepted', length)
        result = attempt(check_state, digest='f' * 64)
        assert result[0] != 0 and 'SourceMismatch' in result[2], result
        _, decoding = saved[(name, (3, True))]
        rng = random.Random('flips')
        for at in sorted(rng.sample(range(len(decoding)), 120)):
            bad = bytearray(decoding)
            bad[at] ^= 0x01
            result = attempt(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in sorted(rng.sample(range(len(decoding)), 120)):
            assert attempt(decoding[:length])[0] != 0, ('truncated checkpoint accepted', length)

        def resealed(state, edits, drop=0, add=b''):
            bad = bytearray(state[:-4])
            for at, value in edits:
                bad[at:at + len(value)] = value
            if drop:
                del bad[-drop:]
            bad += add
            return bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad)))
        p32 = lambda v: struct.pack('<I', v)
        p64 = lambda v: struct.pack('<Q', v)
        _, idle = saved[(name, (0, None))]
        _, header_state = saved[(name, (1, None))]
        _, stored = saved[(name, (3, False))]
        size_of = lambda state: struct.unpack_from('<Q', state, SIZE)[0]
        forgeries = [
            ('unknown stage', check_state, [(STAGE, b'\x07')], 0, b''),
            ('checked past the stored bytes', check_state, [(DONE, p32(1 << 30))], 0, b''),
            ('cursor past the section', check_state, [(CURSOR, p64(size_of(check_state) + 1))], 0, b''),
            ('more pages than the section has', check_state, [(REFS_DONE, p32(1 << 20))], 0, b''),
            ('first page flag against pages done', check_state, [(FIRST_REF, bytes([1 - check_state[FIRST_REF]]))], 0, b''),
            ('flag that is not 0 or 1', check_state, [(FIRST_REF, b'\x02')], 0, b''),
            ('header no longer matches its checksum', check_state, [(HEAD + 9, bytes([check_state[HEAD + 9] ^ 1]))], 0, b''),
            ('encrypted section', check_state, [(ENCRYPTED, p32(1))], 0, b''),
            ('unknown compression', check_state, [(COMPRESSED, p32(3))], 0, b''),
            ('page size of zero', check_state, [(MAX_PAGE, p32(0))], 0, b''),
            ('page outside the source', check_state, [(PAGE_ADDRESS, p64(1 << 62))], 0, b''),
            ('reference to another page', check_state, [(REF_PAGE, p32(u32(check_state, REF_PAGE) + 1))], 0, b''),
            ('start off a page boundary', check_state, [(REF_START, p64(struct.unpack_from('<Q', check_state, REF_START)[0] + 1))], 0, b''),
            ('decoder bytes outside a decoding page', check_state, [(LZ_LEN, p32(4))], 0, bytes(4)),
            ('decoding page without its decoder', decoding, [(LZ_LEN, p32(0))], len(decoding) - 4 - (LZ_LEN + 4), b''),
            ('more carried bytes than fit', decoding, [(CARRY_LEN, b'\xc8')], 0, b''),
            ('input position off the carried bytes', decoding, [(DONE, p32(u32(decoding, DONE) + 1))], 0, b''),
            ('stored page carrying bytes', stored, [(CARRY_LEN, b'\x01')], 0, b''),
            ('idle walker holding a page', idle, [(PAGE_NUMBER, p32(1))], 0, b''),
            ('header stage past the header', header_state, [(DONE, p32(0x20))], 0, b''),
        ]
        rejected = []
        for label, state, edits, drop, add in forgeries:
            result = attempt(resealed(state, edits, drop, add))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2], (label, result)
            rejected.append(label)
        receipts.append({'file': name, 'checkpoint_kinds': sorted(str(k) for k in kinds),
                         'bit_flips_rejected': len(check_state) + 120, 'truncations_rejected': len(check_state) + 120,
                         'identity_mismatch': 'rejected', 'resealed_forgeries_rejected': rejected})

        # A page offered to the walker that the page map or section map does
        # not stand behind is refused by the walker, not by the example.
        stop_mid, state_mid = saved[(name, (3, True))]
        paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop_mid, name=name)
        mid_sidecar = Path(str(checkpoint) + '.caller').read_text()
        mid_state = checkpoint.read_bytes()
        refused = []
        for label, edit, error in (
            ('page map address outside the source', lambda l: ' '.join(l.split(' ')[:3] + [str(1 << 40)]) if l.startswith('P ') else l, 'SectionPageRef'),
            ('reference to another section', lambda l: ' '.join(['R', str(int(l.split(' ')[1]) + 1)] + l.split(' ')[2:]) if l.startswith('R ') else l, 'SectionPageRef'),
        ):
            tampered = '\n'.join(edit(l) for l in mid_sidecar.split('\n'))
            result = attempt(mid_state, caller=tampered)
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, result)
            refused.append(label)
        receipts.append({'file': name, 'offered_pages_refused': refused})

        # Every check on a data page, on each release band: both checksums,
        # then each header field a resealed header can lie about, then a
        # stream that is too short for its place and one that outgrows a page.
        for name, data in corpus:
            if name not in CORRUPTED:
                continue
            expected, _, plan = expectations[name]
            lz = [p for p in plan if p['compressed'] == 2 and p['data'] >= 200 and min(p['max'], max(p['size'] - p['start'], 0)) > 8]
            target = lz[0]
            at = target['address']
            raw_flip = bytearray(data)
            raw_flip[at + 8] ^= 0x01
            data_flip = bytearray(data)
            data_flip[at + 0x20 + target['data'] // 2] ^= 0x01
            short = bytes([0x01, 1, 2, 3, 4, 0x11])
            outgrow = bytes([0x01, 1, 2, 3, 4, 0x20]) + bytes(130) + bytes([0x01, 0, 0])
            cases = [
                ('header bit flip', bytes(raw_flip), f'DataPageHeaderChecksum {{ address: {at},'),
                ('data bit flip', bytes(data_flip), f'DataPageChecksum {{ address: {at},'),
                ('page type', edit_page(data, at, fields=[(0, '<I', PAGE_TYPE ^ 1)]), f'DataPage({at})'),
                ('section number', edit_page(data, at, fields=[(4, '<I', target['id'] + 1)]), f'DataPage({at})'),
                ('start offset', edit_page(data, at, fields=[(0x10, '<Q', target['start'] + 1)]), f'DataPage({at})'),
                ('data size', edit_page(data, at, fields=[(8, '<I', target['data'] - 1)]), f'DataPage({at})'),
                ('page size', edit_page(data, at, fields=[(12, '<I', target['page_size'] + 0x20)]), f'DataPage({at})'),
                ('stream shorter than its place', edit_page(data, at, stored=short), f'DataPageLength({at})'),
                ('stream past the page size', edit_page(data, at, stored=outgrow), 'Lz77Overrun'),
            ]
            labels = []
            for label, bad, error in cases:
                result = run(bad, name=name)
                assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (name, label, error, result[2])
                printed = result[1].splitlines()
                assert printed == expected[:len(printed)], (name, label, 'output is not a valid prefix')
                labels.append(label)
            receipts.append({'file': name, 'corruptions_rejected': labels})

        # Shapes only a built file has: pages that decode short, a section the
        # walker is not allowed to decode, and a file whose data page would be
        # cut. Each ends in its named error.
        expected, _, plan = expectations['constructed.dwg']
        exact = next(p for p in plan if p['id'] == 7 and p['start'] == 0)
        padded = next(p for p in plan if p['id'] == 4)
        stored_page = next(p for p in plan if p['id'] == 2 and p['start'] == 0)
        cases = [
            ('compressed page shorter than its place', edit_page(built, exact['address'], stored=bytes([0x01, 1, 2, 3, 4, 0x11])), f'DataPageLength({exact["address"]})'),
            ('encrypted section', constructed(encrypted=1), 'SectionEncrypted(7)'),
            ('stored page data bit flip', edit_page(built, stored_page['address'], data_sum=False, stored=bytes([built[stored_page['address'] + 0x20] ^ 1])), f'DataPageChecksum {{ address: {stored_page["address"]},'),
            ('stored page type', edit_page(built, stored_page['address'], fields=[(0, '<I', PAGE_TYPE ^ 2)]), f'DataPage({stored_page["address"]})'),
            ('padded page past its size', edit_page(built, padded['address'], stored=bytes([0x01, 1, 2, 3, 4, 0x20]) + bytes(130) + bytes([0x01, 0, 0])), 'Lz77Overrun'),
        ]
        labels = []
        for label, bad, error in cases:
            result = run(bad, name='constructed.dwg')
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, error, result[2])
            printed = result[1].splitlines()
            assert printed == expected[:len(printed)], (label, 'output is not a valid prefix')
            labels.append(label)
        receipts.append({'file': 'constructed.dwg', 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 sections 4.5 and 4.6: R2004-family data pages (masked headers, both checksums, '
                       'LZ77 or stored data) assembled into sections, walked after the page map and section map. '
                       'No object decode, R2007 or performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'pinned_files': pinned,
              'local_files': len(extra), 'totals': totals, 'cases': receipts, 'status': 'passed'}
    Path('sections-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
