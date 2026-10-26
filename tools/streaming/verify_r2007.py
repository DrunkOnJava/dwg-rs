#!/usr/bin/env python3
"""Black-box acceptance for the whole R2007 (AC1021) file walk (ODA 5.4.1
sections 5.1-5.4 and 5.10-5.13), never cargo test: one walker and one
checkpoint over the file header, the page map, the section map and every data
page of every section, each page's codewords and checksums checked before its
bytes are handed out.
Usage: verify_r2007.py <stream_r2007 binary> [<streaming_schedules binary>] [--local DIR ...]"""
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

sys.path.insert(0, str(Path(__file__).resolve().parent))
import r2007_ref as ref  # noqa: E402

MANIFESTS = ('r2007-corpus.json', 'r2007-acadsharp-corpus.json')
SCHEDULED = ('arc_2007.dwg', 'constructed_a.dwg')
RESUMED = ('tiny.dwg', 'arc_2007.dwg', 'ellipse_2007.dwg', 'constructed_a.dwg', 'constructed_b.dwg', 'sample_AC1021.dwg')
BIG = 400000
READERS = {0: (), 5: (), 1: ('aux',), 2: ('sec',), 3: ('sec', 'aux'), 4: ('sec', 'aux')}

# Byte offsets in a DWGR checkpoint (src/streaming/r2007_checkpoint.rs).
STAGE, REC_KIND, DESCRIBED, SECTIONS, PAGES, EMITTED, PENDING = 5, 6, 47, 51, 55, 59, 67
HAVE, ENTRY_HAVE, REC_HAVE, FACTS, FLAGS, CUR, CUR_SIZES, REF, VARIABLE = 84, 86, 87, 88, 112, 121, 255, 319, 391
# Within a reader's progress.
R_PASS, R_CW, R_HAVE, R_MSG, R_TAKE, R_FED, R_PRODUCED = 0, 1, 17, 19, 21, 23, 31


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


def p16(v):
    return struct.pack('<H', v)


def p32(v):
    return struct.pack('<I', v)


def p64(v):
    return struct.pack('<Q', v)


def reseal(state, edits):
    bad = bytearray(state[:-4])
    for at, value in edits:
        bad[at:at + len(value)] = value
    return bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad)))


def checkpoint_readers(state):
    """(stage, {name: reader}) of a checkpoint: where each reader starts, its pass, its progress
    and the mode of its decoder."""
    stage = state[STAGE]
    at = VARIABLE + (struct.unpack_from('<H', state, HAVE)[0] if stage == 0 else 272)
    at += state[ENTRY_HAVE] + state[REC_HAVE]
    readers = {}
    for name in READERS[stage]:
        r = {'at': at, 'pass': state[at]}
        r['cw'], r['src'], r['have'], r['msg'], r['take'], r['fed'], r['produced'] = struct.unpack_from('<QQHHHQQ', state, at + 1)
        at += 39
        at += 9 + state[at + 8]
        at += 9 + state[at + 8]
        at += 11 + state[at + 10]
        at += max(r['have'], r['msg'])
        r['lz'] = state[at]
        produced = struct.unpack_from('<Q', state, at + 98)[0]
        at += 98 + 16 + min(produced, 0x10000)
        readers[name] = r
    assert at == len(state) - 4, (stage, at, len(state))
    return stage, readers


def sample(n, seed):
    g = random.Random(seed)
    out = bytearray()
    while len(out) < n:
        k = g.random()
        if k < 0.3 and len(out) > 10:
            d = g.randrange(1, min(len(out), 70000))
            for _ in range(g.randrange(3, g.choice([20, 60, 300, 5000]))):
                out.append(out[-d])
        elif k < 0.6:
            out += bytes(g.getrandbits(8) for _ in range(g.randrange(1, 40)))
        elif k < 0.8:
            out += bytes([g.getrandbits(8)]) * g.randrange(1, 3000)
        else:
            out += bytes(g.getrandbits(8) for _ in range(g.randrange(200, 800)))
    return bytes(out[:n])


def small_spec():
    """A small file with a page of each kind: compressed and raw Reed-Solomon pages,
    a bare page and a bare compressed one."""
    return {'seed': 11, 'crc_seed': 0x2AA, 'sections': [
        {'name': 'AcDb:One', 'data': sample(5000, 1), 'encoding': 4,
         'pages': [{'start': 0, 'len': 2500}, {'start': 2500, 'len': 2500}]},
        {'name': 'AcDb:Two', 'data': sample(3000, 2), 'encoding': 4, 'pages': [{'start': 0, 'len': 3000, 'compress': False}]},
        {'name': 'AcDb:Three', 'data': sample(600, 3), 'encoding': 1, 'pages': [{'start': 0, 'len': 600, 'compress': False}]},
    ]}


def constructed():
    """Whole files in shapes the corpus lacks: (name, bytes, shape)."""
    out = []
    tiny = {'seed': 5, 'crc_seed': 0x155, 'factors': (2, 2), 'sections': [
        {'name': 'AcDb:A', 'data': sample(300, 1), 'encoding': 4, 'pages': [{'start': 0, 'len': 300}]},
        {'name': 'AcDb:B', 'data': sample(80, 2), 'encoding': 1, 'pages': [{'start': 0, 'len': 80, 'compress': False}]},
    ]}
    out.append(('tiny.dwg', ref.build(tiny), 'one page of each encoding'))
    a = {'seed': 3, 'crc_seed': 0x155, 'factors': (3, 2), 'unnamed': 2, 'free': [(1, 0x40), (4, 0x20)], 'sections': [
        {'name': 'AcDb:Big', 'data': sample(260000, 21), 'encoding': 4, 'size': 250000,
         'pages': [{'start': 0, 'len': 70912}, {'start': 70912, 'len': 66688}, {'start': 137600, 'len': 64512},
                   {'start': 202112, 'len': 57888, 'compress': False}]},
        {'name': 'AcDb:Holes', 'data': sample(9000, 22), 'encoding': 4, 'size': 9000,
         'pages': [{'start': 100, 'len': 2000}, {'start': 4000, 'len': 3000}, {'start': 7000, 'len': 0}]},
        {'name': 'AcDb:Empty', 'data': b'', 'encoding': 4, 'size': 77, 'pages': []},
        {'name': 'AcDb:Padded', 'data': sample(700, 23), 'encoding': 1, 'size': 650,
         'pages': [{'start': 0, 'len': 700, 'compress': True}]},
        {'name': 'AcDb:Short', 'data': sample(4000, 24), 'encoding': 4, 'size': 6000, 'pages': [{'start': 0, 'len': 4000}]},
    ]}
    count = sum(len(s['pages']) for s in a['sections'])
    a['order'] = random.Random(8).sample(range(count), count)
    out.append(('constructed_a.dwg', ref.build(a),
                'pages of 70,912 and 66,688 bytes and 273 codewords, holes, a zero-length page, a page past its '
                'section, a section short of its size, free pages, a nonzero CRC seed, pages out of order'))
    b = {'seed': 4, 'crc_seed': 0x2AA, 'factors': (1, 1), 'compress_maps': False, 'compress_header': False, 'sections': [
        {'name': 'A', 'data': b'', 'encoding': 4, 'size': 0, 'pages': []},
        {'name': 'AcDb:Nil', 'data': b'', 'encoding': 1, 'size': 0, 'pages': [{'start': 0, 'len': 0, 'compress': False}]},
        {'name': 'AcDb:' + 'L' * 57, 'data': sample(30000, 31), 'encoding': 4, 'pages': [{'start': 0, 'len': 30000, 'compress': False}]},
        {'name': 'AcDb:Bare', 'data': sample(20000, 32), 'encoding': 1, 'pages': [{'start': 0, 'len': 20000, 'compress': True}]},
        {'name': 'AcDb:Two', 'data': sample(5000, 33), 'encoding': 1,
         'pages': [{'start': 0, 'len': 2500, 'compress': False}, {'start': 2500, 'len': 2500, 'compress': True}]},
    ]}
    out.append(('constructed_b.dwg', ref.build(b), 'system pages and header stored, repeat factor 1, a compressed bare page, a bare page of zero bytes in a '
                'zero-sized page map slot, a 126-byte section name that straddles a codeword'))
    return out


def load_corpus(local):
    """The pinned R2007 drawings (from a `--local` directory by hash, else fetched by hash), any other
    R2007 drawing in a local directory, and the built files: (corpus, pinned, extra, pinned_local, built, shapes)."""
    local_files = {}
    for directory in local:
        for path in sorted(directory.rglob('*.dwg')):
            data = path.read_bytes()
            if data[:6] == b'AC1021':
                local_files.setdefault(hashlib.sha256(data).hexdigest(), (path.name, data))
    pinned_digests, corpus = set(), []
    for manifest_name in MANIFESTS:
        manifest = json.loads((Path(__file__).resolve().parent / manifest_name).read_text())
        for item in manifest['files']:
            name = Path(item['name']).name
            pinned_digests.add(item['sha256'])
            if item['sha256'] in local_files:
                corpus.append((name, local_files[item['sha256']][1]))
                continue
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((name, fetch(url, item['bytes'], item['sha256'])))
    pinned = len(corpus)
    extra = [(f'local_{name}', data) for digest, (name, data) in sorted(local_files.items(), key=lambda kv: kv[1][0])
             if digest not in pinned_digests]
    local_pinned = sorted(name for digest, (name, _) in local_files.items() if digest in pinned_digests)
    corpus += extra
    built = constructed()
    corpus += [(name, data) for name, data, _ in built]
    return corpus, pinned, extra, local_pinned, built, {name: shape for name, _, shape in built}


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
    with tempfile.TemporaryDirectory(prefix='dwg-r2007-') as tmp:
        tmp = Path(tmp)
        counter = [0]

        def fresh():
            counter[0] += 1
            path = tmp / f'out{counter[0]}'
            path.mkdir()
            return path

        on_disk = {}

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            if on_disk.get(name) is not data:
                path.write_bytes(data)
                on_disk[name] = data
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=600)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        corpus, pinned, extra, local_pinned, built, shapes = load_corpus(local)
        files = dict(corpus)

        totals = {'files': 0, 'sections': 0, 'pages': 0, 'data_codewords': 0, 'bytes': 0}
        expectations = {}
        for name, data in corpus:
            parsed = ref.parse(data)
            lines = ref.expected(parsed)
            done = ref.expected(parsed, True)[len(lines):]
            blobs = [s['blob'] for s in parsed['sections']]
            expectations[name] = (parsed, lines, done, blobs)
            out = fresh()
            plain = run(data)
            assert plain[0] == 0 and plain[1].splitlines() == lines, (name, plain[2], 'walk differs from the independent parse')
            written = run(data, '--out', out)
            assert written[0] == 0 and written[1].splitlines() == lines + done, (name, written[2])
            for i, blob in enumerate(blobs):
                assert (out / f'{i}.bin').read_bytes() == blob, (name, i, 'assembled bytes differ from the independent parse')
            legacy_out = fresh()
            legacy = run(data, '--legacy', '--out', legacy_out)
            assert legacy[0] == 0, (name, legacy[2])
            seen = legacy[1].splitlines()
            assert [l for l in seen if not l.startswith('{"section_done"')] == lines, (name, 'legacy lines differ')
            assert [l for l in seen if l.startswith('{"section_done"')] == done, (name, 'legacy sections differ')
            for i, blob in enumerate(blobs):
                assert (legacy_out / f'{i}.bin').read_bytes() == blob, (name, i, 'legacy bytes differ')
            small = len(data) < BIG
            fixed = ((64,), ('--chunks', '1,7,256,3', '--pending'), ('--would-block',)) + (((7,), (1,), (3,)) if small else ())
            for args in fixed:
                assert run(data, *args) == plain, (name, args)
            pages = sum(len(s['pages']) for s in parsed['sections'])
            words = sum(p['codewords'] for s in parsed['sections'] for p in s['pages'])
            totals['files'] += 1
            totals['sections'] += len(blobs)
            totals['pages'] += pages
            totals['data_codewords'] += words
            totals['bytes'] += sum(len(b) for b in blobs)
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'sections': len(blobs), 'pages': pages,
                       'data_codewords': words, 'page_map_entries': len(parsed['entries']),
                       'oracles': ['independent Python parse', 'legacy r2007::Container'], 'fixed_schedules': len(fixed) + 1}
            if name in shapes:
                receipt['shape'] = shapes[name]
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'r2007-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=3600)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)
        biggest = max(p['uncompressed'] for parsed, *_ in expectations.values() for s in parsed['sections'] for p in s['pages'])
        widest = max(p['codewords'] for parsed, *_ in expectations.values() for s in parsed['sections'] for p in s['pages'])
        assert biggest > 65536 and widest > 255, (biggest, widest)

        # One checkpoint across the whole walk, resumed in a fresh process at stops chosen around every section,
        # page and the end of the walk (and at every step of the tiny file).
        checkpoint, first, second, full_trace = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl', tmp / 'full.jsonl'
        saved, covered = {}, set()
        for name in RESUMED:
            data = files[name]
            parsed, lines, done, blobs = expectations[name]
            identity = hashlib.sha256(data).hexdigest()
            holes = any('"hole"' in l for l in lines)
            configs = ((256, 'every'), (3, 'thirteenth')) if name == 'tiny.dwg' else ((256, 'sampled'),) + (((7, 'sampled'),) if len(data) <= BIG else ())
            for chunk, how in configs:
                full = run(data, chunk, '--trace', full_trace, '--report-steps', '--out', fresh(), name=name)
                reads = full_trace.read_text().splitlines()
                report = [json.loads(l) for l in full[2].splitlines()]
                steps = next(r['steps'] for r in report if 'steps' in r)
                events = [r['step'] for r in report if 'event' in r]
                if how == 'every':
                    stops = list(range(steps + 1))
                elif how == 'thirteenth':
                    stops = list(range(0, steps + 1, 13))
                else:
                    around = {0, 1, 2, steps - 1, steps}
                    for e in events:
                        around.update((e - 1, e, e + 1))
                    around = {s for s in around if 0 <= s <= steps}
                    early = set(range(0, events[0] + 1, max(1, events[0] // 16)))
                    around |= early
                    pick = sorted(around) if len(around) <= 90 else random.Random(f'{name}{chunk}').sample(sorted(around), 24) + sorted(early)
                    stops = sorted(set(pick) | {0, steps} | set(random.Random(f'{name}{chunk}x').sample(range(steps), 24)))
                for stop in stops:
                    out = fresh()
                    if holes:
                        for i in range(len(blobs)):
                            (out / f'{i}.bin').write_bytes(b'X' * 70000)
                    paused = run(data, chunk, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop,
                                 '--trace', first, '--out', out, name=name)
                    assert paused[0] == 0 and paused[1].splitlines()[-1] == ref.jline({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                    state = checkpoint.read_bytes()
                    stage, readers = checkpoint_readers(state)
                    saved.setdefault((name, stage), (stop, state, Path(str(checkpoint) + '.caller').read_text()))
                    for key, r in readers.items():
                        covered.add((stage, key, r['pass'], r['lz']))
                    resumed = run(data, chunk, '--identity', identity, '--resume', checkpoint, '--trace', second, '--out', out, name=name)
                    assert resumed[0] == 0, (name, stop, resumed[2])
                    assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == full[1].splitlines(), (name, stop, 'output not exactly once')
                    assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
                    for i, blob in enumerate(blobs):
                        assert (out / f'{i}.bin').read_bytes() == blob, (name, stop, i, 'section bytes after resume')
                receipts.append({'file': name, 'chunk': chunk, 'steps': steps, 'checkpoint_stops': len(stops),
                                 'fresh_process_resume': True, 'source_rereads': 0, 'duplicate_output': 0})
        stages = {stage for (_, stage) in saved}
        assert stages == {0, 1, 2, 3, 4, 5}, stages
        passes = {(stage, key, p) for stage, key, p, _ in covered}
        for need in ((1, 'aux', 0), (1, 'aux', 1), (2, 'sec', 0), (2, 'sec', 1), (3, 'aux', 2), (4, 'aux', 0), (4, 'aux', 1)):
            assert need in passes, ('no checkpoint was taken in', need)
        modes = sorted({m for _, _, _, m in covered})
        assert {0, 7, 8, 9, 10, 11} <= set(modes), ('decoder modes checkpointed', modes)

        # Corrupt, truncated and forged checkpoints are rejected before use.
        name = 'arc_2007.dwg'
        data = files[name]
        identity = hashlib.sha256(data).hexdigest()
        by_stage = {stage: saved[(name, stage)] for stage in stages}

        def prepare(stage):
            _, state, caller = by_stage[stage]
            checkpoint.write_bytes(state)
            Path(str(checkpoint) + '.caller').write_text(caller)
            return state

        def attempt(state, digest=identity):
            checkpoint.write_bytes(state)
            return run(data, 7, '--identity', digest, '--resume', checkpoint, name=name)

        smallest = min(stages, key=lambda s: len(by_stage[s][1]))
        small = prepare(smallest)
        for at in range(len(small)):
            bad = bytearray(small)
            bad[at] ^= 0x01
            result = attempt(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in range(len(small)):
            assert attempt(small[:length])[0] != 0, ('truncated checkpoint accepted', length)
        result = attempt(small, digest='f' * 64)
        assert result[0] != 0 and 'SourceMismatch' in result[2], result
        rng = random.Random('flips')
        walking = prepare(4)
        for at in sorted(rng.sample(range(len(walking)), 100)):
            bad = bytearray(walking)
            bad[at] ^= 0x01
            result = attempt(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in sorted(rng.sample(range(len(walking)), 100)):
            assert attempt(walking[:length])[0] != 0, ('truncated checkpoint accepted', length)

        forgeries = []
        for label, stage in (('head', 0), ('map', 1), ('sections', 2), ('lookup', 3), ('page', 4), ('done', 5)):
            state = by_stage[stage][1]
            forgeries += [
                (f'{label}: unknown stage', state, [(STAGE, b'\x09')]),
                (f'{label}: unknown record kind', state, [(REC_KIND, b'\x07')]),
                (f'{label}: unknown pending', state, [(PENDING, b'\x09')]),
                (f'{label}: facts flags', state, [(FLAGS, b'\x07')]),
                (f'{label}: entry bytes held past 16', state, [(ENTRY_HAVE, b'\x11')]),
                (f'{label}: record bytes held past 64', state, [(REC_HAVE, b'\x41')]),
                (f'{label}: name bytes held past 128', state, [(REC_HAVE, b'\x81'), (REC_KIND, b'\x01')]),
                (f'{label}: a stage that does not follow', state, [(STAGE, bytes([(stage + 2) % 6]))]),
            ]
        for stage in (1, 2, 3, 4, 5):
            state = by_stage[stage][1]
            forgeries += [
                (f'stage {stage}: header bytes held', state, [(HAVE, p16(7))]),
                (f'stage {stage}: the header corrupted', state, [(VARIABLE + 100, bytes([state[VARIABLE + 100] ^ 0x55]))]),
                (f'stage {stage}: sections described that were not', state, [(DESCRIBED, p32(1 << 20))]),
                (f'stage {stage}: bytes emitted out of nothing', state, [(EMITTED, p64(1 << 40))]),
                (f'stage {stage}: page map entries counted that were not', state, [(FACTS, p64(0xFFFF))]),
            ]
        forgeries.append(('map: pages walked before the first page', by_stage[1][1], [(PAGES, p32(1 << 20))]))
        for stage in (2, 3, 4):
            state = by_stage[stage][1]
            forgeries += [
                (f'stage {stage}: odd name length', state, [(CUR + 5, b'\x03')]),
                (f'stage {stage}: name longer than the buffer', state, [(CUR + 5, b'\xfe')]),
                (f'stage {stage}: unknown encoding', state, [(CUR_SIZES + 32, p64(3))]),
                (f'stage {stage}: encrypted section', state, [(CUR_SIZES + 16, p64(1))]),
                (f'stage {stage}: more pages done than listed', state, [(CUR_SIZES + 48, p64(1 << 30))]),
            ]
        for stage in (3, 4):
            state = by_stage[stage][1]
            forgeries += [
                (f'stage {stage}: page id zero', state, [(REF + 16, p64(0))]),
                (f'stage {stage}: compressed bigger than uncompressed', state, [(REF + 32, p64(1 << 40))]),
            ]
        for stage, key in ((1, 'aux'), (2, 'sec'), (3, 'aux'), (3, 'sec'), (4, 'aux'), (4, 'sec')):
            state = by_stage[stage][1]
            _, readers = checkpoint_readers(state)
            r, at = readers[key], readers[key]['at']
            forgeries += [
                (f'stage {stage} {key}: unknown pass', state, [(at + R_PASS, b'\x09')]),
                (f'stage {stage} {key}: a pass the stage cannot be in', state, [(at + R_PASS, bytes([2 if r['pass'] != 2 else 0]))]),
                (f'stage {stage} {key}: codewords read past the page', state, [(at + R_CW, p64(1 << 40))]),
                (f'stage {stage} {key}: bytes held past the codeword', state, [(at + R_HAVE, p16(300))]),
                (f'stage {stage} {key}: stored bytes fed that were not', state, [(at + R_FED, p64(1 << 40))]),
                (f'stage {stage} {key}: bytes produced past the page', state, [(at + R_PRODUCED, p64(1 << 40))]),
            ]
        rejected, accepted = [], []
        for label, state, edit in forgeries:
            prepare(next(s for s in stages if by_stage[s][1] is state))
            checkpoint.write_bytes(reseal(state, edit))
            result = run(data, 7, '--identity', identity, '--resume', checkpoint, name=name)
            if result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2]:
                rejected.append(label)
            else:
                accepted.append((label, result[2][:200]))
        assert not accepted, ('resealed forgeries not rejected', accepted)
        # A resealed change to any one byte never panics: it is rejected, or it is a state the walk can go on from.
        fuzz = 0
        for stage in sorted(stages):
            state = by_stage[stage][1]
            prepare(stage)
            for at in sorted(random.Random(f'fuzz{stage}').sample(range(5, len(state) - 4), min(30, len(state) - 9))):
                bad = bytearray(state[:-4])
                bad[at] ^= random.Random(at).randrange(1, 256)
                checkpoint.write_bytes(bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad))))
                result = run(data, 256, '--identity', identity, '--resume', checkpoint, name=name)
                assert 'panicked' not in result[2] and result[0] in (0, 1), (stage, at, result[2][:300])
                fuzz += 1
        receipts.append({'file': name, 'checkpoint_stages': sorted(stages), 'bit_flips_rejected': len(small) + 100,
                         'truncations_rejected': len(small) + 100, 'identity_mismatch': 'rejected',
                         'resealed_forgeries_rejected': len(rejected), 'resealed_mutations_without_panic': fuzz,
                         'checkpointed_decoder_modes': modes})

        problems = []

        def must_fail(label, bad, error, prefix=None, nm='input.dwg'):
            result = run(bad, name=nm)
            if not (result[0] != 0 and error in result[2] and 'panicked' not in result[2]):
                problems.append((label, error, result[2][:300]))
            elif prefix is not None and result[1].splitlines() != prefix:
                problems.append((label, 'output is not exactly the walk up to the failure'))
            return label

        # Corruptions of real files, each ending in its named error with exactly the walk before it printed.
        labels = []
        for name in ('arc_2007.dwg', 'sample_AC1021.dwg'):
            data = files[name]
            parsed, lines, done, blobs = expectations[name]
            f = parsed['fields']
            entries = len(parsed['entries'])

            def flip(at, bit=0x01, d=data):
                bad = bytearray(d)
                bad[at] ^= bit
                return bytes(bad)

            pm_base, pm_blocks = ref.PAGE_BASE + f[7], ref.system_blocks(min(f[10], f[11]), f[3])
            sm_ent = next(e for e in parsed['entries'] if abs(e[0]) == f[24])
            sm_base, sm_blocks = ref.PAGE_BASE + sm_ent[2], ref.system_blocks(min(f[22], f[25]), f[27])
            cases = [
                ('another release magic', flip(4, 0x03), 'InvalidMagic', []),
                ('the R2004 magic', data[:4] + b'18' + data[6:], 'UnsupportedLayout', []),
                ('the 0x80 at 0x28', flip(0x28), 'R2007Header(40)', []),
                ('header block, codeword 1', flip(0x80 + 3 * 10 + 1), 'R2007Parity { at: 128, codeword: 1 }', []),
                ('header block, parity of codeword 2', flip(0x80 + 3 * 250 + 2), 'R2007Parity { at: 128, codeword: 2 }', []),
                ('check data, normal CRC', flip(0x80 + 0x3D8), 'R2007Crc64 { at: 128, check: 3,', []),
                ('check data, mirrored CRC', flip(0x80 + 0x3E0), 'R2007Crc64 { at: 128, check: 4,', []),
                ('check data, a random value', flip(0x80 + 0x3E8), 'R2007Crc64 { at: 128, check: 3,', []),
                ('page map, early byte', flip(pm_base + 100),
                 f'R2007Parity {{ at: {pm_base}, codeword: {100 % pm_blocks} }}', lines[:1]),
                ('page map, last codeword', flip(pm_base + pm_blocks * 254 + pm_blocks - 1),
                 f'R2007Parity {{ at: {pm_base}, codeword: {pm_blocks - 1} }}', lines[:1]),
                ('section map', flip(sm_base + 77), f'R2007Parity {{ at: {sm_base}, codeword: {77 % sm_blocks} }}', lines[:1 + entries]),
            ]
            data_pages = [p for s in parsed['sections'] for p in s['pages'] if p['codewords'] >= 3 and p['uncompressed'] > 100]
            for p in (data_pages[0], max(data_pages, key=lambda p: p['codewords'])):
                base, blocks = p['address'], p['codewords']
                upto = next(i for i, l in enumerate(lines) if l.startswith('{"page":') and json.loads(l)['page']['id'] == p['id'])
                rows = [('first byte', 0), ('middle', blocks * 100 + 1), ('parity row', blocks * 253 + blocks - 1)]
                pad = p['compressed'] - (blocks - 1) * 251
                if pad < 251:
                    rows.append(('message padding', pad * blocks + blocks - 1))
                for label, off in rows:
                    cases.append((f'page of {blocks} codewords, {label}', flip(base + off),
                                  f'R2007Parity {{ at: {base}, codeword: {off % blocks} }}', lines[:upto]))
            bare = next(p for s in parsed['sections'] for p in s['pages'] if not p['codewords'] and p['uncompressed'] > 50)
            upto = next(i for i, l in enumerate(lines) if l.startswith('{"page":') and json.loads(l)['page']['id'] == bare['id'])
            cases.append(('bare page', flip(bare['address'] + 20), f"R2007Crc64 {{ at: {bare['address']}, check: 7,", lines[:upto]))
            for label, bad, error, prefix in cases:
                labels.append(must_fail(f'{name}: {label}', bad, error, prefix))
            if name == 'arc_2007.dwg':
                # What the format leaves unprotected is not checked: the random padding after the header's codewords.
                assert run(flip(0x80 + 800)) == run(data), 'header padding is read'
                labels.append('the header block\'s random padding is not covered')
        assert not problems, problems
        receipts.append({'file': 'arc_2007.dwg and sample_AC1021.dwg', 'corruptions_rejected': labels})

        # Rules broken alone in a built file, every codeword still intact.
        labels = []

        def case(label, error, mutate=None, **kw):
            spec = small_spec()
            if mutate:
                mutate(spec['sections'])
            spec.update(kw)
            labels.append(must_fail(label, ref.build(spec), error, None, 'built.dwg'))

        def page(section, index=0, **kw):
            return lambda sections: sections[section]['pages'][index].update(kw)

        def section(index, **kw):
            return lambda sections: sections[index].update(kw)

        case('header CRC', 'R2007Crc64 { at: 128, check: 2,', tamper={'hdr_crc': 1})
        case('header compressed data CRC', 'R2007Crc64 { at: 128, check: 1,', tamper={'ccrc': 1})
        case('header checking sequence CRC', 'R2007Crc64 { at: 128, check: 0,', tamper={'key_crc': 1})
        case('header check data', 'R2007Crc64 { at: 128, check: 3,', tamper={'check_normal': 1})
        case('header check data, mirrored', 'R2007Crc64 { at: 128, check: 4,', tamper={'check_mirror': 1})
        case('header size field', 'R2007Header(8192)', fields={0: 0x71})
        case('0x70 field', 'R2007Header(8304)', fields={14: 0x21})
        case('stream version field', 'R2007Header(8424)', fields={29: 0x60101})
        case('file size past the source', 'R2007Header(8200)', fields={1: 1 << 40})
        case('page map repeat factor zero', 'R2007Header(8216)', fields={3: 0})
        case('section map repeat factor zero', 'R2007Header(8408)', fields={27: 0})
        case('second header past the source', 'R2007Header(8264)', fields={9: 1 << 40})
        case('page map stored CRC', 'R2007Crc64 {', tamper={'pm_crc_comp': 1})
        case('page map decompressed CRC', 'R2007Crc64 {', tamper={'pm_crc_unc': 1})
        case('section map stored CRC', 'R2007Crc64 {', tamper={'sm_crc_comp': 1})
        case('section map decompressed CRC', 'R2007Crc64 {', tamper={'sm_crc_unc': 1})
        case('page count off', 'PageEntry(', fields={12: 99})
        case('largest page id off', 'PageEntry(', fields={13: 99})
        case('section map id the page map lacks', 'SectionMapMissing', fields={24: 9999})
        case('sections amount off', 'SectionEntry(', fields={20: 99})
        case('page map without its own entry', 'PageMapSelf', fields={8: 9999})
        case('page checksum', 'R2007Checksum {', page(0, 0, sum_delta=1))
        case('page stored CRC', 'R2007Crc64 {', page(0, 1, crc_delta=1))
        case('bare page stored CRC', 'R2007Crc64 {', page(2, 0, crc_delta=1))
        case('encrypted section', 'SectionEncrypted(1)', section(1, encryption=1))
        case('unknown encoding', 'SectionEntry(0)', section(0, enc_field=3))
        case('page the page map lacks', 'R2007PageMissing(', page(1, 0, drop=True))
        case('page entry too small for its codewords', 'R2007Page(', page(1, 0, entry_size=0x20))
        case('compressed bigger than uncompressed', 'SectionEntry(', page(2, 0, comp=5000, unc=100))
        case('page overlapping the one before', 'SectionEntry(', page(0, 1, doff=10))
        case('odd name length', 'SectionEntry(', section(0, name_raw=b'A\0B'))
        case('name that is not UTF-16', 'SectionEntry(', section(0, name_raw=b'\x00\xd8A\x00\x00\x00'))
        case('name longer than the walker reads', 'SectionEntry(', section(0, name_raw=b'A\0' * 70))
        case('empty name with pages', 'SectionEntry(', section(2, name_raw=b'\0\0\0\0'))
        case('page map entry that is not a multiple of 0x20', 'PageEntry(',
             pmap_edit=lambda b: b[:48] + struct.pack('<q', 0x21) + b[56:])
        case('page map entry with id zero', 'PageEntry(', pmap_edit=lambda b: b[:56] + bytes(8) + b[64:])
        case('page map that ends inside an entry', 'MapLength(', pmap_edit=lambda b: b + b'\0' * 5)
        case('section map that ends inside a descriptor', 'MapLength(', smap_edit=lambda b: b + b'\0' * 5)
        case('page whose stream reaches before its start', 'Lz77Offset {',
             mutate=page(2, 0, stored=bytes([0x20, 0, 0, 4]) + b'abcd' + bytes([0x31, 0x18]), unc=100))
        case('page whose copy overruns it', 'Lz77Overrun {',
             mutate=page(2, 0, stored=bytes([0x0E]) + bytes(22) + bytes([0xF1, 0x00]), unc=30))
        case('page whose stream ends early', 'Lz77Truncated {',
             mutate=page(2, 0, stored=bytes([0x20, 0, 0, 4]) + b'abcd', unc=100))
        # A source cut short: the header is made to accept a smaller file than there is.
        good = ref.build(dict(small_spec(), fields={1: 0, 9: 0}))
        parsed = ref.parse(good)
        last = max((p for s in parsed['sections'] for p in s['pages']), key=lambda p: p['address'])
        f = parsed['fields']
        sm_address = ref.PAGE_BASE + next(e for e in parsed['entries'] if abs(e[0]) == f[24])[2]
        for label, cut in (('inside the head', 0x47F), ('inside the section map', sm_address + 10),
                           ('inside a data page', last['address'] + 10)):
            labels.append(must_fail(f'cut {label}', good[:cut], 'Truncated {', None, 'built.dwg'))
        assert not problems, problems
        receipts.append({'file': 'a built file', 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 sections 5.1-5.4 and 5.10-5.13: the whole R2007 walk behind one state and one '
                       'checkpoint: file header, page map, section map, then every data page of every section in the '
                       'order its section lists them, each page walked twice (every codeword, CRC-64 and checksum '
                       'checked before any byte is handed out). No object decode.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'pinned_files': pinned,
              'local_files': len(extra), 'local_files_that_are_pinned': local_pinned,
              'built_files': len(built), 'largest_page_bytes': biggest, 'widest_page_codewords': widest,
              'totals': totals, 'cases': receipts, 'status': 'passed'}
    Path('r2007-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
