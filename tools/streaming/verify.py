#!/usr/bin/env python3
"""Black-box acceptance against a built header reader, never cargo test."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

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
        def run(data, split=256):
            path.write_bytes(data)
            result = subprocess.run([binary, str(path), str(split)], capture_output=True, timeout=10)
            return result.returncode, result.stdout.decode(), result.stderr.decode()
        for magic in (b'AC1012', b'AC1014', b'AC1015'):
            for count in range(3, 7):
                data = fixture(magic, count)
                base = run(data)
                assert base[0] == 0, base
                assert 'FlatHeader' in base[1], base
                for split in range(1, 33):
                    assert run(data, split) == base, (magic, count, split)
                for length in range(25 + count * 9 + 18):
                    assert run(data[:length])[0] != 0, (magic, count, length)
                receipts.append({'magic': magic.decode(), 'locators': count, 'sha256': hashlib.sha256(data).hexdigest(), 'schedules': 33, 'truncations': 25 + count * 9 + 18})
        for at in (0, 21, 25, 34, 25 + 27, 25 + 27 + 2):
            data = fixture()
            data[at] ^= 0xff
            assert run(data)[0] != 0, ('corrupt byte accepted', at)
        for magic in (b'AC1018', b'AC1021', b'AC1024', b'AC1027', b'AC1032'):
            result = run(fixture(magic))
            assert result[0] != 0 and 'UnsupportedLayout' in result[2], result
    report = {'scope': 'ODA 5.4.1 section 3.2 file header only; no object parse or performance claim', 'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'cases': receipts, 'status': 'passed'}
    Path('streaming-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))

if __name__ == '__main__':
    main()
