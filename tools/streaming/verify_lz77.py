#!/usr/bin/env python3
"""Black-box acceptance for the bounded §4.7 LZ77 decoder, never cargo test.
Usage: verify_lz77.py <stream_lz77 binary> [<streaming_schedules binary>]"""
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

WINDOW = 0xC000
LZ_FIXED = 54
SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')


def inflate(data):
    """Independent §4.7 decoder with the crate's measured distance rules.
    Returns the output, the bytes consumed through 0x11, and the positions
    of opcodes read where only an opcode may stand (after a literal run),
    as opposed to one found in place of a literal length."""
    out, opcodes = bytearray(), []
    at = 0

    def take():
        nonlocal at
        byte = data[at]
        at += 1
        return byte

    def run(total):
        while (byte := take()) == 0:
            total += 0xff
        return total + byte

    def length_or_opcode():
        byte = take()
        if byte == 0:
            return run(0x0f) + 3, None
        if byte & 0xf0:
            return 0, byte
        return byte + 3, None

    def literal(n):
        nonlocal at
        assert at + n <= len(data), 'literal past end'
        out.extend(data[at:at + n])
        at += n

    n, op = length_or_opcode()
    literal(n)
    while True:
        if op is None:
            op = take()
            opcodes.append(at - 1)
        if op == 0x11:
            return bytes(out), at, opcodes
        assert op >= 0x10, ('reserved opcode', at - 1)
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
            if op == 0x10:
                distance = base + 0x4000
            elif op < 0x20:
                distance = (((op & 0x08) << 11) | base) + 0x4000
            else:
                distance = base + 1
        op = None
        if lit == 0:
            lit, op = length_or_opcode()
        assert 1 <= distance <= len(out), ('distance', distance, len(out))
        for _ in range(count):
            out.append(out[-distance])
        literal(lit)


def fetch(url, size, digest):
    """Bounded retry for a pinned file; a mismatch fails at once."""
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
    with tempfile.TemporaryDirectory(prefix='dwg-lz77-') as tmp:
        tmp = Path(tmp)

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=600)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        corpus = [(f'synthetic_{v}.dwg', Path(f'tests/fixtures/canonical/synthetic_{v}.dwg').read_bytes(), None)
                  for v in (2004, 2010, 2013, 2018)]
        manifest = json.loads(Path('tools/streaming/lz77-corpus.json').read_text())
        for item in manifest['files']:
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((item['name'], fetch(url, item['bytes'], item['sha256']), item))

        pages_total = bytes_total = 0
        for name, data, item in corpus:
            base = run(data)
            assert base[0] == 0, (name, base[2])
            lines = [json.loads(line) for line in base[1].splitlines()]
            pages, summary = lines[:-1], lines[-1]
            legacy = run(data, '--legacy')
            assert legacy[0] == 0, (name, legacy[2])
            strip = [{k: v for k, v in page.items() if k != 'consumed'} for page in pages]
            assert strip + [summary] == [json.loads(line) for line in legacy[1].splitlines()], (name, 'legacy differs')
            trailing = set()
            for page in pages:
                compressed = data[page['offset']:page['offset'] + page['compressed']]
                out, consumed, _ = inflate(compressed)
                assert (len(out), zlib.crc32(out), consumed) == (page['produced'], page['crc32'], page['consumed']), (name, page)
                assert page['produced'] <= page['limit'], (name, page)
                trailing.add(compressed[consumed:].hex())
            if item:
                assert (summary['pages'], summary['produced']) == (item['pages'], item['produced']), (name, summary)
            for args in ((1,), (3, '--out', 1), ('--chunks', '1,7,256,3', '--out', '1,4096,5', '--pending'), (255, '--out', 65536)):
                assert run(data, *args) == base, (name, args)
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'pages': summary['pages'],
                       'produced': summary['produced'], 'oracles': ['crate lz77::decompress', 'independent Python §4.7 parse'],
                       'fixed_schedules': 4, 'bytes_after_terminator': sorted(trailing),
                       'every_page_at_declared_size': all(p['produced'] == p['limit'] for p in pages)}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'lz77-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=1800)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)
            pages_total += summary['pages']
            bytes_total += summary['produced']

        # Fresh-process resume inside one page: the two processes' source
        # reads and output concatenate to the uninterrupted run's.
        checkpoint, full_trace, first, second = tmp / 'state.bin', tmp / 'full.jsonl', tmp / 'first.jsonl', tmp / 'second.jsonl'
        saved = {}
        for name, data, _ in corpus:
            if name not in RESUMED:
                continue
            pages = [json.loads(line) for line in run(data)[1].splitlines()[:-1]]
            index = max((p for p in pages if p['kind'] == 'data'), key=lambda p: p['compressed'])['page']
            schedule = ('--chunks', 7, '--out', 13)
            full = run(data, '--page', index, *schedule, '--trace', full_trace, name=name)
            assert full[0] == 0, (name, full[2])
            reads = full_trace.read_text().splitlines()
            calls = json.loads(full[1])['calls']
            stops = range(calls + 1) if name.startswith('synthetic') else \
                sorted({0, 1, 2, 3, calls - 1, calls, *random.Random(name).sample(range(calls), 40)})
            for stop in stops:
                paused = run(data, '--page', index, *schedule, '--stop-after', stop, '--checkpoint-out', checkpoint, '--trace', first, name=name)
                assert paused[0] == 0, (name, stop, paused[2])
                assert json.loads(paused[1]) == {'checkpoint': True, 'calls': stop}, (name, stop, paused[1])
                state = checkpoint.read_bytes()
                assert len(state) <= LZ_FIXED + WINDOW + 4, (name, stop, len(state))
                saved.setdefault((name, state[5]), (stop, state, (tmp / 'state.bin.caller').read_bytes(), index))
                resumed = run(data, '--resume', checkpoint, *schedule, '--trace', second, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert resumed[1] == full[1], (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
            receipts.append({'file': name, 'page': index, 'calls': calls, 'checkpoint_stops': len(stops),
                             'fresh_process_resume': True, 'source_rereads': 0, 'duplicate_output': 0})

        # Corrupt, truncated and forged checkpoints are rejected before use.
        name, data = 'line_2004.dwg', next(d for n, d, _ in corpus if n == 'line_2004.dwg')
        modes = sorted(mode for (file, mode) in saved if file == name)
        assert {2, 3, 8} <= set(modes), modes

        def restore(state, caller):
            checkpoint.write_bytes(state)
            (tmp / 'state.bin.caller').write_bytes(caller)
            return run(data, '--resume', checkpoint, '--chunks', 7, '--out', 13, name=name)

        small = min((v for (f, _), v in saved.items() if f == name and v[0] > 0), key=lambda v: len(v[1]))
        _, state, caller, _ = small
        assert restore(state, caller)[0] == 0
        for at in range(len(state)):
            bad = bytearray(state)
            bad[at] ^= 0x01
            result = restore(bytes(bad), caller)
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in range(len(state)):
            assert restore(state[:length], caller)[0] != 0, ('truncated checkpoint accepted', length)
        assert restore(bytes(LZ_FIXED + WINDOW + 5), caller)[0] != 0

        def resealed(state, edits):
            bad = bytearray(state)
            for at, value in edits:
                bad[at:at + len(value)] = value
            bad[-4:] = struct.pack('<I', zlib.crc32(bytes(bad[:-4])))
            return bytes(bad)
        _, copy_state, copy_caller, _ = saved[(name, 8)]
        produced = struct.unpack_from('<Q', copy_state, 30)[0]
        forgeries = [
            (copy_state, [(5, b'\x0a')]),                              # unknown mode
            (copy_state, [(10, struct.pack('<I', produced + 1))]),      # copy before the first byte
            (copy_state, [(10, struct.pack('<I', 0))]),                 # zero distance
            (copy_state, [(22, struct.pack('<Q', 1 << 40))]),           # copy past the limit
            (copy_state, [(46, struct.pack('<Q', produced - 1))]),      # limit below output
            (copy_state, [(6, b'\x02')]),                               # extension kind outside Ext
            (copy_state, [(5, b'\x00')]),                               # Start after input
            (copy_state, [(30, struct.pack('<Q', produced + 1))]),      # window length disagrees
        ]
        _, literal_state, literal_caller, _ = saved[(name, 2)]
        forgeries.append((literal_state, [(22, struct.pack('<Q', 0))]))  # empty literal run
        rejected = 0
        for state, edits in forgeries:
            result = restore(resealed(state, edits), copy_caller if state is copy_state else literal_caller)
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], (edits, result)
            rejected += 1
        receipts.append({'file': name, 'checkpoint_modes': modes, 'bit_flips_rejected': len(state),
                         'truncations_rejected': len(state), 'resealed_forgeries_rejected': rejected})

        # Corrupt compressed bytes end in the named error.
        for name, data, _ in corpus:
            if name not in RESUMED:
                continue
            pages = [json.loads(line) for line in run(data)[1].splitlines()[:-1]]
            page = next(p for p in pages if p['kind'] == 'data' and p['compressed'] > 8)
            offset, index = page['offset'], page['page']
            _, consumed, opcodes = inflate(data[offset:offset + page['compressed']])
            cases = []
            bad = bytearray(data)
            bad[offset + opcodes[0]] = 0x05
            cases.append(('reserved opcode', bytes(bad), (), f'Lz77Opcode {{ at: {opcodes[0]}, opcode: 5 }}'))
            bad = bytearray(data)
            bad[offset:offset + 2] = b'\x41\xff'
            cases.append(('distance before output', bytes(bad), (), 'Lz77Offset { at: 1, distance: 1021 }'))
            cases.append(('output past limit', data, ('--limit', 100), 'Lz77Overrun { limit: 100 }'))
            cases.append(('stream cut before 0x11', data, ('--truncate', consumed - 3), 'Lz77Truncated'))
            labels = []
            for label, mutated, extra, error in cases:
                result = run(mutated, '--page', index, *extra, name=name)
                assert result[0] != 0 and error in result[2], (name, label, error, result[2])
                labels.append(label)
            receipts.append({'file': name, 'page': index, 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 section 4.7 LZ77 over every compressed page (page map, section map, data pages) '
                       'of R2004-family files. Decoder only: no page-map walk, section assembly or performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
              'files': len(corpus), 'pages': pages_total, 'decompressed_bytes': bytes_total,
              'cases': receipts, 'status': 'passed'}
    Path('lz77-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
