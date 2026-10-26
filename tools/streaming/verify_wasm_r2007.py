#!/usr/bin/env python3
"""Black-box acceptance that an R2007 walk's one checkpoint crosses runtimes,
never cargo test. A native build checkpoints the whole walk (tools/streaming's
stream_r2007); a WebAssembly kernel built from the identical grammar
(tools/streaming/wasm_resume.rs, run under node) restores it from the
checkpoint alone and finishes; and the kernel checkpoints a walk for the
native build to resume.
Usage: verify_wasm_r2007.py <stream_r2007 binary> [--local DIR ...]"""
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_r2007 as v7  # noqa: E402

FILES = ('tiny.dwg', 'arc_2007.dwg', 'constructed_a.dwg', 'sample_AC1021.dwg')
NATIVE_CHUNK = 7
KERNEL_CHUNKS = (1, 7, 64, 256)


def main():
    argv = sys.argv[1:]
    local = []
    while '--local' in argv:
        at = argv.index('--local')
        local.append(Path(argv[at + 1]))
        del argv[at:at + 2]
    binary = str(Path(argv[0]).resolve())
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-wasm-r2007-') as tmp:
        tmp = Path(tmp)
        kernel = tmp / 'wasm_resume.wasm'
        built = subprocess.run(['rustc', '--edition=2024', '--crate-type', 'cdylib', '--target', 'wasm32-unknown-unknown',
                                '-C', 'opt-level=2', '-C', 'panic=abort', '-C', 'link-arg=-zstack-size=4194304',
                                'tools/streaming/wasm_resume.rs', '-o', str(kernel)], capture_output=True, timeout=900)
        assert built.returncode == 0, built.stderr.decode()
        kernel_sha = hashlib.sha256(kernel.read_bytes()).hexdigest()
        corpus = {name: data for name, data in v7.load_corpus(local)[0] if name in FILES}
        assert set(corpus) == set(FILES), set(corpus)

        def native(data, *args, name):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=600)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        def kernel_jobs(jobs):
            (tmp / 'jobs.json').write_text(json.dumps(jobs))
            result = subprocess.run(['node', 'tools/streaming/wasm_resume.mjs', str(kernel), str(tmp / 'jobs.json')],
                                    capture_output=True, timeout=3600)
            assert result.returncode == 0, result.stderr.decode()
            return json.loads(result.stdout)

        def caller(ck):
            head = Path(str(ck) + '.caller').read_text().split('\n')[0].split(' ')
            return tuple(int(x) for x in head[1:5])

        stage_seen = set()
        for name, data in corpus.items():
            identity = hashlib.sha256(data).hexdigest()
            path = tmp / name
            path.write_bytes(data)
            full = native(data, NATIVE_CHUNK, '--report-steps', name=name)
            assert full[0] == 0, full[2]
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = report[-1]['steps']
            around = {e + d for r in report[:-1] for e in (r['step'],) for d in (-1, 0, 1)}
            rng = random.Random(name)
            count = 30 if len(data) < v7.BIG else 14
            stops = sorted({0, 1, steps - 1, steps} | set(rng.sample(range(steps), count)) | set(rng.sample(sorted(around), min(count, len(around)))))
            if name == 'tiny.dwg':
                stops = sorted(set(stops) | set(range(0, steps + 1, 13)))
            full_lines = full[1].splitlines()
            jobs, resumed_lines, paused_lines, native_ckpts, cursors = [], {}, {}, {}, {}
            for stop in stops:
                ck = tmp / f'r2007-{name}-{stop}.bin'
                paused = native(data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == v7.ref.jline({'checkpoint': True, 'steps': stop}), (name, stop)
                paused_lines[stop] = paused[1].splitlines()[:-1]
                native_ckpts[stop] = ck.read_bytes()
                stage_seen.add(native_ckpts[stop][v7.STAGE])
                cursors[stop] = caller(ck)
                resumed = native(data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused_lines[stop] + resumed[1].splitlines() == full_lines, (name, stop)
                resumed_lines[stop] = resumed[1].splitlines()
                crc, length, section, page_id = cursors[stop]
                jobs.append({'kind': 'r2007-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(ck),
                             'chunk': KERNEL_CHUNKS[stop % len(KERNEL_CHUNKS)], 'page_crc': crc, 'page_len': length,
                             'section': section, 'id': page_id})
            results = kernel_jobs(jobs)
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert result['out'].splitlines() == resumed_lines[stop], (name, stop, 'kernel output differs from the native resume')
            # The kernel's own checkpoint, resumed natively.
            jobs = [{'kind': 'r2007-run', 'file': str(path), 'digest': identity, 'stop': stop, 'chunk': NATIVE_CHUNK,
                     'checkpoint_out': str(tmp / f'wasm-{name}-{stop}.bin')} for stop in stops]
            results = kernel_jobs(jobs)
            identical = 0
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert result['out'].splitlines() == paused_lines[stop], (name, stop, 'kernel output before its checkpoint differs')
                ck = tmp / f'wasm-{name}-{stop}.bin'
                identical += ck.read_bytes() == native_ckpts[stop]
                assert tuple(result['cursors']) == cursors[stop], (name, stop, 'cursors differ', result['cursors'], cursors[stop])
                crc, length, section, page_id = result['cursors']
                Path(str(ck) + '.caller').write_text(f'{stop} {crc} {length} {section} {page_id}\n')
                resumed = native(data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert result['out'].splitlines() + resumed[1].splitlines() == full_lines, (name, stop, 'kernel checkpoint, native resume')
            assert identical == len(stops), (name, 'kernel and native checkpoints differ at the same step')
            receipts.append({'file': name, 'walk': 'whole R2007 file, one checkpoint', 'steps': steps, 'stops': len(stops),
                             'native_to_wasm': 'finished equal to the native resume, from the checkpoint alone',
                             'wasm_to_native': 'finished equal to the uninterrupted run',
                             'checkpoints_byte_identical_across_runtimes': identical,
                             'kernel_chunk_sizes': list(KERNEL_CHUNKS)})
        assert stage_seen == {0, 1, 2, 3, 4, 5}, stage_seen

        # The kernel refuses what the native build refuses: a flipped bit in a checkpoint made natively,
        # a wrong identity, and a checkpoint of another file.
        name = 'arc_2007.dwg'
        data = corpus[name]
        identity = hashlib.sha256(data).hexdigest()
        path = tmp / name
        path.write_bytes(data)
        ck = tmp / 'forge.bin'
        native(data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', 1500, name=name)
        state = ck.read_bytes()
        positions = sorted(random.Random('forge').sample(range(len(state)), 80) + [0, len(state) - 1])
        jobs = []
        for at in positions:
            bad = bytearray(state)
            bad[at] ^= 1
            target = tmp / f'forge-{at}.bin'
            target.write_bytes(bytes(bad))
            jobs.append({'kind': 'r2007-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(target), 'chunk': 7,
                         'page_crc': 0, 'page_len': 0, 'section': 0, 'id': 0})
        jobs.append({'kind': 'r2007-resume', 'file': str(path), 'digest': 'f' * 64, 'checkpoint': str(ck), 'chunk': 7,
                     'page_crc': 0, 'page_len': 0, 'section': 0, 'id': 0})
        results = kernel_jobs(jobs)
        assert all(r['code'] == -1 for r in results), [r['code'] for r in results]
        receipts.append({'file': name, 'kernel_refuses_bit_flips_and_wrong_identity': len(jobs)})

    report = {'scope': 'A checkpoint of the whole R2007 walk made by a native build restored and finished by a wasm32 kernel '
                       'built from the same no_std grammar, run under node, from the checkpoint alone; and the kernel\'s '
                       'checkpoint finished by the native build. No browser, throughput or object-decode claim.',
              'kernel_sha256': kernel_sha, 'cases': receipts, 'status': 'passed'}
    Path('wasm-r2007-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
