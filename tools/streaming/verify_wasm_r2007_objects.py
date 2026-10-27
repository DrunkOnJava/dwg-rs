#!/usr/bin/env python3
"""Black-box acceptance that the R2007 object stream's checkpoint crosses runtimes, never cargo
test. A native build checkpoints the R2007 object stream (the whole-file walk with the object section
framed into records, one `DWGQ` checkpoint that carries the verified header, both readers, the framer and
the section bytes not yet framed); a WebAssembly kernel built from the identical grammar
(tools/streaming/wasm_resume.rs, run under node) restores it and finishes; and the kernel's own
checkpoint is finished by the native build.
Usage: verify_wasm_r2007_objects.py <stream_r2007_objects binary> [--local DIR ...]"""
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_r2007_objects as v7  # noqa: E402

FILES = ('line_2007.dwg', 'arc_2007.dwg', 'ellipse_2007.dwg')
BIG = ('sample_AC1021.dwg',)
KERNEL_CHUNKS = (1, 7, 64, 256)
FOUND, STAGE, PENDING_LEN = v7.FOUND, v7.STAGE, v7.PENDING_LEN


def line(value):
    return json.dumps(value, separators=(',', ':'))


def main():
    argv = sys.argv[1:]
    local = []
    while '--local' in argv:
        at = argv.index('--local')
        local.append(Path(argv[at + 1]))
        del argv[at:at + 2]
    binary = str(Path(argv[0]).resolve())
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-wasm-r2007-objects-') as tmp:
        tmp = Path(tmp)
        kernel = tmp / 'wasm_resume.wasm'
        built = subprocess.run(['rustc', '--edition=2024', '--crate-type', 'cdylib', '--target', 'wasm32-unknown-unknown',
                                '-C', 'opt-level=2', '-C', 'panic=abort', '-C', 'link-arg=-zstack-size=4194304',
                                'tools/streaming/wasm_resume.rs', '-o', str(kernel)], capture_output=True, timeout=900)
        assert built.returncode == 0, built.stderr.decode()
        kernel_sha = hashlib.sha256(kernel.read_bytes()).hexdigest()
        pinned, extra = v7.pinned_corpus(local)
        corpus = {name: data for name, data in pinned + extra if name in FILES + BIG}
        arc_lines, arc_records, _, arc_section = v7.expected_stream(corpus['arc_2007.dwg'])
        tiny_blob, _ = v7.records_prefix(arc_section['blob'], arc_records, 140)
        small_blob, _ = v7.records_prefix(arc_section['blob'], arc_records, 3000)
        corpus = {'tiny_objects.dwg': v7.build(tiny_blob, mx=70), 'paged_mx61.dwg': v7.build(small_blob, mx=61), **corpus}

        def native(data, *args, name):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=1800)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        def kernel_jobs(jobs):
            (tmp / 'jobs.json').write_text(json.dumps(jobs))
            result = subprocess.run(['node', 'tools/streaming/wasm_resume.mjs', str(kernel), str(tmp / 'jobs.json')],
                                    capture_output=True, timeout=3600)
            assert result.returncode == 0, result.stderr.decode()
            return json.loads(result.stdout)

        def lines(text):
            return text.splitlines()

        for name, data in corpus.items():
            identity = hashlib.sha256(data).hexdigest()
            path = tmp / name
            path.write_bytes(data)
            chunk = 256 if name in BIG else 7
            full = native(data, chunk, '--report-steps', name=name)
            assert full[0] == 0
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = next(r['steps'] for r in report if 'steps' in r)
            events = [r['step'] for r in report if 'event' in r]
            if name == 'tiny_objects.dwg':
                stops = sorted(set(range(0, steps + 1, max(1, steps // 400))) | {steps})
            else:
                around = {0, 1, steps - 1, steps}
                for e in random.Random(name + 'e').sample(events, min(len(events), 10 if name in BIG else 30)):
                    around.update((e - 1, e, e + 1))
                around |= set(range(0, events[0] + 1, max(1, events[0] // 10)))
                rng = random.Random(name)
                stops = sorted(set(rng.sample(sorted(a for a in around if 0 <= a <= steps), min(len(around), 10 if name in BIG else 40)))
                               | {0, steps} | set(rng.sample(range(steps), 6)))
            full_lines = lines(full[1])
            jobs, resumed_lines, paused_lines, native_ckpts, cursors, stages = [], {}, {}, {}, {}, set()
            for stop in stops:
                ck = tmp / f'r2007-objects-{name}-{stop}.bin'
                paused = native(data, chunk, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
                assert paused[0] == 0 and lines(paused[1])[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop)
                paused_lines[stop] = lines(paused[1])[:-1]
                native_ckpts[stop] = ck.read_bytes()
                state = native_ckpts[stop]
                inner, _ = v7.inner_at(state)
                stages.add((state[STAGE] if state[FOUND] else -1, state[inner + 5]))
                caller = Path(str(ck) + '.caller').read_text().split()
                cursors[stop] = (int(caller[1]), int(caller[2]))
                resumed = native(data, chunk, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused_lines[stop] + lines(resumed[1]) == full_lines, (name, stop)
                resumed_lines[stop] = lines(resumed[1])
                jobs.append({'kind': 'r2007-objects-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(ck),
                             'chunk': KERNEL_CHUNKS[stop % len(KERNEL_CHUNKS)], 'digest_crc': cursors[stop][0], 'len': cursors[stop][1]})
            for stop, result in zip(stops, kernel_jobs(jobs)):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == resumed_lines[stop], (name, stop, 'kernel output differs from the native resume')
            jobs = [{'kind': 'r2007-objects-run', 'file': str(path), 'digest': identity, 'stop': stop, 'chunk': chunk,
                     'checkpoint_out': str(tmp / f'wasm-r2007-objects-{name}-{stop}.bin')} for stop in stops]
            identical = 0
            for stop, result in zip(stops, kernel_jobs(jobs)):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == paused_lines[stop], (name, stop, 'kernel output before its checkpoint differs')
                ck = tmp / f'wasm-r2007-objects-{name}-{stop}.bin'
                identical += ck.read_bytes() == native_ckpts[stop]
                assert tuple(result['cursors']) == cursors[stop], (name, stop, 'cursors differ')
                digest, length = result['cursors']
                Path(str(ck) + '.caller').write_text(f'{stop} {digest} {length}\n')
                resumed = native(data, chunk, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert lines(result['out']) + lines(resumed[1]) == full_lines, (name, stop, 'kernel checkpoint, native resume')
            assert identical == len(stops), (name, 'kernel and native checkpoints differ at the same step')
            receipts.append({'file': name, 'walk': 'R2007 object stream, one checkpoint', 'steps': steps, 'stops': len(stops),
                             'checkpoint_states': sorted(f'framer stage {s}, walker stage {w}' if s >= 0 else f'section not reached, walker stage {w}'
                                                         for s, w in stages),
                             'native_to_wasm': 'finished equal to the native resume, from the checkpoint alone',
                             'wasm_to_native': 'finished equal to the uninterrupted run',
                             'checkpoints_byte_identical_across_runtimes': identical,
                             'kernel_chunk_sizes': list(KERNEL_CHUNKS)})

        # The kernel refuses what the native build refuses.
        name = 'arc_2007.dwg'
        data = corpus[name]
        identity = hashlib.sha256(data).hexdigest()
        path = tmp / name
        path.write_bytes(data)
        ck = tmp / 'forge.bin'
        full = native(data, 7, '--report-steps', name=name)
        total = next(json.loads(l)['steps'] for l in full[2].splitlines() if '"steps"' in l and '"event"' not in l)
        native(data, 7, '--identity', identity, '--checkpoint-out', ck, '--stop-after', total * 2 // 3, name=name)
        state = ck.read_bytes()
        jobs = []
        for at in sorted(random.Random('forge').sample(range(len(state)), 80) + [0, len(state) - 1]):
            bad = bytearray(state)
            bad[at] ^= 1
            target = tmp / f'forge-{at}.bin'
            target.write_bytes(bytes(bad))
            jobs.append({'kind': 'r2007-objects-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(target),
                         'chunk': 7, 'digest_crc': 0, 'len': 0})
        jobs.append({'kind': 'r2007-objects-resume', 'file': str(path), 'digest': 'f' * 64, 'checkpoint': str(ck),
                     'chunk': 7, 'digest_crc': 0, 'len': 0})
        results = kernel_jobs(jobs)
        assert all(r['code'] == -1 for r in results), [r['code'] for r in results]
        receipts.append({'file': name, 'kernel_refuses_bit_flips_and_wrong_identity': len(jobs)})

    report = {'scope': 'A DWGQ checkpoint of the R2007 object stream made by a native build restored and finished by a '
                       'wasm32 kernel built from the same grammar, run under node, and the kernel\'s checkpoint finished by the '
                       'native build. No browser or throughput claim.',
              'kernel_sha256': kernel_sha, 'cases': receipts, 'status': 'passed'}
    Path('wasm-r2007-objects-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
