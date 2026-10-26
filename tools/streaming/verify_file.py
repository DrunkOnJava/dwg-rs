#!/usr/bin/env python3
"""Black-box acceptance for the whole R2004-family file walk (§4.1, §4.3-§4.6),
never cargo test: one walker and one checkpoint over the header, the page map,
the section map and every data page in page-map order.
Usage: verify_file.py <stream_r2004_file binary> [<streaming_schedules binary>] [--local DIR ...]"""
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
import verify_sections as vs  # noqa: E402

SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
CORRUPTED = ('synthetic_2004.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')
FAMILY = (b'AC1018', b'AC1024', b'AC1027', b'AC1032')

# Byte offsets in a DWGF checkpoint (src/streaming/checkpoint.rs).
STAGE, SLOT_COUNT, SLOTS, SLOT = 5, 46, 47, 52
S_INDEX, S_ID, S_PAGE_COUNT, S_COMPRESSED, S_REFS, S_REF_DIGEST, S_SEEN, S_SEEN_DIGEST = 0, 4, 16, 24, 28, 32, 40, 44


def line(value):
    return json.dumps(value, separators=(',', ':'))


def u32(data, at):
    return struct.unpack_from('<I', data, at)[0]


def expected_file(data):
    """The exact stdout a correct walk prints (with --out), and every section's
    bytes, from the independent map parse, decoder and assembly."""
    records = [json.loads(l) for l in vm.oracle(data)]
    pm, pmid, smid = vm.layout(data)
    section_lines = [line({'section': {'index': r['section'], 'name': r['name'], 'size': r['size'],
                                       'pages': r['page_count'], 'id': r['section_id']}})
                     for r in records if 'section' in r and 'name' in r]
    walked, blobs, _ = vs.assemble(data)
    by_page, current = {}, None
    for text in walked:
        item = json.loads(text)
        if 'page' in item:
            current = item['page']['number']
            by_page[current] = [text]
        elif 'page_end' in item:
            by_page[current].append(line({'page_end': {'number': current, **{k: v for k, v in item['page_end'].items()
                                                                           if k not in ('section', 'number')}}}))
    order = [r['page'] for r in records if 'page' in r and 'size' in r and r['page'] > 0 and r['page'] not in (pmid, smid)]
    pages = [l for n in order for l in by_page[n]]
    done = [l for l in walked if l.startswith('{"section_done"')]
    finished = line({'finished': {'sections': len(section_lines), 'pages': len(order)}})
    return section_lines + pages + [finished] + done, blobs, order


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
    with tempfile.TemporaryDirectory(prefix='dwg-file-') as tmp:
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
        extra = [(f'local_{name}', data) for _, (name, data) in sorted(local_files.items(), key=lambda kv: kv[1][0])]
        corpus += extra
        sections_of_built = len(vs.constructed_sections())
        pages_of_built = sum(len(s['pages']) for s in vs.constructed_sections())
        corpus.append(('constructed.dwg', vs.constructed()))
        # The same pages in every other order the file can hold them, a
        # property the section-order walk of verify_sections never had.
        rng = random.Random('order')
        for label, order in (('reversed', list(range(pages_of_built))[::-1]),
                             ('shuffled', rng.sample(range(pages_of_built), pages_of_built))):
            corpus.append((f'constructed_{label}.dwg', vs.constructed(order=order)))

        totals = {'files': 0, 'sections': 0, 'pages': 0, 'bytes': 0}
        expectations = {}
        for name, data in corpus:
            expected, blobs, order = expected_file(data)
            expectations[name] = (expected, blobs, order)
            out = fresh()
            plain = run(data)
            assert plain[0] == 0 and plain[1].splitlines() == [l for l in expected if not l.startswith('{"section_done"')], \
                (name, plain[2], 'walk differs from the independent assembly')
            written = run(data, '--out', out)
            assert written[0] == 0 and written[1].splitlines() == expected, (name, written[2])
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
            totals['files'] += 1
            totals['sections'] += len(blobs)
            totals['pages'] += len(order)
            totals['bytes'] += sum(len(b) for b in blobs)
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'sections': len(blobs), 'pages': len(order),
                       'oracles': ['independent Python assembly', 'legacy section_map::read_section_payload'],
                       'fixed_schedules': 5}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'file-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=1800)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)
        built_order = expectations['constructed_shuffled.dwg'][2]
        assert built_order != sorted(built_order), 'the shuffled file is in order'

        # One checkpoint across the whole walk, resumed in a fresh process at
        # stops chosen around every section, page and the end of the walk.
        checkpoint, first, second, full_trace = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl', tmp / 'full.jsonl'
        saved = {}
        for name, data in corpus:
            if name not in RESUMED and not name.startswith('constructed'):
                continue
            expected, blobs, _ = expectations[name]
            identity = hashlib.sha256(data).hexdigest()
            full = run(data, 7, '--trace', full_trace, '--report-steps', '--report-header', '--out', fresh(), name=name)
            reads = full_trace.read_text().splitlines()
            report = [json.loads(l) for l in full[2].splitlines()]
            header = next(r for r in report if 'header' in r)
            steps = next(r['steps'] for r in report if 'steps' in r)
            events = [r['step'] for r in report if 'event' in r]
            if name == 'synthetic_2018.dwg':
                stops = list(range(steps + 1))
            else:
                around = {0, 1, 2, steps - 1, steps}
                for e in events:
                    around.update((e - 1, e, e + 1))
                around = sorted(s for s in around if 0 <= s <= steps)
                pick = random.Random(name).sample(around, min(len(around), 70))
                stops = sorted(set(pick) | {0, steps} | set(random.Random(name + 'x').sample(range(steps), 12)))
            for stop in stops:
                out = fresh()
                # Stale output from an earlier run sits where each section's
                # file will be: a section described after the resume must
                # start empty, so no stale byte survives in a hole.
                for i in range(len(blobs)):
                    (out / f'{i}.bin').write_bytes(b'X' * 70000)
                paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop,
                             '--trace', first, '--out', out, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                state = checkpoint.read_bytes()
                saved.setdefault((name, state[STAGE]), (stop, state, Path(str(checkpoint) + '.caller').read_text()))
                resumed = run(data, 7, '--identity', identity, '--resume', checkpoint, '--trace', second, '--out', out,
                              '--report-header', name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert json.loads(resumed[2].splitlines()[-1]) == header, (name, stop, 'the verified header is lost across the checkpoint')
                assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == full[1].splitlines(), \
                    (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
                for i, blob in enumerate(blobs):
                    assert (out / f'{i}.bin').read_bytes() == blob, (name, stop, i, 'section bytes after resume')
            receipts.append({'file': name, 'steps': steps, 'checkpoint_stops': len(stops), 'fresh_process_resume': True,
                             'source_rereads_beyond_design': 0, 'duplicate_output': 0,
                             'note': 'the page map is read twice and each data page three times (header peek, checksum pass, decoding pass) by design'})
        stages = {stage for (_, stage) in saved}
        assert stages == {0, 1, 2, 3, 4, 5}, stages

        # Corrupt, truncated and forged checkpoints are rejected before use.
        name, data = next(c for c in corpus if c[0] == 'line_2004.dwg')
        identity = hashlib.sha256(data).hexdigest()
        by_stage = {stage: saved[(name, stage)] for stage in stages}

        def attempt(state, digest=identity):
            checkpoint.write_bytes(state)
            return run(data, 7, '--identity', digest, '--resume', checkpoint, name=name)

        def prepare(stage):
            _, state, caller = by_stage[stage]
            checkpoint.write_bytes(state)
            Path(str(checkpoint) + '.caller').write_text(caller)
            return state
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
        walking = prepare(3)
        for at in sorted(rng.sample(range(len(walking)), 100)):
            bad = bytearray(walking)
            bad[at] ^= 0x01
            result = attempt(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in sorted(rng.sample(range(len(walking)), 100)):
            assert attempt(walking[:length])[0] != 0, ('truncated checkpoint accepted', length)

        def tail_offsets(state):
            n = state[SLOT_COUNT]
            entry = SLOTS + SLOT * n
            return {'entry': entry, 'peeked': entry + 16, 'peek': entry + 17, 'pages': entry + 49, 'maps_len': entry + 53}

        def resealed(state, edits, insert=None):
            bad = bytearray(state[:-4])
            for at, value in edits:
                bad[at:at + len(value)] = value
            if insert:
                at, value = insert
                bad[at:at] = value
            return bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad)))
        p32 = lambda v: struct.pack('<I', v)
        p64 = lambda v: struct.pack('<Q', v)
        slot0 = lambda state, field: SLOTS + field
        forgeries = []
        for stage_name, stage in (('pages', 1), ('peek', 2), ('walk', 3), ('verify', 4), ('done', 5)):
            _, state, _ = by_stage[stage]
            n = state[SLOT_COUNT]
            t = tail_offsets(state)
            last = SLOTS + SLOT * (n - 1)
            forgeries += [
                (f'{stage_name}: unknown stage', state, [(STAGE, b'\x09')], None),
                (f'{stage_name}: more sections than the table holds', state, [(SLOT_COUNT, b'\x41')], None),
                (f'{stage_name}: more references than pages', state, [(slot0(state, S_REFS), p32(1 << 20))], None),
                (f'{stage_name}: pages seen past references', state, [(slot0(state, S_SEEN), p32(1 << 20))], None),
                (f'{stage_name}: two sections with one id', state, [(SLOTS + SLOT + S_ID, state[SLOTS + S_ID:SLOTS + S_ID + 4])], None),
                (f'{stage_name}: section index off its place', state, [(slot0(state, S_INDEX), p32(5))], None),
                (f'{stage_name}: unknown compression', state, [(slot0(state, S_COMPRESSED), p32(3))], None),
                (f'{stage_name}: page count with no references listed', state, [(last + S_PAGE_COUNT, p32(1 << 20))], None),
            ]
        _, peek_state, _ = by_stage[2]
        t = tail_offsets(peek_state)
        forgeries += [
            ('peek: no page number', peek_state, [(t['entry'], p32(0))], None),
            ('peek: page outside the source', peek_state, [(t['entry'] + 8, p64(1 << 62))], None),
            ('peek: header longer than 0x20 read', peek_state, [(t['peeked'], b'\x21')], None),
            ('peek: bytes past the ones read', peek_state, [(t['peek'] + 31, b'\x01')], None),
        ]
        _, walk_state, _ = by_stage[3]
        t = tail_offsets(walk_state)
        n = walk_state[SLOT_COUNT]
        forgeries += [
            ('walk: page differs from the walker\'s', walk_state, [(t['entry'] + 4, p32(u32(walk_state, t['entry'] + 4) + 0x20))], None),
            ('walk: header bytes held in the peek buffer', walk_state, [(t['peeked'], b'\x01')], None),
            ('walk: page of a section that saw none', walk_state,
             [(SLOTS + SLOT * i + S_SEEN, p32(0)) for i in range(n)] + [(SLOTS + SLOT * i + S_SEEN_DIGEST, p64(0)) for i in range(n)], None),
        ]
        _, verify_state, _ = by_stage[4]
        t = tail_offsets(verify_state)
        forgeries += [('verify: a map walker kept', verify_state, [(t['maps_len'], p32(4))], (t['maps_len'] + 4, bytes(4)))]
        _, done_state, _ = by_stage[5]
        forgeries += [('done: digests that differ', done_state, [(SLOTS + S_SEEN_DIGEST, p64(1))], None)]
        _, maps_state, _ = by_stage[0]
        t = tail_offsets(maps_state)
        forgeries += [('maps: pages already walked', maps_state, [(t['pages'], p32(3))], None)]
        rejected = []
        for label, state, edits, insert in forgeries:
            checkpoint.write_bytes(resealed(state, edits, insert))
            result = run(data, 7, '--identity', identity, '--resume', checkpoint, name=name)
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2], (label, result)
            rejected.append(label)
        receipts.append({'file': name, 'checkpoint_stages': sorted(stages), 'bit_flips_rejected': len(small) + 100,
                         'truncations_rejected': len(small) + 100, 'identity_mismatch': 'rejected',
                         'resealed_forgeries_rejected': len(rejected)})

        # Every check on a data page reached from the page map, on each release
        # band: the peeked header, then the section walker's checks, then the
        # page-by-page accounting against the section map.
        for name, data in corpus:
            if name not in CORRUPTED:
                continue
            expected, _, order = expectations[name]
            _, _, plan = vs.assemble(data)
            lz = [p for p in plan if p['compressed'] == 2 and p['data'] >= 200]
            target = lz[0]
            at = target['address']
            other_ids = sorted({p['id'] for p in plan if p['id'] != target['id']})
            raw_flip = bytearray(data)
            raw_flip[at + 8] ^= 0x01
            data_flip = bytearray(data)
            data_flip[at + 0x20 + target['data'] // 2] ^= 0x01
            moved = target['start'] + target['max']
            cases = [
                ('header bit flip', bytes(raw_flip), f'DataPageHeaderChecksum {{ address: {at},'),
                ('data bit flip', bytes(data_flip), f'DataPageChecksum {{ address: {at},'),
                ('page type', vs.edit_page(data, at, fields=[(0, '<I', vs.PAGE_TYPE ^ 1)]), f'DataPage({at})'),
                ('section the section map does not describe', vs.edit_page(data, at, fields=[(4, '<I', 0x7777)]),
                 f'SectionPageUnknown({target["number"]})'),
                ('start offset the section map does not list', vs.edit_page(data, at, fields=[(0x10, '<Q', moved)]), 'SectionDigest'),
                ('data size', vs.edit_page(data, at, fields=[(8, '<I', target['data'] - 1)]), f'DataPageChecksum {{ address: {at},'),
            ]
            labels = []
            # The page map is walked to its end before the accounting, so a
            # page the section map does not list is printed (with its own
            # start) before the error; everything before it is the oracle's.
            first = next(i for i, l in enumerate(expected) if l.startswith('{"page":')
                         and json.loads(l)['page']['number'] == target['number'])
            for label, bad, error in cases:
                result = run(bad, name=name)
                assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (name, label, error, result[2])
                printed = result[1].splitlines()
                if error == 'SectionDigest':
                    assert printed[:first] == expected[:first] and len(printed) > first, (name, label, 'output before the page differs')
                else:
                    assert printed == expected[:len(printed)], (name, label, 'output is not a valid prefix')
                labels.append(label)
            receipts.append({'file': name, 'corruptions_rejected': labels})

        # Shapes only a built file has, each ending in its named error: a
        # listed page the page map lacks, one it lists twice, one no section
        # lists (of a known section and of an unknown one), more sections than
        # the table holds, and an encrypted section.
        built = vs.constructed
        exact = next(i for i, s in enumerate(vs.constructed_sections()) if s['id'] == 7)
        refs = []
        for s in vs.constructed_sections():
            refs.append(len(s['pages']))
        numbers = {}
        n = 0
        for s in vs.constructed_sections():
            for k, _ in enumerate(s['pages']):
                n += 1
                numbers[(s['id'], k)] = n
        known = vs.constructed_sections()[6]
        unknown = {'id': 0x7777, 'compressed': 2}
        many = [{'name': f'AcDb:S{i}', 'id': i + 1, 'compressed': 2, 'mx': 0x400, 'size': 0, 'encrypted': 0, 'pages': []}
                for i in range(65)]
        cases = [
            ('listed page missing from the page map', built(drop={numbers[(7, 1)]}), 'SectionDigest(6)'),
            ('page listed twice in the page map', built(dup=[numbers[(7, 1)]]), 'SectionDigest(6)'),
            ('page no section lists, of a known section', built(orphans=[(known, 3 * 0x400, vs.lcg(12, 100))]), 'SectionDigest(6)'),
            ('page of a section the map does not describe', built(orphans=[(unknown, 0, vs.lcg(13, 100))]), 'SectionPageUnknown('),
            ('more sections than the table holds', vs.build(many), 'SectionTableFull'),
            ('encrypted section', built(encrypted=1), 'SectionEncrypted(7)'),
        ]
        labels = []
        for label, bad, error in cases:
            result = run(bad, name='constructed.dwg')
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, error, result[2])
            labels.append(label)
        receipts.append({'file': 'constructed.dwg', 'corruptions_rejected': labels})

    report = {'scope': 'ODA 5.4.1 sections 4.1 and 4.3-4.6: the whole R2004-family walk behind one state and one '
                       'checkpoint: header, page map, section map, then every data page in page-map order, each '
                       'section\'s pages checked against its section map references by count and digest. No object '
                       'decode, R2007 or performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'pinned_files': pinned,
              'local_files': len(extra), 'totals': totals, 'cases': receipts, 'status': 'passed'}
    Path('file-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
