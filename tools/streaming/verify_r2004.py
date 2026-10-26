#!/usr/bin/env python3
"""Black-box acceptance for the bounded §4.1 R2004-family header, never cargo test.
Usage: verify_r2004.py <stream_r2004 binary> [<streaming_schedules binary>]"""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import urllib.request
import zlib

SCHEDULED = ('synthetic_2018.dwg', 'line_2004.dwg', 'line_2013.dwg')
RESUMED = ('synthetic_2004.dwg', 'line_2004.dwg', 'line_2010.dwg', 'line_2013.dwg')


def lcg(n):
    out, seed = bytearray(), 1
    for _ in range(n):
        seed = (seed * 0x343fd + 0x269ec3) & 0xffffffff
        out.append((seed >> 16) & 0xff)
    return bytes(out)


MAGIC = lcg(0x100)


def block(data):
    return bytearray(a ^ b for a, b in zip(data[0x80:0xec], MAGIC))


def oracle(data):
    """Independent §4.1 parse: the exact stdout a correct parser prints."""
    k = block(data)
    u32 = lambda b, at: struct.unpack_from('<I', b, at)[0]
    u64 = lambda b, at: struct.unpack_from('<Q', b, at)[0]
    crc = bytearray(k)
    crc[0x68:0x6c] = bytes(4)
    assert zlib.crc32(crc) == u32(k, 0x68) and k[:12] == b'AcFssFcAJMB\0' and u32(data, 0x28) == 0x80
    assert [u32(k, at) for at in (0x0c, 0x10, 0x14, 0x44, 0x48, 0x4c)] == [0, 0x6c, 4, 0x20, 0x80, 0x40]
    assert data[0xec:0x100] == MAGIC[0xec:]
    fields = {'magic': data[:6].decode(), 'maintenance': data[0x0b], 'preview_address': u32(data, 0x0d),
              'app_version': data[0x11], 'app_maintenance': data[0x12], 'codepage': struct.unpack_from('<H', data, 0x13)[0],
              'security_flags': u32(data, 0x18), 'summary_info_address': u32(data, 0x20), 'vba_project_address': u32(data, 0x24),
              'last_page_id': u32(k, 0x28), 'last_page_end': u64(k, 0x2c), 'second_header_address': u64(k, 0x34),
              'gap_amount': u32(k, 0x3c), 'page_amount': u32(k, 0x40), 'page_map_id': u32(k, 0x50),
              'page_map_address': u64(k, 0x54) + 0x100, 'section_map_id': u32(k, 0x5c), 'page_array_size': u32(k, 0x60),
              'gap_array_size': u32(k, 0x64)}
    return json.dumps(fields, separators=(',', ':')) + '\n'


def reseal(data, edit):
    """Edit the decrypted block, recompute its CRC and encrypt it again."""
    bad = bytearray(data)
    k = block(bad)
    edit(k)
    k[0x68:0x6c] = bytes(4)
    struct.pack_into('<I', k, 0x68, zlib.crc32(k))
    bad[0x80:0xec] = bytes(a ^ b for a, b in zip(k, MAGIC))
    return bytes(bad)


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


def main():
    binary = str(Path(sys.argv[1]).resolve())
    schedules = sys.argv[2] if len(sys.argv) > 2 else None
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-r2004-') as tmp:
        tmp = Path(tmp)

        def run(data, *args, name='input.dwg'):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), *map(str, args)], capture_output=True, timeout=60)
            return result.returncode, result.stdout.decode(), result.stderr.decode()

        corpus = [(f'synthetic_{v}.dwg', Path(f'tests/fixtures/canonical/synthetic_{v}.dwg').read_bytes())
                  for v in (2004, 2010, 2013, 2018)]
        manifest = json.loads(Path('tools/streaming/lz77-corpus.json').read_text())
        for item in manifest['files']:
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((item['name'], fetch(url, item['bytes'], item['sha256'])))

        for name, data in corpus:
            base = run(data)
            assert base == (0, oracle(data), ''), (name, base)
            mine = json.loads(base[1])
            legacy = run(data, '--legacy')
            assert legacy[0] == 0, (name, legacy[2])
            theirs = json.loads(legacy[1])
            assert all(mine[key] == value for key, value in theirs.items()), (name, 'legacy differs')
            for split in range(1, 33):
                assert run(data, split) == base, (name, split)
            assert run(data, '--chunks', '1,7,256,3', '--pending') == base, name
            for length in range(0x100):
                result = run(data[:length])
                assert result[0] != 0 and 'Truncated' in result[2], (name, length, result[2])
            receipt = {'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'header': mine,
                       'oracles': ['independent Python §4.1 parse', 'legacy header::R2004Header'],
                       'schedules': 33, 'truncations': 0x100}
            if schedules and name in SCHEDULED:
                path = tmp / name
                path.write_bytes(data)
                log = Path(f'r2004-schedules-{name}.jsonl')
                result = subprocess.run([schedules, binary, str(path), str(log)], capture_output=True, timeout=600)
                assert result.returncode == 0, result.stderr.decode()
                receipt['proptest_schedules'] = 128
            receipts.append(receipt)

        # Every documented check, on each release band.
        def field(at, value):
            return lambda k: struct.pack_into('<I', k, at, value)
        for name, data in corpus:
            if name not in RESUMED:
                continue
            length = len(data)
            plain = bytearray(data)
            struct.pack_into('<I', plain, 0x28, 0x81)
            flipped, trailer = bytearray(data), bytearray(data)
            flipped[0x80 + 0x40] ^= 0x01
            trailer[0xf0] ^= 0x01
            cases = [
                ('unknown magic', b'AC1099' + data[6:], 'InvalidMagic'),
                ('R2007 magic', b'AC1021' + data[6:], 'UnsupportedLayout'),
                ('R2000 magic', b'AC1015' + data[6:], 'UnsupportedLayout'),
                ('0x28 not 0x80', bytes(plain), 'R2004Field(40)'),
                ('file ID', reseal(data, lambda k: k.__setitem__(3, k[3] ^ 1)), 'R2004Field(128)'),
                ('block CRC', bytes(flipped), 'R2004HeaderCrc'),
                ('0x10 not 0x6C', reseal(data, field(0x10, 0x6d)), 'R2004Field(144)'),
                ('0x4C not 0x40', reseal(data, field(0x4c, 0x41)), 'R2004Field(204)'),
                ('trailer', bytes(trailer), 'R2004Field(236)'),
                ('page map past the end', reseal(data, lambda k: struct.pack_into('<Q', k, 0x54, length)), 'R2004Field(212)'),
                ('page map off a 0x20 boundary', reseal(data, lambda k: struct.pack_into('<Q', k, 0x54, struct.unpack_from('<Q', k, 0x54)[0] + 1)), 'R2004Field(212)'),
                ('second header past the end', reseal(data, lambda k: struct.pack_into('<Q', k, 0x34, length)), 'R2004Field(180)'),
            ]
            labels = []
            for label, bad, error in cases:
                result = run(bad)
                assert result[0] != 0 and error in result[2] and result[1] == '', (name, label, error, result)
                labels.append(label)
            receipts.append({'file': name, 'corruptions_rejected': labels})

        # Fresh-process resume at every byte of the header.
        checkpoint, first, second = tmp / 'state.bin', tmp / 'first.jsonl', tmp / 'second.jsonl'
        for name, data in corpus:
            if name not in RESUMED:
                continue
            identity = hashlib.sha256(data).hexdigest()
            full = run(data, 7, name=name)

            def covered(path):
                return [at for r in map(json.loads, path.read_text().splitlines())
                        for at in range(r['offset'], r['offset'] + r['bytes'])]
            for stop in range(0x101):
                paused = run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop, '--trace', first, name=name)
                assert paused[0] == 0, (name, stop, paused[2])
                assert len(checkpoint.read_bytes()) <= 316
                resumed = run(data, 7, '--identity', identity, '--resume', checkpoint, '--trace', second, name=name)
                assert resumed[0] == 0 and resumed[1] == full[1], (name, stop, resumed)
                # Each byte is read once: the first process reads [0, stop), the
                # resumed one [stop, 0x100), each in order.
                assert covered(first) == list(range(stop)), (name, stop, 'first process reads')
                assert covered(second) == list(range(stop, 0x100)), (name, stop, 'source re-read')
            receipts.append({'file': name, 'checkpoint_stops': 0x101, 'fresh_process_resume': True, 'source_rereads': 0})

        name, data = next(c for c in corpus if c[0] == 'line_2004.dwg')
        identity = hashlib.sha256(data).hexdigest()
        assert run(data, 7, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', 100, name=name)[0] == 0
        state = checkpoint.read_bytes()

        def restore(blob, digest=identity, source=data):
            checkpoint.write_bytes(blob)
            return run(source, 7, '--identity', digest, '--resume', checkpoint, name=name)
        for at in range(len(state)):
            bad = bytearray(state)
            bad[at] ^= 0x01
            result = restore(bytes(bad))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], ('bit flip accepted', at, result)
        for length in range(len(state)):
            assert restore(state[:length])[0] != 0, ('truncated checkpoint accepted', length)
        result = restore(state, digest='f' * 64)
        assert result[0] != 0 and 'SourceMismatch' in result[2], result
        broken = bytearray(data)
        broken[0x80 + 0x40] ^= 0x01
        failed = run(bytes(broken), 7, '--identity', identity, '--checkpoint-out', checkpoint, name=name)
        assert failed[0] != 0 and 'R2004HeaderCrc' in failed[2], failed
        again = run(bytes(broken), 7, '--identity', identity, '--resume', checkpoint, name=name)
        assert again[0] != 0 and 'R2004HeaderCrc' in again[2], ('failure not restored', again)
        receipts.append({'file': name, 'bit_flips_rejected': len(state), 'truncations_rejected': len(state),
                         'identity_mismatch': 'rejected', 'failure_snapshot': 'restored'})

    report = {'scope': 'ODA 5.4.1 section 4.1 R2004-family file header only: no page map, section map or object walk.',
              'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'files': len(corpus),
              'cases': receipts, 'status': 'passed'}
    Path('r2004-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
