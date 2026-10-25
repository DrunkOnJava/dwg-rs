#!/usr/bin/env python3
"""Black-box acceptance against a built header reader, never cargo test."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import urllib.request
import zlib

SENTINEL = bytes.fromhex('95a04e2899821ae55e41e05f9d3a4d00')

def crc(data):
    value = 0
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ (0xa001 if value & 1 else 0)
    return value

def fixture(magic=b'AC1014', count=3):
    data = bytearray(25)
    data[:6] = magic
    data[12] = 1
    struct.pack_into('<H', data, 19, 30)
    struct.pack_into('<I', data, 21, count)
    end = 25 + count * 9 + 18
    for number in range(count):
        data.extend(struct.pack('<BII', number, end + number * 3, 3))
    data.extend(struct.pack('<H', crc(data) ^ {3: 0xa598, 4: 0x8101, 5: 0x3cc4, 6: 0x8461}[count]))
    data.extend(SENTINEL)
    data.extend(bytes(count * 3))
    return data

def main():
    binary = str(Path(sys.argv[1]).resolve())
    receipts = []
    with tempfile.TemporaryDirectory(prefix='dwg-streaming-') as tmp:
        path = Path(tmp) / 'input.dwg'
        def run(data, split=256, extra=()):
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), str(split), *extra], capture_output=True, timeout=10)
            return result.returncode, result.stdout.decode(), result.stderr.decode()
        for magic in (b'AC1012', b'AC1014', b'AC1015'):
            for count in range(3, 7):
                data = fixture(magic, count)
                base = run(data)
                assert base[0] == 0, base
                header = json.loads(base[1])
                assert header['magic'] == magic.decode() and header['locator_count'] == count, header
                for split in range(1, 33):
                    assert run(data, split) == base, (magic, count, split)
                for length in range(25 + count * 9 + 18):
                    assert run(data[:length])[0] != 0, (magic, count, length)
                receipts.append({'magic': magic.decode(), 'locators': count, 'sha256': hashlib.sha256(data).hexdigest(), 'schedules': 33, 'truncations': 25 + count * 9 + 18})
        manifest = json.loads(Path('tools/streaming/header-corpus.json').read_text())
        for item in manifest['files']:
            url = f"https://raw.githubusercontent.com/nextgis/dwg_samples/{manifest['commit']}/{item['name']}"
            with urllib.request.urlopen(url, timeout=30) as src, path.open('wb') as dst:
                total = 0
                digest = hashlib.sha256()
                while block := src.read(65536):
                    total += len(block)
                    assert total <= item['bytes'], 'source exceeds manifest'
                    digest.update(block)
                    dst.write(block)
            assert total == item['bytes'] and digest.hexdigest() == item['sha256'], item
            data = path.read_bytes()
            base = run(data)
            assert base[0] == 0, base
            expected = dict(item['header'], locators=item['locators'])
            assert json.loads(base[1]) == expected, (base, expected)
            for split in (1, 2, 3, 7, 9, 15, 16, 17, 31, 64, 256):
                assert run(data, split) == base, (item['name'], split)
            if len(sys.argv) > 2:
                property_receipt = Path('streaming-schedules-' + item['name'] + '.jsonl')
                result = subprocess.run([sys.argv[2], binary, str(path), str(property_receipt)], capture_output=True, timeout=120)
                assert result.returncode == 0, result.stderr.decode()
            receipts.append({'real_file': item['name'], 'sha256': digest.hexdigest(), 'header': expected, 'schedules': 12})
        checkpoint = str(Path(tmp) / 'state.bin')
        trace = str(Path(tmp) / 'reads.jsonl')
        data = fixture(b'AC1015', 6)
        identity = hashlib.sha256(data).hexdigest()
        base = run(data)
        header_end = 25 + 6 * 9 + 18
        for stop in range(header_end + 1):
            paused = run(data, 7, ('--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', str(stop), '--pending'))
            assert paused[0] == 0 and json.loads(paused[1]) == {'checkpoint': True, 'offset': stop}, paused
            saved = Path(checkpoint).read_bytes()
            assert len(saved) <= 256
            resumed = run(data, 1, ('--identity', identity, '--resume', checkpoint, '--trace', trace, '--pending'))
            assert resumed == base, (stop, resumed, base)
            reads = [json.loads(line) for line in Path(trace).read_text().splitlines()]
            position = stop
            for read in reads:
                assert read['offset'] == position and read['bytes'] > 0, (stop, reads)
                position += read['bytes']
            assert position == header_end, (stop, reads)
        receipts.append({'checkpoint_positions': header_end + 1, 'fresh_process_resume': True, 'source_rereads': 0, 'pending_delivery': 'passed'})
        run(data, 7, ('--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', '33'))
        valid = Path(checkpoint).read_bytes()
        for at in range(len(valid)):
            bad = bytearray(valid)
            bad[at] ^= 1
            Path(checkpoint).write_bytes(bad)
            result = run(data, 7, ('--identity', identity, '--resume', checkpoint))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], (at, result)
        for at in (5, 6, 7, 8, 9, 12):
            bad = bytearray(valid)
            bad[at] ^= 1
            bad[-4:] = struct.pack('<I', zlib.crc32(bad[:-4]))
            Path(checkpoint).write_bytes(bad)
            result = run(data, 7, ('--identity', identity, '--resume', checkpoint))
            assert result[0] != 0 and 'InvalidCheckpoint' in result[2], (at, result)
        for length in range(len(valid)):
            Path(checkpoint).write_bytes(valid[:length])
            assert run(data, 7, ('--identity', identity, '--resume', checkpoint))[0] != 0, length
        Path(checkpoint).write_bytes(bytes(257))
        assert run(data, 7, ('--identity', identity, '--resume', checkpoint))[0] != 0
        Path(checkpoint).write_bytes(valid)
        wrong_id = run(data, 7, ('--identity', 'f' * 64, '--resume', checkpoint))
        assert wrong_id[0] != 0 and 'SourceMismatch' in wrong_id[2], wrong_id
        wrong_length = run(data + b'X', 7, ('--identity', identity, '--resume', checkpoint))
        assert wrong_length[0] != 0 and 'SourceMismatch' in wrong_length[2], wrong_length
        for at in (0, 21, 25, 34, 25 + 27, 25 + 27 + 2):
            data = fixture()
            data[at] ^= 0xff
            failed = run(data)
            assert failed[0] != 0, ('corrupt byte accepted', at)
            identity = hashlib.sha256(data).hexdigest()
            snap = run(data, 7, ('--identity', identity, '--checkpoint-out', checkpoint))
            assert snap == failed, (at, snap, failed)
            assert run(data, 1, ('--identity', identity, '--resume', checkpoint)) == failed, ('failed-state restore', at)
        for magic in (b'AC1018', b'AC1021', b'AC1024', b'AC1027', b'AC1032'):
            result = run(fixture(magic))
            assert result[0] != 0 and 'UnsupportedLayout' in result[2], result
    report = {'scope': 'ODA 5.4.1 section 3.2 file header only; no object parse or performance claim', 'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'cases': receipts, 'status': 'passed'}
    Path('streaming-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))

if __name__ == '__main__':
    main()
