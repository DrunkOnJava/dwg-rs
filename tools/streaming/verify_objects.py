#!/usr/bin/env python3
"""Black-box acceptance for the R2004-family object stream (§20.1), never cargo
test: the whole-file walk with AcDb:AcDbObjects framed into records in section
order, each record's payload equal to what the legacy ObjectWalker yields.
Usage: verify_objects.py <stream_r2004_objects binary> [<streaming_schedules binary>] [--local DIR ...]"""
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
import verify_file as vf  # noqa: E402
import verify_maps as vm  # noqa: E402
import verify_sections as vs  # noqa: E402

SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg', 'sample_AC1018.dwg', 'sample_AC1032.dwg')
# Drawings of this size are walked in 256-byte reads and checkpointed at fewer
# stops: a debug build walks a megabyte in a few seconds, not in a hundred.
BIG = 300_000
FAMILY = {b'AC1018': 'r2000', b'AC1024': 'r2010', b'AC1027': 'r2010', b'AC1032': 'r2010'}
OBJECTS = 'AcDb:AcDbObjects'
PROLOGUE = bytes.fromhex('ca0d0000')

# Byte offsets in a DWGO checkpoint (src/streaming/checkpoint.rs).
FOUND, SECTION_ID, FORMAT, TOTAL, AT, STAGE, START, SIZE, WORDS, MS, MS_LEN = 5, 6, 10, 11, 19, 27, 28, 36, 40, 41, 45
MC, MC_LEN, MC_DONE, DONE, CRC, FIELD, FIELD_LEN, HEAD, RECORDS, PENDING_LEN, PENDING = 46, 54, 55, 56, 60, 62, 64, 65, 97, 105, 107


def line(value):
    return json.dumps(value, separators=(',', ':'))


def crc_table():
    table = []
    for n in range(256):
        value = n
        for _ in range(8):
            value = (value >> 1) ^ (0xa001 if value & 1 else 0)
        table.append(value)
    return table


TABLE = crc_table()


def crc16(data, seed=0xc0c1):
    value = seed
    for byte in data:
        value = (value >> 8) ^ TABLE[(value ^ byte) & 0xff]
    return value


class Bits:
    def __init__(self, data, byte):
        self.data, self.at = data, byte * 8

    def take(self, n):
        value = 0
        for _ in range(n):
            value = value << 1 | (self.data[self.at >> 3] >> (7 - (self.at & 7))) & 1
            self.at += 1
        return value


def prologue(payload, fmt, mc_len):
    """Type, handle and the bit where the handle's counter nibble sits."""
    bits = Bits(payload, mc_len)
    tag = bits.take(2)
    if fmt == 'r2010':
        if tag == 0:
            code = bits.take(8)
        elif tag == 1:
            code = bits.take(8) + 0x1f0
        else:
            low = bits.take(8)
            code = bits.take(8) << 8 | low
    elif tag == 0:
        low = bits.take(8)
        code = bits.take(8) << 8 | low
    else:
        code = {1: lambda: bits.take(8), 2: lambda: 0, 3: lambda: 256}[tag]()
    if fmt == 'r2000':
        bits.take(32)
    bits.take(4)
    counter_at = bits.at
    counter = bits.take(4)
    assert counter <= 8
    handle = 0
    for _ in range(counter):
        handle = handle << 8 | bits.take(8)
    return code, handle, counter_at


def frame(blob, fmt):
    """An independent framing of an object section: every record, with its CRC,
    its MC and the tiling of the section checked."""
    assert blob[:4] == PROLOGUE, 'prologue'
    at, records = 4, []
    while at < len(blob):
        start, size, words = at, 0, 0
        while True:
            word = blob[at] | blob[at + 1] << 8
            at += 2
            size |= (word & 0x7fff) << (15 * words)
            words += 1
            if not word & 0x8000:
                break
            assert words < 2, ('MS of three words', start)
        assert size > 0, ('zero MS', start)
        mc_len = mc = 0
        if fmt == 'r2010':
            j = at
            while True:
                mc |= (blob[j] & 0x7f) << (7 * (j - at))
                j += 1
                if not blob[j - 1] & 0x80:
                    break
            mc_len = j - at
            assert mc <= 8 * (mc_len + size), ('MC larger than the record', start)
        payload = blob[at:at + mc_len + size]
        assert len(payload) == mc_len + size, ('record past the end', start)
        at += mc_len + size
        assert crc16(blob[start:at]) == blob[at] | blob[at + 1] << 8, ('record CRC', start)
        at += 2
        code, handle, counter_at = prologue(payload, fmt, mc_len)
        records.append({'start': start, 'end': at, 'size': size, 'payload': payload, 'type': code, 'handle': handle,
                        'mc_len': mc_len, 'counter_at': counter_at, 'ms_len': 2 * words})
    assert at == len(blob), 'the records do not tile the section'
    return records


def object_line(r):
    return line({'object': r['handle'], 'offset': r['start'], 'size': r['size'], 'type': r['type'],
                 'len': len(r['payload']), 'crc32': zlib.crc32(r['payload'])})


def handle_map(blob):
    """(handle, offset) pairs of an AcDb:Handles section, parsed independently."""
    pairs, at = [], 0
    while True:
        n = struct.unpack_from('>H', blob, at)[0]
        if n <= 2:
            break
        assert crc16(blob[at:at + n]) == struct.unpack_from('>H', blob, at + n)[0], ('handle map CRC', at)
        q, handle, offset = at + 2, 0, 0
        while q < at + n:
            value = shift = 0
            while True:
                byte = blob[q]
                q += 1
                if byte & 0x80:
                    value |= (byte & 0x7f) << shift
                    shift += 7
                else:
                    value |= byte << shift
                    break
            handle += value
            value = shift = 0
            while True:
                byte = blob[q]
                q += 1
                if byte & 0x80:
                    value |= (byte & 0x7f) << shift
                    shift += 7
                else:
                    value |= (byte & 0x3f) << shift
                    value = -value if byte & 0x40 else value
                    break
            offset += value
            pairs.append((handle, offset))
        assert q == at + n
        at += n + 2
    return pairs


def sections_of(data):
    records = [json.loads(l) for l in vm.oracle(data)]
    return [r for r in records if 'section' in r and 'name' in r]


def expected_stream(data, cache):
    """What a correct walk prints, the records behind it and the section
    bytes, all from the independent map parse, decoder and framing."""
    expected, blobs, order = cache or vf.expected_file(data)
    secs = sections_of(data)
    by_name = {s['name']: blob for s, blob in zip(secs, blobs)}
    fmt = FAMILY[data[:6]]
    records = frame(by_name[OBJECTS], fmt)
    finished = next(l for l in expected if l.startswith('{"finished"'))
    lines = [object_line(r) for r in records]
    lines += [line({'objects': {'records': len(records), 'bytes': len(by_name[OBJECTS])}}), finished]
    return lines, records, by_name


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
    with tempfile.TemporaryDirectory(prefix='dwg-objects-') as tmp:
        tmp = Path(tmp)

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=600)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        local_files = {}
        for directory in local:
            for path in sorted(directory.rglob('*.dwg')):
                data = path.read_bytes()
                if data[:6] in FAMILY:
                    local_files.setdefault(hashlib.sha256(data).hexdigest(), (path.name, data))
        corpus = [(f'synthetic_{v}.dwg', Path(f'tests/fixtures/canonical/synthetic_{v}.dwg').read_bytes())
                  for v in (2004, 2010, 2013, 2018)]
        for manifest_name in ('lz77-corpus.json', 'objects-corpus.json'):
            manifest = json.loads(Path('tools/streaming', manifest_name).read_text())
            for item in manifest['files']:
                name = Path(item['name']).name
                if item['sha256'] in local_files:
                    corpus.append((name, local_files.pop(item['sha256'])[1]))
                    continue
                url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
                corpus.append((name, vm.fetch(url, item['bytes'], item['sha256'])))
        pinned = len(corpus)
        extra = [(f'local_{name}', data) for _, (name, data) in sorted(local_files.items(), key=lambda kv: kv[1][0])]
        corpus += extra

        totals = {'files': 0, 'records': 0, 'object_bytes': 0, 'payload_bytes': 0}
        expectations, caches = {}, {}
        for name, data in corpus:
            caches[name] = vf.expected_file(data)
            lines, records, by_name = expected_stream(data, caches[name])
            expectations[name] = (lines, records, by_name)
            pairs = handle_map(by_name['AcDb:Handles'])
            assert sorted(pairs, key=lambda p: p[1]) == [(r['handle'], r['start']) for r in records], \
                (name, 'the handle map and the stream disagree about the records')
            plain = run(data)
            assert plain[0] == 0 and plain[1].splitlines() == lines, (name, plain[2], 'stream differs from the independent framing')
            legacy = run(data, '--legacy')
            assert legacy[0] == 0 and not any('skipped' in l for l in legacy[1].splitlines()), (name, legacy[2])
            streamed = sorted(l for l in plain[1].splitlines() if l.startswith('{"object"'))
            assert sorted(l for l in legacy[1].splitlines() if l.startswith('{"object"')) == streamed, \
                (name, 'records differ from the legacy reader')
            big = len(data) > BIG
            for args in ((13,), (256,), ('--chunks', '1,7,256,3', '--pending')) if big else ((1,), (3,), (64,), ('--chunks', '1,7,256,3', '--pending')):
                assert run(data, *args) == plain, (name, args)
            totals['files'] += 1
            totals['records'] += len(records)
            totals['object_bytes'] += len(by_name[OBJECTS])
            totals['payload_bytes'] += sum(len(r['payload']) for r in records)
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'format': FAMILY[data[:6]],
                       'records': len(records), 'object_section_bytes': len(by_name[OBJECTS]),
                       'oracles': ['independent Python framing with CRC and MC checks', 'AcDb:Handles map',
                                   'legacy DwgFile::all_objects_lossy (handle, offset, MS, type, payload length, payload CRC-32)'],
                       'fixed_schedules': 4 if big else 5}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'objects-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=1800)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)

        # The same object stream in files this script builds: every page size
        # from a few bytes up (records and MC fields straddle pages), and the
        # pages of the other sections interleaved with the object section's.
        def rebuild(data, objects=None, mx=None, edit=None, **options):
            built = []
            _, blobs, _ = vs.assemble(data)
            for r, blob in zip(sections_of(data), blobs):
                is_objects = r['name'] == OBJECTS
                if is_objects and objects is not None:
                    blob = objects
                page = mx if is_objects and mx else r['max_page_size']
                pages = []
                for start in range(0, len(blob), page):
                    chunk = blob[start:start + page]
                    chunk = chunk.ljust(page, b'\0') if r['compressed'] == 1 else chunk.ljust(4, b'\0')
                    pages.append((start, chunk))
                built.append({'name': r['name'], 'id': r['section_id'], 'compressed': r['compressed'], 'mx': page,
                              'size': len(blob) if is_objects else r['size'], 'encrypted': r['encrypted'], 'pages': pages})
            if edit:
                edit(built)
            return data[:6] + vs.build(built, **options)[6:]

        by = dict(corpus)
        page_sizes = {}
        for source, sizes in (('line_2013.dwg', (4, 5, 7, 16, 63, 64, 1000)), ('arc_2013.dwg', (5, 61)),
                              ('line_2004.dwg', (4, 31, 64, 4096))):
            lines, records, by_name = expectations[source]
            for mx in sizes:
                data = rebuild(by[source], mx=mx)
                built = vf.expected_file(data)
                built_lines, built_records, _ = expected_stream(data, built)
                assert built_records == records
                result = run(data, 11)
                assert result[0] == 0 and result[1].splitlines() == built_lines, (source, mx, result[2])
                assert built_lines[:-1] == lines[:-1], (source, mx, 'records differ from the real file')
                page_sizes.setdefault(source, []).append(mx)
        rng = random.Random('interleave')
        for source in ('line_2004.dwg', 'line_2010.dwg'):
            lines, records, _ = expectations[source]
            sections = sections_of(by[source])
            data = rebuild(by[source])
            plan = vs.assemble(data)[2]
            objects_id = next(s['section_id'] for s in sections if s['name'] == OBJECTS)
            at_objects = [i for i, p in enumerate(plan) if p['id'] == objects_id]
            others = [i for i in range(len(plan)) if i not in at_objects]
            slots = sorted(rng.sample(range(len(plan)), len(at_objects)))
            rest = rng.sample(others, len(others))
            order_ = [None] * len(plan)
            for slot, page in zip(slots, at_objects):
                order_[slot] = page
            it = iter(rest)
            order_ = [p if p is not None else next(it) for p in order_]
            shuffled = rebuild(by[source], order=order_)
            result = run(shuffled)
            expected, _, _ = expected_stream(shuffled, vf.expected_file(shuffled))
            assert result[0] == 0 and result[1].splitlines() == expected and expected[:-1] == lines[:-1], (source, result[2])
            assert order_ != sorted(order_) and [p for p in order_ if p in at_objects] == at_objects
            page_sizes.setdefault(source + ' interleaved', []).append(len(plan))
        receipts.append({'built_files': page_sizes, 'note': 'real object sections repacked at the page sizes listed (and, last, with '
                                                                'the other sections\' pages interleaved with the object section\'s); '
                                                                'output equals the real file\'s'})

        # One checkpoint across the whole walk, resumed in a fresh process.
        checkpoint, first, second, full_trace = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl', tmp / 'full.jsonl'
        saved, coverage = {}, {'stages': set(), 'pending': 0, 'mc_open': 0, 'ms_split': 0, 'resumes': 0}
        resumable = [(n, d) for n, d in corpus if n in RESUMED]

        def straddle(records, wanted):
            """A page size and a record whose MS (or, for `mc`, whose MC) starts on the
            last byte of a page, so that field is split across two steps."""
            for mx in range(16, 64):
                for t, r in enumerate(records):
                    if t > 0 and wanted == 'ms' and r['start'] % mx == mx - 1:
                        return mx, t
                    if t > 0 and wanted == 'mc' and r['mc_len'] >= 2 and (r['start'] + r['ms_len']) % mx == mx - 1:
                        return mx, t
            raise AssertionError('no page size splits that field')
        windows = {}
        for wanted, source in (('ms', 'line_2004.dwg'), ('ms', 'line_2013.dwg'), ('mc', 'line_2013.dwg'), ('mc', 'arc_2013.dwg')):
            mx, t = straddle(expectations[source][1], wanted)
            label = f'{source[:-4]}_{wanted}_split_mx{mx}.dwg'
            windows[label] = t
            resumable.append((label, rebuild(by[source], mx=mx)))
        resumable_by = dict(resumable)
        for name, data in resumable:
            expected = expectations[name][0] if name in expectations else expected_stream(data, None)[0]
            identity = hashlib.sha256(data).hexdigest()
            chunk = 256 if len(data) > BIG or name in windows else 7
            full = run(data, chunk, '--trace', full_trace, '--report-steps', name=name)
            reads = full_trace.read_text().splitlines()
            report = [json.loads(l) for l in full[2].splitlines()]
            steps = next(r['steps'] for r in report if 'steps' in r)
            events = [r['step'] for r in report if 'event' in r]
            objects_at = [r['step'] for r in report if r.get('event') == 'object']
            if name == 'synthetic_2018.dwg':
                stops = list(range(steps + 1))
            else:
                around = {0, 1, 2, steps - 1, steps}
                for e in events[:12] + events[-6:] + random.Random(name + 'e').sample(events, min(len(events), 40)):
                    around.update((e - 2, e - 1, e, e + 1))
                # In a built file with a split field, the 80 steps after the record
                # before the split one (which starts or whose MC starts on a page's
                # last byte) committed.
                forced = set(range(objects_at[windows[name] - 1], min(objects_at[windows[name] - 1] + 80, objects_at[windows[name]] + 2))) if name in windows else set()
                around = sorted(s for s in around if 0 <= s <= steps)
                wanted = 24 if len(data) > BIG else 90
                stops = sorted(set(random.Random(name).sample(around, min(len(around), wanted))) | {0, steps} | forced
                               | set(random.Random(name + 'x').sample(range(steps), 6 if len(data) > BIG else 20)))
            for stop in stops:
                paused = run(data, chunk, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop,
                             '--trace', first, name=name)
                assert paused[0] == 0 and paused[1].splitlines()[-1] == line({'checkpoint': True, 'steps': stop}), (name, stop, paused)
                state = checkpoint.read_bytes()
                if state[FOUND]:
                    coverage['stages'].add(state[STAGE])
                    coverage['pending'] += struct.unpack_from('<H', state, PENDING_LEN)[0] > 0
                    coverage['mc_open'] += state[MC_LEN] > 0 and not state[MC_DONE] and state[STAGE] == 2
                    coverage['ms_split'] += state[STAGE] == 1 and state[MS_LEN] > 0
                    saved.setdefault((name, state[STAGE], struct.unpack_from('<H', state, PENDING_LEN)[0] > 0),
                                     (stop, state, Path(str(checkpoint) + '.caller').read_text()))
                resumed = run(data, chunk, '--identity', identity, '--resume', checkpoint, '--trace', second, name=name)
                assert resumed[0] == 0, (name, stop, resumed[2])
                assert paused[1].splitlines()[:-1] + resumed[1].splitlines() == full[1].splitlines(), \
                    (name, stop, 'output not exactly once')
                assert first.read_text().splitlines() + second.read_text().splitlines() == reads, (name, stop, 'source re-read')
                coverage['resumes'] += 1
            assert full[1].splitlines() == expected, (name, 'the traced run differs from the independent framing')
            receipts.append({'file': name, 'steps': steps, 'checkpoint_stops': len(stops), 'fresh_process_resume': True,
                             'source_rereads_beyond_design': 0, 'duplicate_output': 0})
        assert coverage['stages'] == {0, 1, 2, 3, 4, 5}, coverage
        assert coverage['pending'] and coverage['mc_open'] and coverage['ms_split'], coverage
        receipts.append({'checkpoint_framer_stages': sorted(coverage['stages']), 'checkpoints_holding_unframed_bytes': coverage['pending'],
                         'checkpoints_inside_an_MC': coverage['mc_open'], 'checkpoints_inside_an_MS': coverage['ms_split'],
                         'fresh_process_resumes': coverage['resumes']})

        # Corrupt, truncated and forged checkpoints are rejected before use.
        # The fixture is walked at every step, so every framer stage (with and
        # without unframed bytes) has a checkpoint; a real file's checkpoint is
        # flipped and truncated as well.
        forge = 'synthetic_2018.dwg'
        forge_data = by[forge]
        forge_id = hashlib.sha256(forge_data).hexdigest()

        def attempt(state, source, source_name, digest=None):
            checkpoint.write_bytes(state)
            identity = digest or hashlib.sha256(source).hexdigest()
            return run(source, 7, '--identity', identity, '--resume', checkpoint, name=source_name)

        def stored(file_name, stage, pending):
            _, state, caller = saved[(file_name, stage, pending)]
            Path(str(checkpoint) + '.caller').write_text(caller)
            return state
        rng = random.Random('flips')
        early = stored(forge, 0, True) if (forge, 0, True) in saved else stored(forge, 0, False)
        body = stored(forge, 2, True)
        real = next(state for (n, stage, _), (_, state, caller) in sorted(saved.items()) if n == 'line_2010.dwg' and stage == 2)
        real_name = 'line_2010.dwg'
        flipped = truncated = 0
        for label, state, source, source_name in (('the fixture before its first record', early, forge_data, forge),
                                                  ('the fixture inside a record', body, forge_data, forge),
                                                  ('a real drawing inside a record', real, by[real_name], real_name)):
            Path(str(checkpoint) + '.caller').write_text(next(c for (n, _, _), (_, s_, c) in saved.items() if s_ == state))
            for at in sorted(rng.sample(range(len(state)), min(len(state), 250))):
                bad = bytearray(state)
                bad[at] ^= 0x01
                result = attempt(bytes(bad), source, source_name)
                assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', label, at, result)
                flipped += 1
            for length in sorted(rng.sample(range(len(state)), min(len(state), 250))):
                assert attempt(state[:length], source, source_name)[0] != 0, ('truncated checkpoint accepted', label, length)
                truncated += 1
            result = attempt(state, source, source_name, digest='f' * 64)
            assert result[0] != 0 and 'SourceMismatch' in result[2], result

        def resealed(state, edits, insert=None):
            bad = bytearray(state[:-4])
            for at, value in edits:
                bad[at:at + len(value)] = value
            if insert:
                at, value = insert
                bad[at:at] = value
            return bytes(bad) + struct.pack('<I', zlib.crc32(bytes(bad)))
        p8 = lambda v: bytes([v])
        p16 = lambda v: struct.pack('<H', v)
        p32 = lambda v: struct.pack('<I', v)
        p64 = lambda v: struct.pack('<Q', v)
        u64 = lambda state, at: struct.unpack_from('<Q', state, at)[0]
        forgeries = []
        for (n, stage, pending), (_, state, _) in sorted(saved.items()):
            if n != forge:
                continue
            label = f'stage {stage}{" with unframed bytes" if pending else ""}'
            at = u64(state, AT)
            forgeries += [
                (f'{label}: unknown framer stage', state, [(STAGE, p8(9))]),
                (f'{label}: unknown record format', state, [(FORMAT, p8(7))]),
                (f'{label}: wrong record format for the release', state, [(FORMAT, p8(1 if state[FORMAT] == 2 else 2))]),
                (f'{label}: section not in the table', state, [(SECTION_ID, p32(0x7777))]),
                (f'{label}: size differing from the section map', state, [(TOTAL, p64(u64(state, TOTAL) + 1))]),
                (f'{label}: past the end of the section', state, [(AT, p64(u64(state, TOTAL) + 1))]),
                (f'{label}: unfound section with a live framer', state, [(FOUND, p8(0))]),
                (f'{label}: unframed bytes past the buffer', state, [(PENDING_LEN, p16(300))]),
                (f'{label}: record start past the stream', state, [(START, p64((1 << 64) - 1))]),
                (f'{label}: field lengths of 255', state, [(WORDS, p8(255)), (MS_LEN, p8(255)), (FIELD_LEN, p8(255))]),
                (f'{label}: one word with a field length of 255', state, [(WORDS, p8(1)), (FIELD_LEN, p8(255))]),
                (f'{label}: section of 2^64 - 1 bytes', state, [(TOTAL, p64((1 << 64) - 1))]),
            ]
            if stage in (1, 2, 3):
                forgeries += [
                    (f'{label}: record start off its bytes', state, [(START, p64(u64(state, START) + 1))]),
                ]
                if struct.unpack_from('<I', state, DONE)[0] <= 32:
                    # Past 32 payload bytes the CRC covers bytes no longer held, so a
                    # forged CRC there ends in `RecordCrc` at the record's end instead.
                    forgeries += [(f'{label}: CRC that the held bytes do not give', state,
                                   [(CRC, p16(struct.unpack_from('<H', state, CRC)[0] ^ 0x55))])]
                forgeries += [
                    (f'{label}: more records counted than the bytes before it can hold', state, [(RECORDS, p64(1 << 40))]),
                ]
            if stage in (2, 3):
                forgeries += [
                    (f'{label}: MS size off its bytes', state, [(SIZE, p32(struct.unpack_from('<I', state, SIZE)[0] + 1))]),
                    (f'{label}: payload consumed past the record', state, [(DONE, p32(1 << 20))]),
                    (f'{label}: MC length past ten', state, [(MC_LEN, p8(11))]),
                    (f'{label}: held payload bytes the record has not reached', state,
                     [(HEAD + 31, p8(1))] if struct.unpack_from('<I', state, DONE)[0] < 32 else [(MC, p64(1 << 62))]),
                ]
            if stage in (4, 5):
                forgeries += [(f'{label}: bytes held after the stream ended', state, [(PENDING_LEN, p16(1))], (PENDING, b'\0'))]
            if pending and stage < 4:
                forgeries += [(f'{label}: unframed bytes the walker has not handed over', state, [(AT, p64(at + 1))])]
        rejected = []
        for item in forgeries:
            label, state, edits, insert = item if len(item) == 4 else (*item, None)
            checkpoint.write_bytes(resealed(state, edits, insert))
            result = run(forge_data, 7, '--identity', forge_id, '--resume', checkpoint, name=forge)
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2] and 'panicked' not in result[2], (label, result)
            rejected.append(label)
        receipts.append({'files': [forge, real_name], 'bit_flips_rejected': flipped, 'truncations_rejected': truncated,
                         'identity_mismatch': 'rejected', 'resealed_forgeries_rejected': len(rejected)})

        # Every way the object stream can be wrong, each ending in its named
        # error with exactly the valid records before it committed.
        def mutate(blob, at, value):
            bad = bytearray(blob)
            bad[at:at + len(value)] = value
            return bytes(bad)

        def reseal(blob, r):
            bad = bytearray(blob)
            end = r['end'] - 2
            struct.pack_into('<H', bad, end, crc16(bytes(bad[r['start']:end])))
            return bytes(bad)

        def with_counter_nibble(blob, r, value):
            """The record's handle counter nibble set to `value`, CRC resealed."""
            bad = bytearray(blob)
            base = r['start'] + r['ms_len']
            for i in range(4):
                at = r['counter_at'] + i
                bit = (value >> (3 - i)) & 1
                bad[base + at // 8] = (bad[base + at // 8] & ~(0x80 >> (at % 8)) & 0xff) | (bit * (0x80 >> (at % 8)))
            return reseal(bytes(bad), r)

        def record_bytes(fmt, payload):
            if fmt == 'r2010':
                payload = b'\x00' + payload
                body = struct.pack('<H', len(payload) - 1) + payload
            else:
                body = struct.pack('<H', len(payload)) + payload
            return body + struct.pack('<H', crc16(body))

        cases_run = []
        for source in ('line_2004.dwg', 'line_2010.dwg', 'arc_2013.dwg'):
            lines, records, by_name = expectations[source]
            blob, fmt = by_name[OBJECTS], FAMILY[by[source][:6]]
            # A record in the middle, with a payload of a few hundred bytes: long
            # enough to hold the bytes edited below, short enough that an MC
            # claiming 16,383 bits is more than it holds.
            eligible = [i for i, x in enumerate(records) if 60 <= x['size'] <= 1500]
            k = eligible[len(eligible) // 2]
            r, last = records[k], records[-1]
            assert r['payload'] and 8 * len(r['payload']) < 16383
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
                ('record too short for its type and handle', PROLOGUE + record_bytes(fmt, b'\x00'), 'RecordHeader(4)', 0),
                ('section shorter than the prologue', PROLOGUE[:2], 'ObjectPrologue', 0),
            ]
            if fmt == 'r2010':
                cases += [
                    ('MC of eleven bytes', mutate(blob, payload_at, b'\xff' * 11), f'RecordHeader({r["start"]})', k),
                    ('MC claiming more bits than the record holds', mutate(blob, payload_at, b'\xff\x7f'),
                     f'RecordHeader({r["start"]})', k),
                    ('MC of 2^64 (nine continuation bytes, then 2)', mutate(blob, payload_at, b'\x80' * 9 + b'\x02'),
                     f'RecordHeader({r["start"]})', k),
                ]
            for label, bad, error, valid in cases:
                result = run(rebuild(by[source], objects=bad), name='corrupt.dwg')
                assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (source, label, error, result[2])
                assert result[1].splitlines() == lines[:valid], (source, label, 'output is not exactly the records before the failure')
                cases_run.append(f'{source}: {label}')

        # Shapes only a built file has.
        source = 'line_2010.dwg'
        lines, records, by_name = expectations[source]
        sections = sections_of(by[source])
        objects_section = next(s['section'] for s in sections if s['name'] == OBJECTS)
        plan = vs.assemble(rebuild(by[source], mx=0x400))[2]
        objects_pages = [i for i, p in enumerate(plan) if p['index'] == objects_section]
        swapped = list(range(len(plan)))
        swapped[objects_pages[0]], swapped[objects_pages[1]] = swapped[objects_pages[1]], swapped[objects_pages[0]]
        gap = objects_pages[3]

        def duplicate(built):
            twin = dict(next(s for s in built if s['name'] == OBJECTS))
            twin['id'] = 0x7001
            built.append(twin)

        def without(built):
            built[:] = [s for s in built if s['name'] != OBJECTS]
        shape_cases = [
            ('two pages of the object section swapped in the page map', rebuild(by[source], mx=0x400, order=swapped),
             f'ObjectsOrder {{ expected: 0, actual: {0x400} }}'),
            ('a page of the object section missing from the page map', rebuild(by[source], mx=0x400, drop={gap + 1}),
             f'ObjectsOrder {{ expected: {3 * 0x400}, actual: {4 * 0x400} }}'),
            ('no object section', rebuild(by[source], edit=without), 'ObjectsMissing'),
            ('two object sections', rebuild(by[source], edit=duplicate), 'SectionEntry('),
        ]
        for label, bad, error in shape_cases:
            result = run(bad, name='built.dwg')
            assert result[0] != 0 and error in result[2] and 'panicked' not in result[2], (label, error, result[2])
            printed = result[1].splitlines()
            assert printed == lines[:len(printed)] and not any('"objects"' in l or '"finished"' in l for l in printed), \
                (label, 'output is not a valid prefix')
            cases_run.append(f'built: {label}')
        receipts.append({'corruptions_rejected': cases_run})

    report = {'scope': 'ODA 5.4.1 section 20.1: the object records of an R2004-family file framed in section order out of the whole-file '
                       'walk (R2004, R2010, R2013 and R2018 drawings): MS, MC, payload, CRC, type and handle, each record\'s bytes equal to '
                       'the legacy ObjectWalker\'s. No object decode, R2007, out-of-order object pages or performance claim.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'pinned_files': pinned,
              'local_files': len(extra), 'totals': totals, 'cases': receipts, 'status': 'passed'}
    Path('objects-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
