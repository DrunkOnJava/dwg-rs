#!/usr/bin/env python3
"""Black-box acceptance that a checkpoint crosses runtimes, never cargo test.
A native build checkpoints the R2004 page map and section map walk and a
section's data-page walk; a WebAssembly kernel built from the identical
grammar (tools/streaming/wasm_resume.rs, run under node) restores it and
finishes; and the kernel checkpoints the map walk for the native build to
resume; the whole-file walker's one checkpoint, which carries its section table and
needs no page index, crosses both ways.
Usage: verify_wasm.py <stream_r2004_maps binary> <stream_r2004_sections binary> <stream_r2004_file binary> [--local DIR ...]"""
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_maps as vm  # noqa: E402
import verify_sections as vs  # noqa: E402

FILES = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
NATIVE_CHUNK = 7
KERNEL_CHUNKS = (1, 7, 64, 256)


def main():
    argv = sys.argv[1:]
    local = []
    while '--local' in argv:
        at = argv.index('--local')
        local.append(Path(argv[at + 1]))
        del argv[at:at + 2]
    maps_bin, sections_bin, file_bin = (str(Path(a).resolve()) for a in argv[:3])
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-wasm-') as tmp:
        tmp = Path(tmp)
        kernel = tmp / 'wasm_resume.wasm'
        built = subprocess.run(['rustc', '--edition=2024', '--crate-type', 'cdylib', '--target', 'wasm32-unknown-unknown',
                                '-C', 'opt-level=2', '-C', 'panic=abort', '-C', 'link-arg=-zstack-size=4194304', 'tools/streaming/wasm_resume.rs', '-o', str(kernel)],
                               capture_output=True, timeout=600)
        assert built.returncode == 0, built.stderr.decode()
        kernel_sha = hashlib.sha256(kernel.read_bytes()).hexdigest()
        pinned = {}
        for directory in local:
            for path in sorted(directory.rglob('*.dwg')):
                pinned.setdefault(path.name, path.read_bytes())
        corpus = {'synthetic_2018.dwg': Path('tests/fixtures/canonical/synthetic_2018.dwg').read_bytes()}
        manifest = json.loads(Path('tools/streaming/lz77-corpus.json').read_text())
        for item in manifest['files']:
            if item['name'] in FILES:
                data = pinned.get(item['name'])
                if data is None or hashlib.sha256(data).hexdigest() != item['sha256']:
                    url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
                    data = vm.fetch(url, item['bytes'], item['sha256'])
                corpus[item['name']] = data
        corpus['constructed.dwg'] = vs.constructed()
        pages_of_built = sum(len(x['pages']) for x in vs.constructed_sections())
        corpus['constructed_shuffled.dwg'] = vs.constructed(order=random.Random('order').sample(range(pages_of_built), pages_of_built))

        def native(binary, data, *args, name):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=300)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        def kernel_jobs(jobs):
            (tmp / 'jobs.json').write_text(json.dumps(jobs))
            result = subprocess.run(['node', 'tools/streaming/wasm_resume.mjs', str(kernel), str(tmp / 'jobs.json')],
                                    capture_output=True, timeout=1800)
            assert result.returncode == 0, result.stderr.decode()
            return json.loads(result.stdout)

        def lines(text):
            return text.splitlines()

        def pick(name, count, steps, around=()):
            if name == 'synthetic_2018.dwg' and steps < 200:
                return list(range(steps + 1))
            chosen = {0, 1, steps - 1, steps, *around}
            rng = random.Random(name)
            chosen.update(rng.sample(range(steps), min(count, steps)))
            return sorted(s for s in chosen if 0 <= s <= steps)

        for name, data in corpus.items():
            if name.startswith('constructed'):
                continue
            identity = hashlib.sha256(data).hexdigest()
            path = tmp / name
            path.write_bytes(data)
            full = native(maps_bin, data, NATIVE_CHUNK, name=name)
            assert full[0] == 0
            lo, hi = 0, 4096
            while lo < hi:
                mid = (lo + hi + 1) // 2
                probe = native(maps_bin, data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', tmp / 'probe.bin',
                               '--stop-after', mid, name=name)
                if '"checkpoint"' in probe[1]:
                    lo = mid
                else:
                    hi = mid - 1
            steps = lo
            stops = pick(name, 40, steps)
            native_resumed, jobs, paused_lines, native_ckpts = {}, [], {}, {}
            for stop in stops:
                ck = tmp / f'maps-{name}-{stop}.bin'
                paused = native(maps_bin, data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
                assert paused[0] == 0 and lines(paused[1])[-1] == vs.line({'checkpoint': True, 'steps': stop}), (name, stop)
                paused_lines[stop] = lines(paused[1])[:-1]
                native_ckpts[stop] = ck.read_bytes()
                resumed = native(maps_bin, data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0
                assert paused_lines[stop] + lines(resumed[1]) == lines(full[1]), (name, stop)
                native_resumed[stop] = lines(resumed[1])
                jobs.append({'kind': 'maps-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(ck),
                             'chunk': KERNEL_CHUNKS[stop % len(KERNEL_CHUNKS)]})
            results = kernel_jobs(jobs)
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == native_resumed[stop], (name, stop, 'kernel output differs from the native resume')
            # The kernel's own checkpoint, resumed natively.
            jobs = [{'kind': 'maps-run', 'file': str(path), 'digest': identity, 'stop': stop, 'chunk': NATIVE_CHUNK,
                     'checkpoint_out': str(tmp / f'wasm-{name}-{stop}.bin')} for stop in stops]
            results = kernel_jobs(jobs)
            identical = 0
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == paused_lines[stop], (name, stop, 'kernel output before its checkpoint differs')
                ck = tmp / f'wasm-{name}-{stop}.bin'
                identical += ck.read_bytes() == native_ckpts[stop]
                Path(str(ck) + '.caller').write_text(f'{stop}\n')
                resumed = native(maps_bin, data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert lines(result['out']) + lines(resumed[1]) == lines(full[1]), (name, stop, 'kernel checkpoint, native resume')
            assert identical == len(stops), (name, 'kernel and native checkpoints differ at the same step')
            receipts.append({'file': name, 'walk': 'page map and section map', 'steps': steps, 'stops': len(stops),
                             'native_to_wasm': 'finished equal to the native resume',
                             'wasm_to_native': 'finished equal to the uninterrupted run',
                             'checkpoints_byte_identical_across_runtimes': identical,
                             'kernel_chunk_sizes': list(KERNEL_CHUNKS)})

        for name, data in corpus.items():
            if name == 'constructed_shuffled.dwg':
                continue
            identity = hashlib.sha256(data).hexdigest()
            path = tmp / name
            path.write_bytes(data)
            expected, _, plan = vs.assemble(data)
            done = [l for l in expected if l.startswith('{"section_done"')]
            full = native(sections_bin, data, NATIVE_CHUNK, '--report-steps', name=name)
            assert full[0] == 0
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = report[-1]['steps']
            around = {e + d for r in report[:-1] for e in (r['step'],) for d in (-1, 0, 1)}
            stops = pick(name, 30, steps, around) if name != 'synthetic_2018.dwg' else list(range(steps + 1))
            if len(stops) > 90:
                stops = sorted(set(random.Random(name + 's').sample(stops, 90)) | {0, steps})
            jobs, native_resumed, kinds, stage_at = [], {}, set(), {}
            for stop in stops:
                ck = tmp / f'sec-{name}-{stop}.bin'
                paused = native(sections_bin, data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
                assert paused[0] == 0 and lines(paused[1])[-1] == vs.line({'checkpoint': True, 'steps': stop}), (name, stop)
                stage_at[stop] = ck.read_bytes()[vs.STAGE]
                kinds.add(stage_at[stop])
                resumed = native(sections_bin, data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert lines(paused[1])[:-1] + lines(resumed[1]) == lines(full[1]), (name, stop)
                native_resumed[stop] = lines(resumed[1])
                jobs.append({'kind': 'section-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(ck),
                             'sidecar': str(ck) + '.caller', 'chunk': KERNEL_CHUNKS[stop % len(KERNEL_CHUNKS)]})
            results = kernel_jobs(jobs)
            finished = 0
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                got = lines(result['out'])
                assert got == native_resumed[stop][:len(got)], (name, stop, 'kernel output differs from the native resume')
                if stage_at[stop] == 4:
                    assert got == [], (name, stop, 'a finished section checkpoint produced output')
                    continue
                # Every other checkpoint is an unfinished section: the kernel
                # must run it through its section_done, equal to the
                # independent assembly's.
                assert got and got[-1].startswith('{"section_done"') and got[-1] in done, \
                    (name, stop, 'kernel did not finish the section, or finished it differently from the independent assembly')
                finished += 1
            assert kinds >= ({0, 1, 2, 3} if name != 'synthetic_2018.dwg' else {0, 2, 3}), (name, kinds)
            assert finished == sum(1 for v in stage_at.values() if v != 4) and finished > 0, (name, finished)
            receipts.append({'file': name, 'walk': 'data pages of a section', 'steps': steps, 'stops': len(stops),
                             'checkpoint_stages': sorted(kinds), 'sections_finished_by_the_kernel': finished,
                             'native_to_wasm': 'finished equal to the native resume and the independent assembly',
                             'kernel_chunk_sizes': list(KERNEL_CHUNKS)})

        # The whole-file walker: one checkpoint, no page index, both ways.
        for name, data in corpus.items():
            identity = hashlib.sha256(data).hexdigest()
            path = tmp / name
            path.write_bytes(data)
            full = native(file_bin, data, NATIVE_CHUNK, '--report-steps', name=name)
            assert full[0] == 0
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = report[-1]['steps']
            around = {e + d for r in report[:-1] for e in (r['step'],) for d in (-1, 0, 1)}
            full_lines = lines(full[1])
            stops = pick(name, 25, steps, around)
            if len(stops) > 60:
                stops = sorted(set(random.Random(name + 'f').sample(stops, 60)) | {0, steps})
            jobs, resumed_lines, paused_lines, native_ckpts, cursors = [], {}, {}, {}, {}
            for stop in stops:
                ck = tmp / f'file-{name}-{stop}.bin'
                paused = native(file_bin, data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
                assert paused[0] == 0 and lines(paused[1])[-1] == vs.line({'checkpoint': True, 'steps': stop}), (name, stop)
                paused_lines[stop] = lines(paused[1])[:-1]
                native_ckpts[stop] = ck.read_bytes()
                caller = Path(str(ck) + '.caller').read_text().split('\n')
                cursors[stop] = tuple(int(v) for v in caller[0].split(' ')[1:4])
                resumed = native(file_bin, data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused_lines[stop] + lines(resumed[1]) == full_lines, (name, stop)
                resumed_lines[stop] = lines(resumed[1])
                crc, length, number = cursors[stop]
                jobs.append({'kind': 'file-resume', 'file': str(path), 'digest': identity, 'checkpoint': str(ck),
                             'chunk': KERNEL_CHUNKS[stop % len(KERNEL_CHUNKS)], 'page_crc': crc, 'page_len': length, 'number': number})
            results = kernel_jobs(jobs)
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == resumed_lines[stop], (name, stop, 'kernel output differs from the native resume')
            jobs = [{'kind': 'file-run', 'file': str(path), 'digest': identity, 'stop': stop, 'chunk': NATIVE_CHUNK,
                     'checkpoint_out': str(tmp / f'wasm-file-{name}-{stop}.bin')} for stop in stops]
            results = kernel_jobs(jobs)
            identical = 0
            for stop, result in zip(stops, results):
                assert result['code'] >= 0, (name, stop, result)
                assert lines(result['out']) == paused_lines[stop], (name, stop, 'kernel output before its checkpoint differs')
                ck = tmp / f'wasm-file-{name}-{stop}.bin'
                identical += ck.read_bytes() == native_ckpts[stop]
                assert tuple(result['cursors']) == cursors[stop], (name, stop, 'cursors differ')
                sections = [json.loads(l)['section'] for l in lines(result['out']) if l.startswith('{"section"')]
                crc, length, number = result['cursors']
                Path(str(ck) + '.caller').write_text(f'{stop} {crc} {length} {number}\n' + ''.join(
                    f"S {x['index']} {x['id']} {x['size']} {x['pages']} {x['name']}\n" for x in sections))
                resumed = native(file_bin, data, NATIVE_CHUNK, '--identity', identity, '--resume', ck, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert lines(result['out']) + lines(resumed[1]) == full_lines, (name, stop, 'kernel checkpoint, native resume')
            assert identical == len(stops), (name, 'kernel and native checkpoints differ at the same step')
            receipts.append({'file': name, 'walk': 'whole file, one checkpoint', 'steps': steps, 'stops': len(stops),
                             'native_to_wasm': 'finished equal to the native resume, from the checkpoint alone',
                             'wasm_to_native': 'finished equal to the uninterrupted run',
                             'checkpoints_byte_identical_across_runtimes': identical,
                             'kernel_chunk_sizes': list(KERNEL_CHUNKS)})

        # The kernel refuses what the native build refuses: a flipped bit in a
        # checkpoint made natively, on both walks.
        name = 'line_2004.dwg'
        data = corpus[name]
        identity = hashlib.sha256(data).hexdigest()
        path = tmp / name
        path.write_bytes(data)
        refused = {}
        for kind, binary, kernel_kind, stop in (('maps', maps_bin, 'maps-resume', 40), ('sections', sections_bin, 'section-resume', 2000),
                                               ('file', file_bin, 'file-resume', 6000)):
            ck = tmp / f'forge-{kind}.bin'
            native(binary, data, NATIVE_CHUNK, '--identity', identity, '--checkpoint-out', ck, '--stop-after', stop, name=name)
            state = ck.read_bytes()
            jobs = []
            positions = sorted(random.Random(kind).sample(range(len(state)), 60) + [0, len(state) - 1])
            for at in positions:
                bad = bytearray(state)
                bad[at] ^= 1
                target = tmp / f'forge-{kind}-{at}.bin'
                target.write_bytes(bytes(bad))
                if kind != 'maps':
                    Path(str(target) + '.caller').write_bytes(Path(str(ck) + '.caller').read_bytes())
                jobs.append({'kind': kernel_kind, 'file': str(path), 'digest': identity, 'checkpoint': str(target),
                             'sidecar': str(target) + '.caller', 'chunk': 7, 'page_crc': 0, 'page_len': 0, 'number': 0})
            jobs.append({'kind': kernel_kind, 'file': str(path), 'digest': 'f' * 64, 'checkpoint': str(ck),
                         'sidecar': str(ck) + '.caller', 'chunk': 7, 'page_crc': 0, 'page_len': 0, 'number': 0})
            results = kernel_jobs(jobs)
            assert all(r['code'] == -1 for r in results), (kind, [r['code'] for r in results])
            refused[kind] = len(jobs)
        receipts.append({'file': name, 'kernel_refuses_bit_flips_and_wrong_identity': refused})

    report = {'scope': 'A checkpoint made by a native build restored and finished by a wasm32 kernel built from the same '
                       'grammar, run under node, for the R2004 page map and section map walk and a section data-page walk; '
                       'and the kernel\'s map-walk checkpoint finished by the native build. No browser, throughput or '
                       'whole-walk claim.',
              'kernel_sha256': kernel_sha,
              'cases': receipts, 'status': 'passed'}
    Path('wasm-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
