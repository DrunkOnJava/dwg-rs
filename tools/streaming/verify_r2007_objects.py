#!/usr/bin/env python3
"""Black-box acceptance for the R2007 (AC1021) object stream (ODA 5.4.1 section 20.1), never cargo
test: the whole-file walk with AcDb:AcDbObjects framed into records in section order, each record
equal to the independent Python framing, to the AcDb:Handles map and to the legacy reader.
Usage: verify_r2007_objects.py <stream_r2007_objects binary> [<streaming_schedules binary>] [--local DIR ...]"""
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
import r2007_ref as ref  # noqa: E402
import verify_objects as vo  # noqa: E402
import verify_r2007 as vr  # noqa: E402

OBJECTS = 'AcDb:AcDbObjects'
FORMAT = 'r2000'
RESUMED = ('tiny_objects.dwg', 'arc_2007.dwg', 'ellipse_2007.dwg', 'sample_AC1021.dwg')
SCHEDULED = ('arc_2007.dwg', 'paged_mx61.dwg')
BIG = 400000

# Byte offsets in a DWGQ checkpoint (src/streaming/checkpoint.rs): the framer's fields as in a DWGO
# checkpoint, then the unframed bytes, then the whole DWGR checkpoint of the file walker.
FOUND, SECTION, FMT, TOTAL, AT, STAGE, START, SIZE, WORDS, MS, MS_LEN = 5, 6, 10, 11, 19, 27, 28, 36, 40, 41, 45
MC, MC_LEN, MC_DONE, DONE, CRC, FIELD, FIELD_LEN, HEAD, RECORDS, PENDING_LEN, PENDING = 46, 54, 55, 56, 60, 62, 64, 65, 97, 105, 107


def line(value):
    return json.dumps(value, separators=(',', ':'))


def inner_at(state):
    """Where the file walker's checkpoint starts, and its length."""
    held = struct.unpack_from('<H', state, PENDING_LEN)[0]
    at = PENDING + held
    return at + 4, struct.unpack_from('<I', state, at)[0]


def reseal(state, edits, inner=()):
    """A DWGQ checkpoint with outer bytes edited (offset, value), then bytes of the embedded DWGR
    checkpoint edited (offset within it, value), both CRC-32 trailers recomputed."""
    bad = bytearray(state[:-4])
    if inner:
        start, length = inner_at(state)
        part = bytearray(bad[start:start + length - 4])
        for at, value in inner:
            part[at:at + len(value)] = value
        bad[start:start + length] = bytes(part) + struct.pack('<I', zlib.crc32(bytes(part)))
    for at, value in edits:
        bad[at:at + len(value)] = value
    return bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad)))


def pinned_corpus(local):
    """The pinned R2007 drawings (from a `--local` directory by hash, else fetched by hash) and any
    other R2007 drawing in a local directory: (pinned, extra)."""
    local_files = {}
    for directory in local:
        for path in sorted(directory.rglob('*.dwg')):
            data = path.read_bytes()
            if data[:6] == b'AC1021':
                local_files.setdefault(hashlib.sha256(data).hexdigest(), (path.name, data))
    pinned_digests, corpus = set(), []
    for manifest_name in vr.MANIFESTS:
        manifest = json.loads((Path(__file__).resolve().parent / manifest_name).read_text())
        for item in manifest['files']:
            name = Path(item['name']).name
            pinned_digests.add(item['sha256'])
            if item['sha256'] in local_files:
                corpus.append((name, local_files[item['sha256']][1]))
                continue
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((name, vr.fetch(url, item['bytes'], item['sha256'])))
    extra = [(f'local_{name}', data) for digest, (name, data) in sorted(local_files.items(), key=lambda kv: kv[1][0])
             if digest not in pinned_digests]
    return corpus, extra


def records_prefix(blob, records, limit):
    """The object stream up to the last record that ends within `limit` bytes, and those records."""
    count = max(i for i, r in enumerate(records) if r['end'] <= limit) + 1
    return blob[:records[count - 1]['end']], records[:count]


def build(objects, mx=None, encoding=4, compress=None, order=None, size=None, gap=None, names=(OBJECTS,), encryption=0, **spec_extra):
    """A whole file: one section before the object section, the object section(s) in pages of `mx` bytes, one
    after. `gap` drops the page that starts at that multiple of `mx` from the section map's list (a stretch no
    page covers); `size` overrides the object section's size."""
    mx = mx or 60000
    starts = list(range(0, len(objects), mx)) or [0]
    pages = []
    for start in starts:
        page = {'start': start, 'len': min(mx, len(objects) - start)}
        if compress is not None:
            page['compress'] = compress
        if gap is not None and start == gap * mx:
            continue
        pages.append(page)
    sections = [{'name': 'AcDb:Preview', 'data': vr.sample(1500, 41), 'encoding': 4, 'pages': [{'start': 0, 'len': 1500}]}]
    for name in names:
        sections.append({'name': name, 'data': objects, 'encoding': encoding, 'pages': [dict(p) for p in pages],
                         'size': len(objects) if size is None else size, 'encryption': encryption})
    sections.append({'name': 'AcDb:AppInfo', 'data': vr.sample(300, 42), 'encoding': 1,
                     'pages': [{'start': 0, 'len': 300, 'compress': False}]})
    spec = {'seed': 7, 'crc_seed': 0x155, 'sections': sections}
    if order == 'shuffle':
        count = sum(len(s['pages']) for s in sections)
        spec['order'] = random.Random(5).sample(range(count), count)
    spec.update(spec_extra)
    return ref.build(spec)


def expected_stream(data):
    """What a correct walk prints, from the independent parse, decoder and framing."""
    parsed = ref.parse(data)
    section = next(s for s in parsed['sections'] if s['name'] == OBJECTS)
    records = vo.frame(section['blob'], FORMAT)
    finished = next(l for l in ref.expected(parsed) if l.startswith('{"finished"'))
    lines = [vo.object_line(r) for r in records]
    lines += [line({'objects': {'records': len(records), 'bytes': len(section['blob'])}}), finished]
    return lines, records, parsed, section


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
    with tempfile.TemporaryDirectory(prefix='dwg-r2007-objects-') as tmp:
        tmp = Path(tmp)
        on_disk = {}

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            if on_disk.get(name) is not data:
                path.write_bytes(data)
                on_disk[name] = data
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=900)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        pinned_files, extra = pinned_corpus(local)
        by = dict(pinned_files + extra)
        real = pinned_files + extra

        # The real object streams of a built file: the first records of a drawing's, so every CRC, type
        # and handle is real, repacked into pages of any size and among other sections.
        arc_lines, arc_records, arc_parsed, arc_section = expected_stream(by['arc_2007.dwg'])
        full = arc_section['blob']

        def prefix(limit):
            return records_prefix(full, arc_records, limit)

        # Built files whose pages put every field of a record on a boundary.
        def straddle(records, wanted):
            """A page size and a record whose MS (or whose CRC) starts on the last byte of a page, so that field
            is split across two pages."""
            for mx in range(16, 90):
                for t, r in enumerate(records):
                    if t > 0 and wanted == 'ms' and r['start'] % mx == mx - 1:
                        return mx, t
                    if t > 0 and wanted == 'crc' and (r['end'] - 2) % mx == mx - 1:
                        return mx, t
            raise AssertionError('no page size splits that field')

        small_blob, small_records = prefix(3000)
        tiny_blob, tiny_records = prefix(140)
        assert len(small_records) >= 12 and len(tiny_records) >= 2, (len(small_records), len(tiny_records))
        built = [('tiny_objects.dwg', build(tiny_blob, mx=70)),
                 ('paged_mx61.dwg', build(small_blob, mx=61)),
                 ('stored_pages.dwg', build(small_blob, mx=700, encoding=1, compress=False)),
                 ('bare_compressed_pages.dwg', build(small_blob, mx=900, encoding=1, compress=True)),
                 ('shuffled_pages.dwg', build(small_blob, mx=256, order='shuffle')),
                 ('whole_section.dwg', build(small_blob, mx=70000))]
        windows = {}
        for wanted in ('ms', 'crc'):
            mx, t = straddle(small_records, wanted)
            label = f'split_{wanted}_mx{mx}.dwg'
            windows[label] = (wanted, t)
            built.append((label, build(small_blob, mx=mx)))
        built_notes = {'tiny_objects.dwg': 'two records in pages of 70 bytes, every step checkpointed',
                       'paged_mx61.dwg': 'the real first records of arc_2007.dwg in pages of 61 bytes (records and fields straddle pages)',
                       'stored_pages.dwg': 'the object section in stored pages (encoding 1, no compression)',
                       'bare_compressed_pages.dwg': 'the object section in bare compressed pages (encoding 1)',
                       'shuffled_pages.dwg': 'pages in a shuffled order in the file and the page map',
                       'whole_section.dwg': 'the object section in one page'}
        for label, (wanted, t) in windows.items():
            built_notes[label] = f'a page size that leaves record {t}\'s {wanted.upper()} on the last byte of a page'
        corpus = real + built
        by.update(dict(built))

        totals = {'files': 0, 'records': 0, 'object_bytes': 0, 'payload_bytes': 0}
        expectations = {}
        for name, data in corpus:
            lines, records, parsed, section = expected_stream(data)
            expectations[name] = (lines, records, section)
            plain = run(data)
            assert plain[0] == 0 and plain[1].splitlines() == lines, (name, plain[2], 'stream differs from the independent framing')
            is_real = name in dict(real)
            if is_real:
                handles = next(s for s in parsed['sections'] if s['name'] == 'AcDb:Handles')['blob']
                pairs = vo.handle_map(handles)
                assert sorted(pairs, key=lambda p: p[1]) == [(r['handle'], r['start']) for r in records], \
                    (name, 'the handle map and the stream disagree about the records')
                legacy = run(data, '--legacy')
                assert legacy[0] == 0 and not any('skipped' in l for l in legacy[1].splitlines()), (name, legacy[2])
                streamed = sorted(l for l in plain[1].splitlines() if l.startswith('{"object"'))
                assert sorted(l for l in legacy[1].splitlines() if l.startswith('{"object"')) == streamed, \
                    (name, 'records differ from the legacy reader')
            big = len(data) > BIG
            fixed = ((13,), (256,), ('--chunks', '1,7,256,3', '--pending'), ('--would-block',)) if big else \
                ((1,), (3,), (64,), ('--chunks', '1,7,256,3', '--pending'), ('--would-block',))
            for args in fixed:
                assert run(data, *args) == plain, (name, args)
            totals['files'] += 1
            totals['records'] += len(records)
            totals['object_bytes'] += len(section['blob'])
            totals['payload_bytes'] += sum(len(r['payload']) for r in records)
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'records': len(records),
                       'object_section_bytes': len(section['blob']), 'object_pages': len(section['pages']),
                       'fixed_schedules': len(fixed) + 1}
            if is_real:
                receipt['oracles'] = ['independent Python framing with CRC checks', 'AcDb:Handles map',
                                      'legacy DwgFile::all_objects_lossy (handle, offset, MS, type, payload length, payload CRC-32)']
            else:
                receipt['oracles'] = ['independent Python framing with CRC checks']
                receipt['shape'] = built_notes[name]
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'r2007-objects-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=3600)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)
        straddling = 0
        for name, _ in real:
            _, records, section = expectations[name]
            bounds = [p['data_offset'] for p in section['pages']][1:]
            straddling += sum(1 for r in records for b in bounds if r['start'] < b < r['end'])
        assert straddling > 20, straddling
        receipts.append({'records_straddling_a_page_boundary_in_pinned_drawings': straddling})

        # One checkpoint across the whole walk, resumed in a fresh process.
        checkpoint, first, second, full_trace = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl', tmp / 'full.jsonl'
        saved = {}
        coverage = {'framer': set(), 'walker': set(), 'pending': 0, 'ms_split': 0, 'crc_split': 0, 'body': 0, 'resumes': 0}
        resumable = [(n, by[n]) for n in RESUMED] + [(n, by[n]) for n in windows]
        for name, data in resumable:
            expected = expectations[name][0]
            identity = hashlib.sha256(data).hexdigest()
            chunk = 256 if len(data) > BIG else 7
            reported = run(data, chunk, '--trace', full_trace, '--report-steps', name=name)
            assert reported[0] == 0 and reported[1].splitlines() == expected, (name, reported[2])
            reads = full_trace.read_text().splitlines()
            report = [json.loads(l) for l in reported[2].splitlines()]
            steps = next(r['steps'] for r in report if 'steps' in r)
            events = [r['step'] for r in report if 'event' in r]
            objects_at = [r['step'] for r in report if r.get('event') == 'object']
            if name == 'tiny_objects.dwg':
                step = max(1, steps // 500)
                stops = sorted(set(range(0, steps + 1, step)) | {steps})
            else:
                around = {0, 1, 2, steps - 1, steps}
                for e in events[:20] + events[-8:] + random.Random(name + 'e').sample(events, min(len(events), 60)):
                    around.update((e - 2, e - 1, e, e + 1))
                forced = set()
                if name in windows:
                    wanted, t = windows[name]
                    forced = set(range(objects_at[t - 1], objects_at[t - 1] + 50)) if wanted == 'ms' else set(range(objects_at[t] - 50, objects_at[t] + 1))
                around |= set(range(0, events[0] + 1, max(1, events[0] // 16)))
                around = sorted(s for s in around if 0 <= s <= steps)
                wanted_n = 14 if len(data) > BIG else 60
                stops = sorted((set(random.Random(name).sample(around, min(len(around), wanted_n))) | {0, steps} | forced
                                | set(random.Random(name + 'x').sample(range(steps), 6 if len(data) > BIG else 12)))
                               & set(range(steps + 1)))
            for stop in stops:
                paused = run(data, chunk, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop,
                             '--trace', first, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                state = checkpoint.read_bytes()
                start, _ = inner_at(state)
                walker_stage = state[start + 5]
                coverage['walker'].add(walker_stage)
                held = struct.unpack_from('<H', state, PENDING_LEN)[0]
                if state[FOUND]:
                    coverage['framer'].add(state[STAGE])
                    coverage['pending'] += held > 0
                    coverage['ms_split'] += state[STAGE] == 1 and state[MS_LEN] % 2 == 1
                    coverage['crc_split'] += state[STAGE] == 3 and state[FIELD_LEN] == 1
                    coverage['body'] += state[STAGE] == 2 and state[DONE] > 0
                    saved.setdefault((name, state[STAGE], held > 0, walker_stage),
                                     (stop, state, Path(str(checkpoint) + '.caller').read_text()))
                else:
                    saved.setdefault((name, 'unfound', False, walker_stage), (stop, state, Path(str(checkpoint) + '.caller').read_text()))
                resumed = run(data, chunk, '--identity', identity, '--resume', checkpoint, '--trace', second, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == expected, (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
                coverage['resumes'] += 1
            receipts.append({'file': name, 'steps': steps, 'checkpoint_stops': len(stops), 'fresh_process_resume': True,
                             'source_rereads_beyond_design': 0, 'duplicate_output': 0})
        assert coverage['framer'] == {0, 1, 2, 3, 4, 5}, coverage
        assert coverage['walker'] == {0, 1, 2, 3, 4, 5}, coverage
        assert coverage['pending'] and coverage['ms_split'] and coverage['crc_split'] and coverage['body'], coverage
        receipts.append({'checkpoint_framer_stages': sorted(coverage['framer']), 'checkpoint_walker_stages': sorted(coverage['walker']),
                         'checkpoints_holding_unframed_bytes': coverage['pending'], 'checkpoints_inside_an_MS': coverage['ms_split'],
                         'checkpoints_inside_a_CRC': coverage['crc_split'], 'checkpoints_inside_a_payload': coverage['body'],
                         'fresh_process_resumes': coverage['resumes']})

        # Corrupt, truncated and forged checkpoints are rejected before use.
        def attempt(state, source, source_name, digest=None):
            checkpoint.write_bytes(state)
            return run(source, 7, '--identity', digest or hashlib.sha256(source).hexdigest(), '--resume', checkpoint, name=source_name)

        def stored(key):
            _, state, caller = saved[key]
            Path(str(checkpoint) + '.caller').write_text(caller)
            return state

        forge, forge_data = 'tiny_objects.dwg', by['tiny_objects.dwg']
        forge_id = hashlib.sha256(forge_data).hexdigest()
        keys = sorted((k for k in saved if k[0] == forge), key=str)
        real_name = 'arc_2007.dwg'
        real_key = next(k for k in sorted(saved, key=str) if k[0] == real_name and k[1] == 2 and k[3] == 4)
        rng = random.Random('flips')
        flipped = truncated = 0
        for label, key, source, source_name in (('the tiny file between records', next(k for k in keys if k[1] == 1), forge_data, forge),
                                                ('the tiny file inside a record', next(k for k in keys if k[1] == 2), forge_data, forge),
                                                ('a real drawing inside a record', real_key, by[real_name], real_name)):
            state = stored(key)
            for at in sorted(rng.sample(range(len(state)), min(len(state), 150))):
                bad = bytearray(state)
                bad[at] ^= 0x01
                result = attempt(bytes(bad), source, source_name)
                assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', label, at, result)
                flipped += 1
            for length in sorted(rng.sample(range(len(state)), min(len(state), 150))):
                assert attempt(state[:length], source, source_name)[0] != 0, ('truncated checkpoint accepted', label, length)
                truncated += 1
            result = attempt(state, source, source_name, digest='f' * 64)
            assert result[0] != 0 and 'SourceMismatch' in result[2], result
        # The walker's own checkpoint inside the framer's is checked as strictly: bytes flipped in it with both
        # CRC-32 trailers recomputed are refused or are a state the walk can go on from, never a panic. Flips
        # of the decoder's retained window and of the stored bytes in hand are not restorable invariants.
        inner_key = next(k for k in sorted(saved, key=str) if k[0] == real_name and k[3] == 1)
        state = stored(inner_key)
        start, length = inner_at(state)
        inner_flips = 0
        for at in sorted(rng.sample(range(length - 4), 200)):
            result = attempt(reseal(state, [], [(at, bytes([state[start + at] ^ 0x5a]))]), by[real_name], real_name)
            assert 'panicked' not in result[2] and result[0] in (0, 1), (at, result[2][:300])
            inner_flips += result[0] != 0 and 'InvalidCheckpoint' in result[2]
        assert inner_flips >= 100, inner_flips

        p8 = lambda v: bytes([v])
        p16 = lambda v: struct.pack('<H', v)
        p32 = lambda v: struct.pack('<I', v)
        p64 = lambda v: struct.pack('<Q', v)
        u64 = lambda state, at: struct.unpack_from('<Q', state, at)[0]
        forgeries = []
        for key in keys:
            _, state, _ = saved[key]
            if key[1] == 'unfound':
                continue
            label = f'framer stage {key[1]}{" with unframed bytes" if key[2] else ""}, walker stage {key[3]}'
            held = struct.unpack_from('<H', state, PENDING_LEN)[0]
            at = u64(state, AT)
            forgeries += [
                (f'{label}: unknown framer stage', state, [(STAGE, p8(9))]),
                (f'{label}: unknown record format', state, [(FMT, p8(7))]),
                (f'{label}: the R2010 record format', state, [(FMT, p8(2))]),
                (f'{label}: the R13 record format', state, [(FMT, p8(0))]),
                (f'{label}: unframed bytes past the buffer', state, [(PENDING_LEN, p16(300))]),
                (f'{label}: record start past the stream', state, [(START, p64((1 << 64) - 1))]),
                (f'{label}: field lengths of 255', state, [(WORDS, p8(255)), (MS_LEN, p8(255)), (FIELD_LEN, p8(255))]),
                (f'{label}: one word with a field length of 255', state, [(WORDS, p8(1)), (FIELD_LEN, p8(255))]),
                (f'{label}: section of 2^64 - 1 bytes', state, [(TOTAL, p64((1 << 64) - 1))]),
                (f'{label}: section size differing from the section map', state, [(TOTAL, p64(u64(state, TOTAL) + 1))]),
                (f'{label}: past the end of the section', state, [(AT, p64(u64(state, TOTAL) + 1))]),
                (f'{label}: section index not described yet', state, [(SECTION, p32(0x7777))]),
                (f'{label}: section index at u32::MAX', state, [(SECTION, p32(0xFFFFFFFF))]),
                (f'{label}: a framer begun on no section', state, [(TOTAL, p64(0))]),
            ]
            if key[3] in (2, 3, 4):
                forgeries += [(f'{label}: section not found while inside it', state, [(FOUND, p8(0))])]
            if key[1] in (1, 2, 3):
                forgeries += [
                    (f'{label}: record start off its bytes', state, [(START, p64(u64(state, START) + 1))]),
                    (f'{label}: more records counted than the bytes before it can hold', state, [(RECORDS, p64(1 << 40))]),
                ]
                if struct.unpack_from('<I', state, DONE)[0] <= 32:
                    forgeries += [(f'{label}: CRC that the held bytes do not give', state,
                                   [(CRC, p16(struct.unpack_from('<H', state, CRC)[0] ^ 0x55))])]
            if key[1] in (2, 3):
                forgeries += [
                    (f'{label}: MS size off its bytes', state, [(SIZE, p32(struct.unpack_from('<I', state, SIZE)[0] + 1))]),
                    (f'{label}: payload consumed past the record', state, [(DONE, p32(1 << 20))]),
                    (f'{label}: an R2010 MC in an R2000 record', state, [(MC_LEN, p8(1)), (MC_DONE, p8(1))]),
                    (f'{label}: held payload bytes the record has not reached', state,
                     [(HEAD + 31, p8(1))] if struct.unpack_from('<I', state, DONE)[0] < 32 else [(MC, p64(1 << 62))]),
                ]
            if key[1] in (4, 5):
                forgeries += [(f'{label}: bytes held after the stream ended', state, [(PENDING_LEN, p16(1))], (PENDING, b'\0'))]
            if key[2] and key[1] < 4:
                forgeries += [(f'{label}: unframed bytes the walker has not handed over', state, [(AT, p64(at + 1))])]
            if held and at > 0:
                forgeries += [(f'{label}: the walker ahead of the framer', state, [(AT, p64(at - 1))])]
        for key in keys:
            if key[1] == 'unfound':
                _, state, _ = saved[key]
                forgeries += [
                    (f'unfound, walker stage {key[3]}: found with an idle framer', state, [(FOUND, p8(1))]),
                    (f'unfound, walker stage {key[3]}: found with a section index', state, [(FOUND, p8(1)), (SECTION, p32(1))]),
                ]
        rejected, accepted = [], []
        for item in forgeries:
            label, state, edits, insert = item if len(item) == 4 else (*item, None)
            bad = bytearray(state[:-4])
            for at, value in edits:
                bad[at:at + len(value)] = value
            if insert:
                at, value = insert
                bad[at:at] = value
            checkpoint.write_bytes(bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad))))
            result = run(forge_data, 7, '--identity', forge_id, '--resume', checkpoint, name=forge)
            if result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2]:
                rejected.append(label)
            else:
                accepted.append((label, result[0], result[2][:160]))
        assert not accepted, ('resealed forgeries not rejected', accepted)
        receipts.append({'files': [forge, real_name], 'bit_flips_rejected': flipped, 'truncations_rejected': truncated,
                         'identity_mismatch': 'rejected', 'resealed_forgeries_rejected': len(rejected),
                         'resealed_flips_inside_the_walker_checkpoint': 200, 'of_which_rejected': inner_flips})

        # Every way the object stream can be wrong, each ending in its named error with exactly the valid
        # records before it committed.
        def mutate(blob, at, value):
            bad = bytearray(blob)
            bad[at:at + len(value)] = value
            return bytes(bad)

        def reseal_record(blob, r):
            bad = bytearray(blob)
            end = r['end'] - 2
            struct.pack_into('<H', bad, end, vo.crc16(bytes(bad[r['start']:end])))
            return bytes(bad)

        def with_counter_nibble(blob, r, value):
            bad = bytearray(blob)
            base = r['start'] + r['ms_len']
            for i in range(4):
                at = r['counter_at'] + i
                bit = (value >> (3 - i)) & 1
                bad[base + at // 8] = (bad[base + at // 8] & ~(0x80 >> (at % 8)) & 0xff) | (bit * (0x80 >> (at % 8)))
            return reseal_record(bytes(bad), r)

        def record_bytes(payload):
            body = struct.pack('<H', len(payload)) + payload
            return body + struct.pack('<H', vo.crc16(body))

        cases_run = []
        for source in ('arc_2007.dwg', 'ellipse_2007.dwg'):
            lines, records, section = expectations[source]
            blob = section['blob']
            eligible = [i for i, x in enumerate(records) if 60 <= x['size'] <= 1500]
            k = eligible[len(eligible) // 2]
            r, last = records[k], records[-1]
            payload_at = r['start'] + r['ms_len']
            at_k = f'RecordCrc {{ offset: {r["start"]},'
            cases = [
                ('record CRC flipped', mutate(blob, r['end'] - 2, bytes([blob[r['end'] - 2] ^ 1])), at_k, k),
                ('last payload byte flipped', mutate(blob, r['end'] - 3, bytes([blob[r['end'] - 3] ^ 1])), at_k, k),
                ('payload byte flipped past the held head', mutate(blob, payload_at + 40, bytes([blob[payload_at + 40] ^ 4])), at_k, k),
                ('prologue byte flipped', mutate(blob, 1, bytes([blob[1] ^ 1])), 'ObjectPrologue', 0),
                ('MS of zero', mutate(blob, r['start'], b'\0\0'), f'RecordSize({r["start"]})', k),
                ('MS of three words', mutate(blob, r['start'], b'\xff\xff\xff\xff'), f'RecordSize({r["start"]})', k),
                ('record past the end (MS grown)', mutate(blob, last['start'], struct.pack('<H', last['size'] + 100)),
                 f'RecordRange({last["start"]})', len(records) - 1),
                ('section cut inside the last record', blob[:-3], f'RecordRange({last["start"]})', len(records) - 1),
                ('zero bytes after the last record', blob + bytes(4), f'RecordSize({len(blob)})', len(records)),
                ('one byte after the last record', blob + b'\x01', 'ObjectsIncomplete', len(records)),
                ('type and handle unreadable (counter 9, CRC resealed)', with_counter_nibble(blob, r, 9),
                 f'RecordHeader({r["start"]})', k),
                ('record too short for its type and handle', vo.PROLOGUE + record_bytes(b'\x00'), 'RecordHeader(4)', 0),
                ('section shorter than the prologue', vo.PROLOGUE[:2], 'ObjectPrologue', 0),
            ]
            for label, bad, error, valid in cases:
                result = run(build(bad, compress=False), name='corrupt.dwg')
                assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (source, label, error, result[2])
                assert result[1].splitlines() == lines[:valid], (source, label, 'output is not exactly the records before the failure')
                cases_run.append(f'{source}: {label}')

        # Shapes only a built file has.
        lines, records, section = expectations['paged_mx61.dwg']
        mx = 61
        gap = 3
        shape_cases = [
            ('a stretch of the object section that no page covers (the section map lists no page for it)',
             build(small_blob, mx=mx, gap=gap), f'ObjectsOrder {{ expected: {gap * mx}, actual: {(gap + 1) * mx} }}'),
            ('the object section ends short of its size', build(small_blob, mx=mx, size=len(small_blob) + 40), 'ObjectsIncomplete('),
            ('no object section', build(small_blob, names=('AcDb:Other',)), 'ObjectsMissing'),
            ('two object sections', build(small_blob, mx=700, names=(OBJECTS, OBJECTS)), 'SectionEntry(2)'),
            ('an encrypted object section', build(small_blob, mx=700, encryption=1), 'SectionEncrypted(1)'),
        ]
        for label, bad, error in shape_cases:
            result = run(bad, name='built.dwg')
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, error, result[2])
            printed = result[1].splitlines()
            # The section map is the outer loop, so a second object section is only seen after the first was
            # framed to its end: its records and its `objects` line are printed, never the end of the walk.
            ended = ('"finished"',) if label == 'two object sections' else ('"objects"', '"finished"')
            assert printed == lines[:len(printed)] and not any(m in l for l in printed for m in ended), \
                (label, 'output is not a valid prefix')
            if label == 'two object sections':
                assert printed == lines[:-1], (label, 'the first object section is not framed to its end')
            cases_run.append(f'built: {label}')

        # A page of the object section that fails its own checks hands out nothing, and neither does any page
        # after it: the records committed are exactly those inside the pages before it.
        real_lines, real_records, real_section = expectations['arc_2007.dwg']
        pages = real_section['pages']
        arc = by['arc_2007.dwg']
        for label, page in (('second page', pages[1]), ('last page', pages[-1])):
            base, blocks = page['address'], page['codewords']
            bad = bytearray(arc)
            bad[base + blocks * 100 + 1] ^= 0x01
            error = f'R2007Parity {{ at: {base}, codeword: {(blocks * 100 + 1) % blocks} }}'
            result = run(bytes(bad), name='corrupt.dwg')
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, error, result[2])
            before = [vo.object_line(r) for r in real_records if r['end'] <= page['data_offset']]
            assert result[1].splitlines() == before, (label, 'a record of an unverified page was handed out')
            cases_run.append(f'arc_2007.dwg: a codeword of the object section\'s {label} flipped: {len(before)} records, then R2007Parity')
        big = by['sample_AC1021.dwg']
        big_lines, big_records, big_section = expectations['sample_AC1021.dwg']
        page = big_section['pages'][len(big_section['pages']) // 2]
        base, blocks = page['address'], page['codewords']
        bad = bytearray(big)
        bad[base + 9 * blocks + blocks - 1] ^= 0x10
        result = run(bytes(bad), name='corrupt_big.dwg')
        assert result[0] != 0 and f'R2007Parity {{ at: {base},' in result[2] and 'panicked' not in result[2], result[2]
        before = [vo.object_line(r) for r in big_records if r['end'] <= page['data_offset']]
        assert result[1].splitlines() == before and len(before) > 100, 'a record of an unverified page was handed out'
        cases_run.append(f'sample_AC1021.dwg: a parity byte of a middle object page flipped: {len(before)} records, then R2007Parity')
        receipts.append({'corruptions_rejected': cases_run})

    report = {'scope': 'ODA 5.4.1 section 20.1: the object records of an R2007 (AC1021) file framed in section order out of the '
                       'whole-file walk: MS, payload, CRC, type and handle, each record\'s bytes equal to the legacy ObjectWalker\'s '
                       'and its place equal to the AcDb:Handles map\'s. No object decode, no out-of-order or holed object pages '
                       '(refused), no encrypted section, no performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'pinned_files': len(pinned_files),
              'local_files': len(extra), 'built_files': len(built), 'totals': totals, 'cases': receipts, 'status': 'passed'}
    Path('r2007-objects-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
