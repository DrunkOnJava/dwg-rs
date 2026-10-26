#!/usr/bin/env python3
"""Count, over R2007 (AC1021) drawings, how many of each integrity field of
ODA 5.4.1 sections 5.2-5.4 agree with the reading the streaming walker
enforces, and with the nearest wrong readings (the other CRC flavour, the
other seed update, plain byte order). It uses only the independent reader
tools/streaming/r2007_ref.py.
Usage: probe_r2007_checks.py <drawing.dwg>..."""
import struct
import sys
from pathlib import Path
from collections import Counter

sys.path.insert(0, str(Path(__file__).resolve().parent))
import r2007_ref as r  # noqa: E402


def plain_order_crc(data, seed, mirrored):
    crc = seed
    for b in data:
        crc = r.MIRROR[(crc ^ b) & 0xFF] ^ (crc >> 8) if mirrored else r.NORMAL[(b ^ (crc >> 56)) & 0xFF] ^ ((crc << 8) & r.M64)
    return crc if mirrored else ~crc & r.M64


def readings(data, expected, length, seed):
    """Which (flavour, seed update, byte order) readings give `expected`."""
    out = []
    for flavour, mirrored in (('normal', False), ('mirrored', True)):
        for update, fn in (('seed 0', lambda s, n: s), ('UpdateSeed1', r.seed1), ('UpdateSeed2', r.seed2)):
            if r.crc64(data, fn(seed, length), mirrored) == expected:
                out.append((flavour, update, 'blocks of 8'))
            if plain_order_crc(data, fn(seed, length), mirrored) == expected:
                out.append((flavour, update, 'plain'))
    return out


def main():
    tally, files = Counter(), 0
    page_facts = Counter()
    for path in sys.argv[1:]:
        data = Path(path).read_bytes()
        parsed = r.parse(data)
        f, files = parsed['fields'], files + 1
        block = data[0x80:0x480]
        cws, msg = r.deinterleave(block[:765], 3, 16)
        normal, mirrored, r1, r2, _ = struct.unpack_from('<5Q', block, 0x3D8)
        crc0, key, ccrc = struct.unpack_from('<3Q', msg, 0)
        clen = struct.unpack_from('<i', msg, 0x18)[0]
        tally['header: codewords with zero syndromes (of 3)'] += sum(r.intact(c, 16) for c in cws)
        tally['header: checking sequence CRC, normal, UpdateSeed1(0, 16)'] += r.crc64(struct.pack('<QQ', key, r.rol(key, key)), r.seed1(0, 16), False) == crc0
        if clen < 0:
            # Stored, not compressed (negative ComprLen): the walker reads the 0x110 bytes as they are and checks
            # no compressed-data CRC.
            tally['header: stored, not compressed (negative ComprLen)'] += 1
            plain = msg[0x20:0x20 + 0x110]
        else:
            tally['header: compressed'] += 1
            tally['header: compressed data CRC, normal, UpdateSeed2(0, length)'] += r.crc64(msg[0x20:0x20 + clen], r.seed2(0, clen), False) == ccrc
            tally['header: compressed data CRC under any other reading'] += sum(1 for x in readings(msg[0x20:0x20 + clen], ccrc, clen, 0)
                                                                              if x != ('normal', 'UpdateSeed2', 'blocks of 8'))
            plain = r.lz(msg[0x20:0x20 + clen], 0x110)
        tally['header: header CRC, normal, UpdateSeed2(0, 0x110), CRC field zeroed'] += r.crc64(plain[:0x108] + bytes(8), r.seed2(0, 0x110), False) == f[33]
        tally['header: header CRC under any other reading'] += sum(1 for x in readings(plain[:0x108] + bytes(8), f[33], 0x110, 0)
                                                                 if x != ('normal', 'UpdateSeed2', 'blocks of 8'))
        tally['header: check data CRCs, both (0x3D8)'] += r.check_data(r1, r2) == (normal, mirrored)
        tally['header: fixed fields (0x70, 0x20, 0x40, 0xF800, 4, 1, 0x60100)'] += (f[0], f[14], f[15], f[17], f[18], f[19], f[29]) == (0x70, 0x20, 0x40, 0xF800, 4, 1, 0x60100)
        tally['header: file size equals the source'] += f[1] == len(data)
        tally['header: second copy is the last 0x400 bytes'] += r.PAGE_BASE + f[9] + 0x400 == len(data)
        tally['header: CrcSeed is zero'] += f[30] == 0
        tally['header: encoded seeds decode to CrcSeed'] += r.decode10(f[4]) == f[30] == r.decode10(f[28])
        for label, base, comp, unc, factor, crc_c, crc_u, seed in (
                ('page map', r.PAGE_BASE + f[7], f[10], f[11], f[3], f[2], f[16], r.decode10(f[4])),
                ('section map', r.PAGE_BASE + next(e for e in parsed['entries'] if abs(e[0]) == f[24])[2], f[22], f[25], f[27], f[26], f[21], r.decode10(f[28]))):
            stored = min(comp, unc)
            blocks = r.system_blocks(stored, factor)
            cws, m = r.deinterleave(data[base:base + blocks * 255], blocks, 16)
            payload = m[:stored]
            plain = r.lz(payload, unc) if comp < unc else payload[:unc]
            tally[f'{label}: codewords with zero syndromes'] += sum(r.intact(c, 16) for c in cws)
            tally[f'{label}: codewords'] += blocks
            tally[f'{label}: stored CRC, mirrored, UpdateSeed1(seed, length)'] += r.crc64(payload, r.seed1(seed, stored), True) == crc_c
            tally[f'{label}: decompressed CRC, mirrored, UpdateSeed1(seed, length)'] += r.crc64(plain, r.seed1(seed, unc), True) == crc_u
            tally[f'{label}: stored CRC under any other reading'] += sum(1 for x in readings(payload, crc_c, stored, seed) if x != ('mirrored', 'UpdateSeed1', 'blocks of 8'))
            tally[f'{label}: decompressed CRC under any other reading'] += sum(1 for x in readings(plain, crc_u, unc, seed) if x != ('mirrored', 'UpdateSeed1', 'blocks of 8'))
            tally[f'{label}: repeats equal the first copy'] += all(m[k * r.align(stored, 8):k * r.align(stored, 8) + stored] == payload for k in range(1, factor)
                                                                  if (k + 1) * r.align(stored, 8) <= len(m))
        tally['page map: entries equal PagesAmount, largest id equals PagesMaxId'] += len(parsed['entries']) == f[12] and max(abs(e[0]) for e in parsed['entries']) == f[13]
        tally['page map: sizes multiples of 0x20'] += all(abs(e[1]) % 0x20 == 0 for e in parsed['entries'])
        tally['section map: descriptors equal SectionsAmount'] += len(parsed['sections']) + 1 == f[20]
        for s in parsed['sections']:
            tally['sections'] += 1
            tally['sections: pages end exactly at the section size'] += (max((p['data_offset'] + p['uncompressed'] for p in s['pages']), default=0) == s['size'])
            tally['sections: encryption 0 or 2, encoding 1 or 4'] += s['encryption'] in (0, 2) and s['encoding'] in (1, 4)
            for p in s['pages']:
                ent = next(e for e in parsed['entries'] if abs(e[0]) == abs(p['id']))
                tally['data pages'] += 1
                stored = p['compressed']
                if p['codewords']:
                    span = data[p['address']:p['address'] + p['codewords'] * 255]
                    cws, m = r.deinterleave(span, p['codewords'], 4)
                    payload = m[:stored]
                    tally['data pages: Reed-Solomon (255, 251) codewords'] += p['codewords']
                    tally['data pages: codewords with zero syndromes'] += sum(r.intact(c, 4) for c in cws)
                    tally['data pages: entry size is the span rounded up to 0x20'] += abs(ent[1]) == r.align(p['codewords'] * 255, 0x20)
                    tally['data pages: padding after the stored bytes is zero'] += m[stored:].count(0) == len(m) - stored
                else:
                    payload = data[p['address']:p['address'] + stored]
                plain = r.lz(payload, p['uncompressed']) if stored < p['uncompressed'] else payload[:p['uncompressed']]
                tally['data pages: stored CRC, mirrored, UpdateSeed1(CrcSeed, compressed)'] += r.crc64(payload, r.seed1(f[30], stored), True) == p['crc']
                tally['data pages: stored CRC under any other reading'] += sum(1 for x in readings(payload, p['crc'], stored, f[30]) if x != ('mirrored', 'UpdateSeed1', 'blocks of 8'))
                tally['data pages: checksum over the decompressed bytes, seed 0'] += r.checksum32(plain) == p['checksum']
                tally['data pages: compressed no bigger than uncompressed'] += stored <= p['uncompressed']
                tally['data pages: entry holds the span'] += (p['codewords'] * 255 or stored) <= abs(ent[1])
                page_facts['largest uncompressed'] = max(page_facts['largest uncompressed'], p['uncompressed'])
                page_facts['largest compressed'] = max(page_facts['largest compressed'], stored)
                page_facts['most codewords'] = max(page_facts['most codewords'], p['codewords'])
                page_facts['pages over 0xF800 uncompressed'] += p['uncompressed'] > 0xF800
                page_facts['pages whose size field is the uncompressed size'] += p['size'] == p['uncompressed']
    print(f'{files} drawings')
    for key in sorted(tally):
        print(f'{tally[key]:6d}  {key}')
    for key in sorted(page_facts):
        print(f'{page_facts[key]:6d}  {key}')


if __name__ == '__main__':
    main()
