#!/usr/bin/env python3
"""Black-box acceptance for the R13-R2000 container walk, never cargo test.
Usage: verify_walk.py <stream_walk binary> [<streaming_schedules binary>]"""
import hashlib
import json
from pathlib import Path
import random
import struct
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request
import zlib

VARS = (bytes.fromhex('cf7b1f23fdde38a95f7c68b84e6d335f'), bytes.fromhex('3084e0dc0221c756a0839747b192cca0'))
CLASSES = (bytes.fromhex('8da1c4b8c4a9f8c5c0dcf45fe7cfb68a'), bytes.fromhex('725e3b473b56073a3f230ba018304975'))
LOCATOR_SEED = {3: 0xa598, 4: 0x8101, 5: 0x3cc4, 6: 0x8461}


def crc8(data, seed=0xc0c1):
    value = seed
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ (0xa001 if value & 1 else 0)
    return value


def line(value):
    return json.dumps(value, separators=(',', ':'))


def mc(data, at, signed):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        if byte & 0x80:
            value |= (byte & 0x7f) << shift
            shift += 7
        elif signed:
            value |= (byte & 0x3f) << shift
            return (-value if byte & 0x40 else value), at
        else:
            return value | (byte & 0x7f) << shift, at


def layout(data):
    """Independent framing parse: every locator, section, map chunk and record."""
    count = struct.unpack_from('<I', data, 21)[0]
    locators = [struct.unpack_from('<BII', data, 25 + 9 * i) for i in range(count)]
    by = {number: (offset, size) for number, offset, size in locators}
    sections = []
    for part, sentinels in ((0, VARS), (1, CLASSES)):
        offset, total = by[part]
        assert data[offset:offset + 16] == sentinels[0], ('start sentinel', part)
        size = struct.unpack_from('<I', data, offset + 16)[0]
        assert 38 + size == total, ('section size', part)
        assert crc8(data[offset + 16:offset + 20 + size]) == struct.unpack_from('<H', data, offset + 20 + size)[0]
        assert data[offset + 22 + size:offset + 38 + size] == sentinels[1], ('end sentinel', part)
        sections.append((offset, size))
    chunks, records = [], []
    at, total = by[2]
    while True:
        n = struct.unpack_from('>H', data, at)[0]
        assert crc8(data[at:at + n]) == struct.unpack_from('>H', data, at + n)[0], ('map crc', at)
        chunks.append((at, n))
        q, handle, loc = at + 2, 0, 0
        while q < at + n:
            delta, q = mc(data, q, False)
            handle += delta
            delta, q = mc(data, q, True)
            loc += delta
            records.append((handle, loc))
        assert q == at + n
        at += n + 2
        if n == 2:
            break
    assert at == by[2][0] + total, 'map end'
    return locators, sections, chunks, records


def record(data, at, handle, r2000):
    size, p = 0, at
    for word in range(2):
        value = struct.unpack_from('<H', data, p)[0]
        p += 2
        size |= (value & 0x7fff) << (15 * word)
        if not value & 0x8000:
            break
    else:
        raise AssertionError('MS longer than two words')
    body = data[p:p + size]
    assert crc8(data[at:p + size]) == struct.unpack_from('<H', data, p + size)[0], ('record crc', handle)
    bits = ''.join(f'{b:08b}' for b in body[:16])

    def take(n):
        nonlocal bits
        value, bits = int(bits[:n] or '0', 2), bits[n:]
        return value
    tag = take(2)
    kind = (take(8) | take(8) << 8) if tag == 0 else take(8) if tag == 1 else (0 if tag == 2 else 256)
    if r2000:
        take(32)
    take(4)
    assert take(8 * take(4)) == handle, ('record handle', handle)
    return {'object': handle, 'offset': at, 'size': size, 'type': kind, 'crc32': zlib.crc32(body)}, p, size


def oracle(data):
    """The exact stdout a correct walk must print."""
    locators, sections, _, records = layout(data)
    out = [line({'header': data[:6].decode(), 'locators': [list(l) for l in locators]})]
    for name, (offset, size) in zip(('header_vars', 'classes'), sections):
        out.append(line({'section': name, 'offset': offset + 20, 'size': size, 'crc32': zlib.crc32(data[offset + 20:offset + 20 + size])}))
    for handle, at in records:
        out.append(line(record(data, at, handle, data[:6] == b'AC1015')[0]))
    out.append(line({'finished': True, 'objects': len(records)}))
    return out


def fix_header_crc(data):
    count = struct.unpack_from('<I', data, 21)[0]
    end = 25 + count * 9
    struct.pack_into('<H', data, end, crc8(data[:end], 0) ^ LOCATOR_SEED[count])


def corruptions(data):
    """(label, mutated bytes, expected error, committed output is a prefix, extra args)."""
    locators, sections, chunks, records = layout(data)
    r2000 = data[:6] == b'AC1015'
    (vars_at, vars_size), (classes_at, _) = sections
    map_at, map_n = chunks[0]
    handle, first = records[0]
    _, body_at, size = record(data, first, handle, r2000)
    cases = []

    def flip(label, at, error, prefix=True):
        bad = bytearray(data)
        bad[at] ^= 0x01
        cases.append((label, bad, error, prefix, ()))

    def forged(label, edit, error, prefix=True):
        bad = bytearray(data)
        edit(bad)
        cases.append((label, bad, error, prefix, ()))
    flip('vars start sentinel', vars_at + 3, 'SectionSentinel(0)')
    flip('vars size', vars_at + 16, 'SectionSize(0)')
    if vars_size:
        flip('vars data', vars_at + 20, 'SectionCrc { part: 0')
    flip('vars crc', vars_at + 20 + vars_size, 'SectionCrc { part: 0')
    flip('vars end sentinel', vars_at + 22 + vars_size + 15, 'SectionSentinel(0)')
    flip('classes start sentinel', classes_at, 'SectionSentinel(1)')
    flip('map entry byte', map_at + 2, 'MapCrc')
    flip('map crc', map_at + map_n, 'MapCrc')
    flip('record data', body_at + size - 1, f'ObjectCrc {{ handle: {handle},')
    flip('record crc', body_at + size, f'ObjectCrc {{ handle: {handle},')

    def section_size(bad):
        struct.pack_into('<I', bad, vars_at + 16, vars_size + 1)
        struct.pack_into('<H', bad, vars_at + 20 + vars_size, crc8(bad[vars_at + 16:vars_at + 20 + vars_size]))
    forged('vars size with valid crc', section_size, 'SectionSize(0)')

    def oversized_chunk(bad):
        struct.pack_into('>H', bad, map_at, 2035)
    forged('map chunk over 2034 bytes', oversized_chunk, f'MapChunk({map_at})')

    def handle_mismatch(bad):
        # The handle's last byte sits 8 counter bits after the type and RL.
        bits = ''.join(f'{b:08b}' for b in bad[body_at:body_at + 16])
        start = {'00': 18, '01': 10}.get(bits[:2], 2) + (32 if r2000 else 0) + 8
        last = start + 8 * int(bits[start - 4:start], 2) - 8
        bits = bits[:last + 7] + ('1' if bits[last + 7] == '0' else '0') + bits[last + 8:]
        bad[body_at:body_at + 16] = int(bits, 2).to_bytes(16, 'big')
        struct.pack_into('<H', bad, body_at + size, crc8(bad[first:body_at + size]))
    forged('record handle with valid crc', handle_mismatch, f'ObjectHandle {{ map: {handle},')

    def zero_size(bad):
        struct.pack_into('<H', bad, first, 0)
    forged('record MS of zero', zero_size, f'ObjectSize({handle})')

    def into_header(bad):
        # Same entry length: absolute offset 10 in the original offset
        # field's width, using a non-minimal modular char where needed.
        _, h_end = mc(bad, map_at + 2, False)
        _, o_end = mc(bad, h_end, True)
        width = o_end - h_end
        field = [10] if width == 1 else [0x80 | 10] + [0x80] * (width - 2) + [0]
        bad[h_end:o_end] = bytes(field)
        struct.pack_into('>H', bad, map_at + map_n, crc8(bad[map_at:map_at + map_n]))
    forged('map entry into the file header', into_header, f'ObjectRange {{ handle: {handle}, offset: 10')

    def no_classes(bad):
        for i in range(len(locators)):
            if bad[25 + 9 * i] == 1:
                struct.pack_into('<II', bad, 26 + 9 * i, 0, 0)
        fix_header_crc(bad)
    forged('class locator of size zero', no_classes, 'SectionSize(1)', prefix=False)

    def short_map(bad):
        for i in range(len(locators)):
            if bad[25 + 9 * i] == 2:
                struct.pack_into('<I', bad, 30 + 9 * i, locators[i][2] - 1)
        fix_header_crc(bad)
    forged('map locator one byte short', short_map, 'MapChunk(', prefix=False)
    map_end = next(o + n for number, o, n in locators if number == 2)
    for label, at in (('vars', vars_at + 5), ('map', map_at + 3), ('record', first + 3), ('last map byte', map_end - 1)):
        # A short file is caught by the header's locator ranges; a short
        # transport against the manifest length ends in a typed EOF.
        cases.append((f'file cut in {label}', bytearray(data[:at]), 'LocatorRange', False, ()))
        cases.append((f'transport short in {label}', bytearray(data[:at]), 'Truncated', True, ('--length', len(data))))
    return cases


def main():
    walk = str(Path(sys.argv[1]).resolve())
    schedules = sys.argv[2] if len(sys.argv) > 2 else None
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-walk-') as tmp:
        tmp = Path(tmp)

        def run(data, *args):
            path = tmp / 'input.dwg'
            path.write_bytes(data)
            result = subprocess.run([walk, str(path), *map(str, args)], capture_output=True, timeout=120)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        corpus = [('synthetic_2000.dwg', Path('tests/fixtures/canonical/synthetic_2000.dwg').read_bytes(), 5)]
        for path in sorted(Path('tools/streaming').glob('walk-*corpus.json')):
            manifest = json.loads(path.read_text(encoding='utf-8'))
            for item in manifest['files']:
                url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{urllib.parse.quote(item['name'])}"
                with urllib.request.urlopen(url, timeout=30) as src:
                    data = src.read(item['bytes'] + 1)
                assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256'], item['name']
                corpus.append((Path(item['name']).name, data, item['objects']))

        for name, data, objects in corpus:
            expected = oracle(data)
            base = run(data)
            assert base[0] == 0, (name, base[2])
            assert base[1].splitlines() == expected, (name, 'walk differs from the independent parse')
            assert json.loads(expected[-1])['objects'] == objects, name
            legacy = run(data, '--legacy')
            assert legacy[0] == 0 and '"skipped"' not in legacy[1], (name, legacy)
            records = [l for l in expected if l.startswith('{"object"')]
            assert [l for l in legacy[1].splitlines() if l.startswith('{"object"')] == records, (name, 'legacy differs')
            for split in (1, 2, 3, 7, 16, 255):
                assert run(data, split) == base, (name, split)
            assert run(data, '--chunks', '1,7,256,3', '--pending') == base, name
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'objects': objects,
                       'oracle': 'independent framing parse', 'legacy_parity': True, 'fixed_schedules': 7}
            if schedules and name in ('synthetic_2000.dwg', 'line_R14.dwg', 'line_2000.dwg'):
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'walk-schedules-{name}.jsonl')
                result = subprocess.run([schedules, walk, str(path), str(log)], capture_output=True, timeout=600)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)

        checkpoint, caller = tmp / 'state.bin', tmp / 'state.bin.caller'
        full_trace, first, second = tmp / 'full.jsonl', tmp / 'first.jsonl', tmp / 'second.jsonl'
        saved = {}
        for name, data, _ in corpus:
            if name not in ('synthetic_2000.dwg', 'line_R14.dwg', 'line_2000.dwg'):
                continue
            identity = hashlib.sha256(data).hexdigest()
            full = run(data, 7, '--trace', full_trace)
            reads = full_trace.read_text().splitlines()
            feeds = len(reads)
            stops = range(feeds + 1)
            if name != 'synthetic_2000.dwg':
                stops = sorted({0, 1, 2, 3, feeds - 1, feeds, *random.Random(name).sample(range(feeds), 60)})
            for stop in stops:
                paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop, '--trace', first)
                assert paused[0] == 0, (name, stop, paused[2])
                printed = paused[1].splitlines()
                assert printed[-1] == line({'checkpoint': True, 'feeds': stop}), (name, stop)
                state = checkpoint.read_bytes()
                assert len(state) <= 256, (name, stop, len(state))
                saved.setdefault((name, state[6 + state[5]]), (stop, state, caller.read_bytes()))
                resumed = run(data, 7, '--identity', identity, '--resume', checkpoint, '--trace', second, '--pending')
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert printed[:-1] + resumed[1].splitlines() == full[1].splitlines(), (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
            receipts.append({'file': name, 'checkpoint_stops': len(stops), 'feeds': feeds,
                             'fresh_process_resume': True, 'source_rereads': 0, 'duplicate_output': 0})

        name, data, _ = corpus[0]
        identity = hashlib.sha256(data).hexdigest()
        stages = sorted(stage for (file, stage) in saved if file == name)
        assert stages == [0, 1, 2, 3, 4, 5, 6], stages

        def restore(state, *, digest=identity, source=data, extra=None):
            checkpoint.write_bytes(state)
            caller.write_bytes(extra if extra is not None else saved[(name, 5)][2])
            return run(source, 7, '--identity', digest, '--resume', checkpoint)
        stop, state, extra = saved[(name, 5)]
        assert restore(state, extra=extra)[0] == 0
        for at in range(len(state)):
            bad = bytearray(state)
            bad[at] ^= 0x01
            result = restore(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at)
        for length in range(len(state)):
            assert restore(state[:length])[0] != 0, ('truncated checkpoint accepted', length)
        assert restore(bytes(257))[0] != 0
        result = restore(state, digest='f' * 64)
        assert result[0] != 0 and 'SourceMismatch' in result[2], result
        result = restore(state, source=data + b'X')
        assert result[0] != 0 and 'SourceMismatch' in result[2], result

        def resealed(state, edits):
            bad = bytearray(state)
            body = 6 + bad[5]
            for at, value in edits:
                bad[body + at:body + at + len(value)] = value
            bad[-4:] = struct.pack('<I', zlib.crc32(bytes(bad[:-4])))
            return bytes(bad)
        forgeries = {
            5: [[(0, b'\x09')], [(0, b'\x00')], [(1, struct.pack('<I', 1 << 20))], [(39, b'\x03')], [(40, b'\x03')],
                [(31, struct.pack('<Q', 40))], [(53, struct.pack('<Q', 1 << 40))], [(77, struct.pack('<Q', 1))]],
            4: [[(85, b'\x3f')], [(86, b'\x02')], [(53, struct.pack('<Q', 0))]],
            1: [[(1, struct.pack('<I', 64))], [(41, struct.pack('<Q', 1))]],
            3: [[(5, struct.pack('<I', 4000)), (1, struct.pack('<I', 2))]],
        }
        rejected = 0
        for stage, cases in forgeries.items():
            _, state, extra = saved[(name, stage)]
            for edits in cases:
                result = restore(resealed(state, edits), extra=extra)
                assert result[0] != 0 and 'InvalidCheckpoint' in result[2], (stage, edits, result)
                rejected += 1
        receipts.append({'file': name, 'checkpoint_stages': stages, 'bit_flips_rejected': len(state),
                         'truncations_rejected': len(state), 'identity_mismatch': 'rejected', 'resealed_forgeries_rejected': rejected})

        for name, data, _ in (corpus[0], next(c for c in corpus if c[0] == 'line_2000.dwg'), next(c for c in corpus if c[0] == 'line_R14.dwg')):
            expected = oracle(data)
            labels = []
            for label, bad, error, prefix, args in corruptions(data):
                result = run(bytes(bad), *args)
                assert result[0] != 0 and error in result[2], (name, label, error, result[2])
                if prefix:
                    printed = result[1].splitlines()
                    assert printed == expected[:len(printed)], (name, label, 'committed output is not the valid prefix')
                labels.append(label)
            receipts.append({'file': name, 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 R13-R2000 container walk: file header, header-variable and class framing, object map, '
                       'record framing, type and handle. No object body decode, R2004+ container, or performance claim.',
              'binary_sha256': hashlib.sha256(Path(walk).read_bytes()).hexdigest(), 'cases': receipts, 'status': 'passed'}
    Path('walk-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
