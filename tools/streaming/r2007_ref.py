"""An independent R2007 (AC1021) reader and writer for the acceptance run
(ODA v5.4.1 section 5), written from the spec's text and sharing no code with
the crate: GF(2^8) Reed-Solomon (syndromes and an encoder), the two 64-bit
CRCs, the 32-bit page checksum, the section 5.10 decompressor and a matching
compressor, the file header, page map, section map and data pages. `parse`
checks everything the grammar checks and returns every line and every section
byte a correct walk produces; `build` writes whole files in shapes the corpus
lacks, with knobs that break one rule at a time."""
import json
import random
import struct
import zlib

M64 = (1 << 64) - 1
PAGE_BASE = 0x480


class Bad(Exception):
    """The file breaks a rule the walker enforces."""


def crc_table(poly, mirrored):
    table = []
    for i in range(256):
        c = i if mirrored else i << 56
        for _ in range(8):
            if mirrored:
                c = (c >> 1) ^ poly if c & 1 else c >> 1
            else:
                c = ((c << 1) ^ poly) & M64 if c >> 63 else (c << 1) & M64
        table.append(c)
    return table


NORMAL = crc_table(0x42F0E1EBA9EA3693, False)
MIRROR = crc_table(0x95AC9329AC4BC9B5, True)
BLOCK = (6, 7, 4, 5, 2, 3, 0, 1)
TAIL = {1: (0,), 2: (0, 1), 3: (0, 1, 2), 4: (2, 3, 0, 1), 5: (2, 3, 0, 1, 4), 6: (2, 3, 0, 1, 4, 5), 7: (2, 3, 0, 1, 4, 5, 6)}


def crc64(data, seed, mirrored):
    crc = seed
    def step(b):
        nonlocal crc
        if mirrored:
            crc = MIRROR[(crc ^ b) & 0xFF] ^ (crc >> 8)
        else:
            crc = NORMAL[(b ^ (crc >> 56)) & 0xFF] ^ ((crc << 8) & M64)
    whole = len(data) // 8 * 8
    for p in range(0, whole, 8):
        for o in BLOCK:
            step(data[p + o])
    rest = len(data) - whole
    for o in TAIL.get(rest, ()):
        step(data[whole + o])
    return crc if mirrored else ~crc & M64


def seed1(seed, length):
    s = ((seed + length) * 0x343FD + 0x269EC3) & M64
    s |= (s * (0x343FD << 32) + (0x269EC3 << 32)) & M64
    return ~s & M64


def seed2(seed, length):
    s = ((seed + length) * 0x343FD + 0x269EC3) & M64
    s = (s * ((1 << 32) + 0x343FD) + (length + 0x269EC3)) & M64
    return ~s & M64


def checksum32(data):
    n = len(data)
    seed = (n * 0x343FD + 0x269EC3) & M64
    s = [seed & 0xFFFF, (seed >> 16) & 0xFFFF]
    pos, left = 0, n
    def add(b):
        s[0] = (s[0] + b) & 0xFFFFFFFF
        s[1] = (s[1] + s[0]) & 0xFFFFFFFF
    while left:
        big = min(0x15B0, left)
        left -= big
        for _ in range(big >> 3):
            for o in BLOCK:
                add(data[pos + o])
            pos += 8
        rest = big & 7
        for o in TAIL.get(rest, ()):
            add(data[pos + o])
        pos += rest
        s[0] %= 0xFFF1
        s[1] %= 0xFFF1
    return (s[1] << 16) | (s[0] & 0xFFFF)


def rol(v, c):
    c &= 0x1F
    return v if c == 0 else ((v << c) | (v >> (64 - c))) & M64


def decode10(v):
    hi, lo = v >> 32, v & 0xFFFFFFFF
    r = 0
    for m, b in ((0x8000000, 1), (0x200000, 2), (0x8000, 4), (0x200, 8), (0x8, 0x10)):
        if hi & m:
            r |= b
    for m, b in ((0x20000000, 0x20), (0x800000, 0x40), (0x20000, 0x80), (0x800, 0x100), (0x20, 0x200)):
        if lo & m:
            r |= b
    return r


def encode10(value, rnd):
    """A 64-bit value whose ten data bits are `value`, the rest random."""
    r = rnd.getrandbits(64)
    lo, hi = r & 0xDF7DF7DF, (r >> 32) & 0xF7DF7DF7
    for m, b in ((0x20, 0x200), (0x800, 0x100), (0x20000, 0x80), (0x800000, 0x40), (0x20000000, 0x20)):
        if value & b:
            lo |= m
    for m, b in ((0x8, 0x10), (0x200, 8), (0x8000, 4), (0x200000, 2), (0x8000000, 1)):
        if value & b:
            hi |= m
    return lo | (hi << 32)


def check_data(r1, r2, rnd=None):
    """The five values at 0x3D8: normal CRC, mirrored CRC, the two randoms, an encoded seed."""
    w = [0] * 8
    w[0] = rol(r1, r2); w[1] = rol(w[0], w[0]); w[2] = rol(r2, w[1]); w[3] = rol(w[2], w[2])
    w[4] = rol(r1, w[3]); w[5] = rol(w[4], w[4]); w[6] = rol(w[5], w[5]); w[7] = rol(w[6], w[6])
    normal = crc64(struct.pack('<8Q', *w), ~r2 & M64, False)
    w[0] = rol(r1, r2); w[1] = rol(normal, w[0]); w[2] = rol(r2, w[1]); w[3] = rol(normal, w[2])
    w[4] = rol(r1, w[3]); w[5] = rol(normal, w[4]); w[6] = rol(r2, w[5]); w[7] = rol(w[6], w[6])
    mirrored = crc64(struct.pack('<8Q', *w), ~r1 & M64, True)
    return normal, mirrored


def gf(prim):
    exp, log, x = [0] * 512, [0] * 256, 1
    for i in range(255):
        exp[i], log[x] = x, i
        x <<= 1
        if x & 0x100:
            x ^= prim
    for i in range(255, 512):
        exp[i] = exp[i - 255]
    return exp, log


FIELD = {16: gf(0x169), 4: gf(0x11D)}


def gmul(f, a, b):
    exp, log = f
    return 0 if a == 0 or b == 0 else exp[log[a] + log[b]]


def syndromes(cw, parity):
    exp, log = FIELD[parity]
    out = []
    for j in range(1, parity + 1):
        y = 0
        for c in reversed(cw):
            y = (exp[log[y] + j] if y else 0) ^ c
        out.append(y)
    return out


def intact(cw, parity):
    return not any(syndromes(cw, parity))


_inverse = {}


def parity_matrix(parity):
    """A^-1 for A[j][t] = alpha^((j+1)(k+t)): the parity bytes follow from the
    message's syndromes."""
    if parity in _inverse:
        return _inverse[parity]
    f = FIELD[parity]
    exp, log = f
    k = 255 - parity
    a = [[exp[((j + 1) * (k + t)) % 255] for t in range(parity)] + [int(i == j) for i in range(parity)]
         for j in range(parity)]
    for col in range(parity):
        piv = next(r for r in range(col, parity) if a[r][col])
        a[col], a[piv] = a[piv], a[col]
        inv = exp[255 - log[a[col][col]]]
        a[col] = [gmul(f, v, inv) for v in a[col]]
        for r in range(parity):
            if r != col and a[r][col]:
                fac = a[r][col]
                a[r] = [v ^ gmul(f, fac, w) for v, w in zip(a[r], a[col])]
    _inverse[parity] = [row[parity:] for row in a]
    return _inverse[parity]


def rs_encode(message, parity):
    """255 bytes: the message (255 - parity bytes) then the parity."""
    k = 255 - parity
    assert len(message) == k
    s = syndromes(bytes(message) + bytes(parity), parity)
    inv = parity_matrix(parity)
    f = FIELD[parity]
    par = []
    for t in range(parity):
        v = 0
        for j in range(parity):
            v ^= gmul(f, inv[t][j], s[j])
        par.append(v)
    cw = bytes(message) + bytes(par)
    assert intact(cw, parity)
    return cw


def interleave(codewords):
    n = len(codewords)
    out = bytearray(n * 255)
    for j, cw in enumerate(codewords):
        for i in range(255):
            out[i * n + j] = cw[i]
    return bytes(out)


def deinterleave(span, blocks, parity):
    k = 255 - parity
    cws = [bytes(span[i * blocks + j] for i in range(255)) for j in range(blocks)]
    return cws, b''.join(cw[:k] for cw in cws)


COPY = {1: [(1, 0)], 2: [(1, 1), (1, 0)], 3: [(1, 2), (1, 1), (1, 0)], 4: [(1, 0), (1, 1), (1, 2), (1, 3)],
        5: [(1, 4), (4, 0)], 6: [(1, 5), (4, 1), (1, 0)], 7: [(2, 5), (4, 1), (1, 0)], 8: [(4, 0), (4, 4)],
        9: [(1, 8), (8, 0)], 10: [(1, 9), (8, 1), (1, 0)], 11: [(2, 9), (8, 1), (1, 0)], 12: [(4, 8), (8, 0)],
        13: [(1, 12), (4, 8), (8, 0)], 14: [(1, 13), (4, 9), (8, 1), (1, 0)], 15: [(2, 13), (4, 9), (8, 1), (1, 0)],
        16: [(8, 8), (8, 0)], 17: [(8, 9), (1, 8), (8, 0)], 18: [(1, 17), (16, 1), (1, 0)], 19: [(3, 16), (16, 0)],
        20: [(4, 16), (16, 0)], 21: [(1, 20), (4, 16), (16, 0)], 22: [(2, 20), (4, 16), (16, 0)],
        23: [(3, 20), (4, 16), (16, 0)], 24: [(8, 16), (16, 0)], 25: [(8, 17), (1, 16), (16, 0)],
        26: [(1, 25), (8, 17), (1, 16), (16, 0)], 27: [(2, 25), (8, 17), (1, 16), (16, 0)],
        28: [(4, 24), (8, 16), (16, 0)], 29: [(1, 28), (4, 24), (8, 16), (16, 0)],
        30: [(2, 28), (4, 24), (8, 16), (16, 0)], 31: [(1, 30), (4, 26), (8, 18), (16, 2), (2, 0)],
        32: [(16, 16), (16, 0)]}


def perm_order(n):
    """Indices of the n source bytes in output order (section 5.10.1)."""
    def go(base, count):
        if count == 1:
            return [base]
        return [i for size, idx in COPY[count] for i in go(base + idx, size)]
    return go(0, n)


def literal_blocks(n):
    out, pos = [], 0
    while n - pos >= 32:
        out.append((pos, 32))
        pos += 32
    if n - pos:
        out.append((pos, n - pos))
    return out


def lz(src, expected):
    """Decompress per section 5.10; raises Bad on a stream that does not make `expected` bytes."""
    out = bytearray()
    pos = 0
    try:
        op = src[pos]; pos += 1
        if op >> 4 == 2:
            pos += 2
            length = src[pos] & 7; pos += 1
            pend = None
        else:
            length, pend = 0, op
        while True:
            if length == 0:
                o = pend if pend is not None else src[pos]
                if pend is None:
                    pos += 1
                length = o + 8
                if length == 0x17:
                    n = src[pos]; pos += 1
                    length += n
                    if n == 0xFF:
                        while True:
                            w = src[pos] | (src[pos + 1] << 8); pos += 2
                            length += w
                            if w != 0xFFFF:
                                break
            pend = None
            if len(out) + length > expected:
                raise Bad('literal overruns')
            p, c = pos, length
            for start, n in literal_blocks(c):
                for i in perm_order(n):
                    out.append(src[p + start + i])
            pos += c
            if len(out) >= expected:
                break
            op = src[pos]; pos += 1
            while True:
                h = op >> 4
                if h == 0:
                    ln = (op & 15) + 0x13; so = src[pos]; pos += 1; op = src[pos]; pos += 1
                    ln += (op >> 3) & 0x10; so = ((op & 0x78) << 5) + 1 + so
                elif h == 1:
                    ln = (op & 15) + 3; so = src[pos]; pos += 1; op = src[pos]; pos += 1
                    so = ((op & 0xF8) << 5) + 1 + so
                elif h == 2:
                    so = src[pos]; pos += 1
                    so = ((src[pos] << 8) & 0xFF00) | so; pos += 1
                    ln = op & 7
                    if op & 8 == 0:
                        op = src[pos]; pos += 1
                        ln += op & 0xF8
                    else:
                        so += 1
                        ln += src[pos] << 3; pos += 1
                        op = src[pos]; pos += 1
                        ln = ((op & 0xF8) << 8) + ln + 0x100
                else:
                    ln = op >> 4; so = op & 15; op = src[pos]; pos += 1
                    so = ((op & 0xF8) << 1) + so + 1
                if so < 1 or so > len(out):
                    raise Bad('offset')
                if len(out) + ln > expected:
                    raise Bad('copy overruns')
                for _ in range(ln):
                    out.append(out[-so])
                length = op & 7
                if len(out) >= expected:
                    return bytes(out)
                if length != 0:
                    break
                op = src[pos]; pos += 1
                if op >> 4 == 0:
                    pend = op
                    break
                if op >> 4 == 15:
                    op &= 15
            if len(out) >= expected:
                break
    except IndexError:
        raise Bad('stream ends before its size') from None
    if len(out) != expected:
        raise Bad('stream ends before its size')
    return bytes(out)


def lz_encode(data, rnd, style=0):
    """A section 5.10 stream for `data` using every instruction class: greedy
    matches at any distance up to 65536, the class chosen among those that fit
    (the first, or at random when `style` is 1). Raises Bad if `data` cannot
    open with a literal."""
    n = len(data)
    index = {}
    pieces = []
    pos = lit = 0

    def note(at):
        if at + 4 <= n:
            index.setdefault(bytes(data[at:at + 4]), []).append(at)

    while pos < n:
        best = None
        if pos + 4 <= n:
            for cand in reversed(index.get(bytes(data[pos:pos + 4]), [])[-24:]):
                dist = pos - cand
                if dist > 65536:
                    break
                ln = 0
                while pos + ln < n and ln < 0xFFFF and data[pos + ln - dist] == data[pos + ln]:
                    ln += 1
                if ln >= 3 and (best is None or ln > best[1]):
                    best = (dist, ln)
        if best:
            pieces.append((lit, pos - lit, best))
            for k in range(best[1]):
                note(pos + k)
            pos += best[1]
            lit = pos
        else:
            note(pos)
            pos += 1
    if lit < n:
        pieces.append((lit, n - lit, None))

    def literal_length(length):
        if length < 0x17:
            return bytes([length - 8])
        rest = length - 0x17
        b = bytearray([0x0F])
        if rest < 0xFF:
            b.append(rest)
        else:
            b.append(0xFF)
            rest -= 0xFF
            while rest >= 0xFFFF:
                b += b'\xff\xff'
                rest -= 0xFFFF
            b += struct.pack('<H', rest)
        return bytes(b)

    def source_order(start, length):
        raw = bytearray()
        for bstart, bn in literal_blocks(length):
            block = bytearray(bn)
            for dst, src in enumerate(perm_order(bn)):
                block[src] = data[start + bstart + dst]
            raw += block
        return bytes(raw)

    def instruction(dist, ln, chained):
        """Every encoding of a copy; the last byte's low three bits are free."""
        options = []
        so = dist - 1
        if 3 <= ln <= 15 and dist <= 512 and not (ln == 15 and chained):
            options.append(bytes([(ln << 4) | (so & 15), (so >> 1) & 0xF8]))
        if 3 <= ln <= 18 and dist <= 8192:
            options.append(bytes([0x10 | (ln - 3), so & 0xFF, (so >> 5) & 0xF8]))
        if 19 <= ln <= 50 and dist <= 4096:
            extra, base = (ln - 0x13) >> 4, (ln - 0x13) & 15
            options.append(bytes([(0xF0 if chained else 0) | base, so & 0xFF, ((so >> 5) & 0x78) | (extra << 7)]))
        if ln <= 255 and dist <= 65535:
            options.append(bytes([0x20 | (ln & 7), dist & 0xFF, dist >> 8, ln & 0xF8]))
        if ln >= 0x100:
            v = ln - 0x100
            options.append(bytes([0x28 | (v & 7), so & 0xFF, so >> 8, (v >> 3) & 0xFF, ((v >> 11) & 0x1F) << 3]))
        return options

    stream = bytearray()
    slot = None
    previous_copy = False
    for k, (ls, ll, match) in enumerate(pieces):
        if ll:
            body = source_order(ls, ll)
            if k == 0:
                stream += (bytes([0x20, 0, 0, ll]) if ll < 8 else literal_length(ll)) + body
            elif ll <= 7:
                stream[slot] |= ll
                stream += body
            else:
                stream += literal_length(ll) + body
            previous_copy = False
        elif k == 0:
            raise Bad('stream must open with a literal')
        slot = None
        if match:
            options = instruction(match[0], match[1], previous_copy)
            if not options:
                raise Bad('no encoding')
            stream += options[rnd.randrange(len(options))] if style else options[0]
            slot = len(stream) - 1
            previous_copy = True
    return bytes(stream)


def jline(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False)


def align(n, a):
    return -(-n // a) * a


def system_blocks(stored, factor):
    return -(-factor * align(stored, 8) // 239)


def read_system(data, base, comp, unc, factor, crc_c, crc_u, seed):
    """A system page: parity of every codeword, both CRCs, then the plain bytes."""
    stored = min(comp, unc)
    if factor == 0 or stored == 0:
        raise Bad('system page sizes')
    blocks = system_blocks(stored, factor)
    span = data[base:base + blocks * 255]
    if len(span) != blocks * 255:
        raise Bad('system page past the end')
    cws, msg = deinterleave(span, blocks, 16)
    if not all(intact(cw, 16) for cw in cws):
        raise Bad('system page parity')
    payload = msg[:stored]
    if crc64(payload, seed1(seed, stored), True) != crc_c:
        raise Bad('system page stored crc')
    plain = lz(payload, unc) if comp < unc else payload[:unc]
    if crc64(plain, seed1(seed, unc), True) != crc_u:
        raise Bad('system page decompressed crc')
    return plain, blocks


def parse(data):
    """Everything a correct walk reports, from the spec alone: the header
    fields, the page map entries, every section with its pages and bytes."""
    if data[:6] != b'AC1021' or struct.unpack_from('<I', data, 0x28)[0] != 0x80:
        raise Bad('meta data')
    block = data[0x80:0x480]
    cws, msg = deinterleave(block[:765], 3, 16)
    if not all(intact(cw, 16) for cw in cws):
        raise Bad('header parity')
    normal, mirrored, r1, r2, _ = struct.unpack_from('<5Q', block, 0x3D8)
    if check_data(r1, r2) != (normal, mirrored):
        raise Bad('check data')
    crc0, key, ccrc = struct.unpack_from('<3Q', msg, 0)
    clen = struct.unpack_from('<i', msg, 0x18)[0]
    if crc64(struct.pack('<QQ', key, rol(key, key)), seed1(0, 16), False) != crc0:
        raise Bad('checking sequence')
    if clen < 0:
        plain = msg[0x20:0x20 - clen][:0x110]
        if len(plain) != 0x110:
            raise Bad('stored header')
    else:
        comp = msg[0x20:0x20 + clen]
        if clen == 0 or crc64(comp, seed2(0, clen), False) != ccrc:
            raise Bad('compressed header crc')
        plain = lz(comp, 0x110)
    f = list(struct.unpack('<34Q', plain))
    if crc64(plain[:0x108] + bytes(8), seed2(0, 0x110), False) != f[33]:
        raise Bad('header crc')
    for k, v in ((0, 0x70), (14, 0x20), (15, 0x40), (17, 0xF800), (18, 4), (19, 1), (29, 0x60100)):
        if f[k] != v:
            raise Bad(f'fixed field {k}')
    if f[1] > len(data) or f[3] == 0 or f[27] == 0 or PAGE_BASE + f[9] + 0x400 > len(data):
        raise Bad('header fields')
    pm_plain, _ = read_system(data, PAGE_BASE + f[7], f[10], f[11], f[3], f[2], f[16], decode10(f[4]))
    if len(pm_plain) % 16:
        raise Bad('page map length')
    entries, acc = [], 0
    for at in range(0, len(pm_plain), 16):
        size, pid = struct.unpack_from('<qq', pm_plain, at)
        if pid == 0 or abs(size) % 0x20:
            raise Bad('page map entry')
        entries.append((pid, size, acc))
        acc += abs(size)
    if len(entries) != f[12] or max(abs(e[0]) for e in entries) != f[13]:
        raise Bad('page map counts')
    own = next((e for e in entries if abs(e[0]) == f[8]), None)
    if own is None or own[2] != f[7]:
        raise Bad('page map self entry')
    smap = next((e for e in entries if abs(e[0]) == f[24]), None)
    if smap is None:
        raise Bad('section map entry')
    sm_plain, _ = read_system(data, PAGE_BASE + smap[2], f[22], f[25], f[27], f[26], f[21], decode10(f[28]))
    sections, pos, described = [], 0, 0
    while pos < len(sm_plain):
        if pos + 0x40 > len(sm_plain):
            raise Bad('section map ends inside a descriptor')
        q = struct.unpack_from('<8Q', sm_plain, pos)
        pos += 0x40
        described += 1
        size, mx, enc, hsh, raw, _, encoding, npages = q
        if raw == 0:
            if npages:
                raise Bad('unnamed descriptor with pages')
            continue
        if raw > 128 or raw % 2:
            raise Bad('section name')
        nm = sm_plain[pos:pos + raw]
        pos += raw
        if len(nm) != raw:
            raise Bad('section map ends inside a name')
        try:
            text = nm.decode('utf-16-le')
        except UnicodeDecodeError:
            raise Bad('section name utf-16') from None
        text = text.rstrip('\0')
        refs = []
        for _ in range(npages):
            if pos + 56 > len(sm_plain):
                raise Bad('section map ends inside a reference')
            refs.append(struct.unpack_from('<qQqQQQQ', sm_plain, pos))
            pos += 56
        if not text:
            if npages:
                raise Bad('empty name with pages')
            continue
        if encoding not in (1, 4) or enc not in (0, 2):
            raise Bad('section description')
        sections.append({'index': len(sections), 'name': text, 'size': size, 'max': mx, 'encryption': enc, 'hash': hsh,
                         'encoding': encoding, 'refs': refs})
    if described != f[20]:
        raise Bad('sections amount')
    seed = f[30]
    for s in sections:
        blob, end, pages = bytearray(), 0, []
        for doff, psize, pid, unc, comp, chk, crc in s['refs']:
            if pid == 0 or comp > unc or doff < end or chk > 0xFFFFFFFF:
                raise Bad('page reference')
            ent = next((e for e in entries if abs(e[0]) == abs(pid)), None)
            if ent is None:
                raise Bad('page not in the page map')
            address = PAGE_BASE + ent[2]
            if s['encoding'] == 4:
                blocks = max(1, -(-comp // 251))
                span_len = blocks * 255
            else:
                blocks, span_len = 0, comp
            if span_len > abs(ent[1]):
                raise Bad('page entry too small')
            span = data[address:address + span_len]
            if len(span) != span_len:
                raise Bad('page past the end')
            if blocks:
                cws, msg = deinterleave(span, blocks, 4)
                if not all(intact(cw, 4) for cw in cws):
                    raise Bad('page parity')
                stored = msg[:comp]
            else:
                stored = bytes(span)
            if crc64(stored, seed1(seed, comp), True) != crc:
                raise Bad('page stored crc')
            plain = lz(stored, unc) if comp < unc else stored[:unc]
            if checksum32(plain) != chk:
                raise Bad('page checksum')
            keep_from, keep_to = min(end, s['size']), min(doff, s['size'])
            hole = (keep_from, keep_to - keep_from) if keep_to > keep_from else None
            blob += bytes(doff - len(blob))
            blob += plain
            emitted = 0 if doff >= s['size'] else min(unc, s['size'] - doff)
            pages.append({'section': s['index'], 'id': pid, 'address': address, 'data_offset': doff, 'size': psize,
                          'uncompressed': unc, 'compressed': comp, 'checksum': chk, 'crc': crc, 'codewords': blocks,
                          'hole': hole, 'emitted': emitted, 'crc32': zlib.crc32(plain[:emitted]) & 0xFFFFFFFF})
            end = doff + unc
        del blob[s['size']:]
        s['pages'], s['blob'] = pages, bytes(blob)
    return {'fields': f, 'entries': entries, 'sections': sections, 'data': data}


def expected(parsed, with_done=False):
    """Every line the walk prints, in order (and, with `with_done`, the
    `section_done` lines of --out)."""
    out = [jline({'header': parsed['fields']})]
    out += [jline({'page_entry': {'id': i, 'size': s, 'offset': o}}) for i, s, o in parsed['entries']]
    pages = 0
    for s in parsed['sections']:
        out.append(jline({'section': {'index': s['index'], 'name': s['name'], 'size': s['size'], 'max': s['max'],
                                      'encryption': s['encryption'], 'hash': s['hash'], 'encoding': s['encoding'],
                                      'pages': len(s['refs'])}}))
        for p in s['pages']:
            out.append(jline({'page': {k: p[k] for k in ('section', 'id', 'address', 'data_offset', 'size', 'uncompressed',
                                                         'compressed', 'checksum', 'crc', 'codewords')}}))
            if p['hole']:
                out.append(jline({'hole': {'section': s['index'], 'offset': p['hole'][0], 'len': p['hole'][1]}}))
            out.append(jline({'page_end': {'section': s['index'], 'id': p['id'], 'plain': p['uncompressed'],
                                           'emitted': p['emitted'], 'crc32': p['crc32']}}))
            pages += 1
        out.append(jline({'section_end': {'index': s['index'], 'covered': len(s['blob'])}}))
    out.append(jline({'finished': {'sections': len(parsed['sections']), 'pages': pages}}))
    if with_done:
        out += [jline({'section_done': {'index': s['index'], 'name': s['name'], 'size': len(s['blob']),
                                        'pages': len(s['pages']), 'crc32': zlib.crc32(s['blob']) & 0xFFFFFFFF}})
                for s in parsed['sections']]
    return out


def system_span(plain, factor, seed, rnd, compress=True, raw_stored=None):
    """A system page: stored bytes (compressed when shorter), repeated `factor`
    times, Reed-Solomon coded and interleaved. Returns (span, comp, crc_comp,
    crc_unc)."""
    unc = len(plain)
    stored = plain
    if raw_stored is not None:
        stored = raw_stored
    elif compress:
        packed = lz_encode(plain, rnd, 1)
        if len(packed) < unc:
            stored = packed
    comp = len(stored) if raw_stored is not None or len(stored) < unc else unc
    padded = bytes(stored) + bytes(align(len(stored), 8) - len(stored))
    blocks = system_blocks(len(stored), factor)
    msg = (padded * factor).ljust(blocks * 239, b'\0')[:blocks * 239]
    span = interleave([rs_encode(msg[j * 239:(j + 1) * 239], 16) for j in range(blocks)])
    crc_c = crc64(bytes(stored), seed1(seed, len(stored)), True)
    crc_u = crc64(plain, seed1(seed, unc), True)
    return span, comp, crc_c, crc_u


def data_span(stored, encoding):
    if encoding != 4:
        return bytes(stored)
    blocks = max(1, -(-len(stored) // 251))
    msg = bytes(stored).ljust(blocks * 251, b'\0')
    return interleave([rs_encode(msg[j * 251:(j + 1) * 251], 4) for j in range(blocks)])


def build(spec):
    """A whole R2007 file. spec: {'seed', 'crc_seed' (10 bits), 'factors': (pm, sm), 'sections': [...], 'order':
    data-page permutation, 'free': [(position, size)] free pages, 'compress_maps', 'compress_header',
    'unnamed': extra unnamed descriptors, 'tamper': {...}}. A section is {'name', 'data', 'encoding', 'encryption',
    'pages': [{'start', 'len', 'compress', 'stored' (raw override), 'crc_delta', 'sum_delta', 'unc', 'comp', 'id',
    'drop' (omit from page map), 'entry_size'}], 'size' override, 'name_raw' override}."""
    rnd = random.Random(spec.get('seed', 1))
    t = spec.get('tamper', {})
    crc_seed = spec.get('crc_seed', 0)
    pm_factor, sm_factor = spec.get('factors', (3, 2))
    pages, ident = [], 3
    for si, s in enumerate(spec['sections']):
        for pg in s['pages']:
            plain = bytes(s['data'][pg['start']:pg['start'] + pg['len']]).ljust(pg['len'], b'\0')
            stored = plain
            if 'stored' in pg:
                stored = pg['stored']
            elif pg.get('compress', s['encoding'] == 4):
                packed = lz_encode(plain, rnd, 1) if len(plain) else b''
                if 0 < len(packed) < len(plain):
                    stored = packed
            comp = len(stored)
            span = data_span(stored, s['encoding'])
            pages.append({'section': si, 'start': pg['start'], 'plain': plain, 'stored': stored, 'comp': pg.get('comp', comp),
                          'unc': pg.get('unc', len(plain)), 'sum': (checksum32(plain) + pg.get('sum_delta', 0)) & 0xFFFFFFFF,
                          'crc': (crc64(bytes(stored), seed1(crc_seed, comp), True) + pg.get('crc_delta', 0)) & M64,
                          'id': pg.get('id', ident), 'span': span, 'slot': pg.get('entry_size', align(len(span), 0x20)),
                          'drop': pg.get('drop', False), 'size': pg.get('size', len(plain)), 'doff': pg.get('doff', pg['start'])})
            ident += 1
    order = spec.get('order') or list(range(len(pages)))
    pm_id, pm2_id, sm_id, sm2_id = ident, ident + 1, ident + 2, ident + 3
    pm_seed = sm_seed = crc_seed
    # The section map does not depend on the layout.
    smap = bytearray()
    for si, s in enumerate(spec['sections']):
        nm = s.get('name_raw', (s['name'] + '\0').encode('utf-16-le'))
        refs = [p for p in pages if p['section'] == si]
        smap += struct.pack('<8Q', s.get('size', len(s['data'])), s.get('max', 0xF800), s.get('encryption', 0), s.get('hash', 0x1234 + si),
                            len(nm), 0, s.get('enc_field', s['encoding']), len(refs))
        smap += nm
        for p in refs:
            smap += struct.pack('<qQqQQQQ', p['doff'], p['size'], p['id'], p['unc'], p['comp'], p['sum'], p['crc'])
    for _ in range(spec.get('unnamed', 1)):
        smap += struct.pack('<8Q', 0, 0, 0, 0, 0, 0, 0, 0)
    smap_plain = bytes(smap)
    if 'smap_edit' in spec:
        smap_plain = spec['smap_edit'](smap_plain)
    sm_span, sm_comp, sm_crc_c, sm_crc_u = system_span(smap_plain, sm_factor, sm_seed, rnd, spec.get('compress_maps', True))
    sm_slot = align(len(sm_span), 0x20)
    # The page map lists itself, so its slot size is a fixed point.
    slot = 0x400
    for _ in range(12):
        entries = [(pm_id, slot), (pm2_id, slot)]
        body = [(pages[i]['id'], pages[i]['slot']) for i in order if not pages[i]['drop']]
        for position, size in sorted(spec.get('free', []), reverse=True):
            body.insert(min(position, len(body)), (-(1000 + position), size))
        entries += body + [(sm_id, sm_slot), (sm2_id, sm_slot)]
        pm_plain = b''.join(struct.pack('<qq', size, ident_) for ident_, size in entries)
        if 'pmap_edit' in spec:
            pm_plain = spec['pmap_edit'](pm_plain)
        pm_span, pm_comp, pm_crc_c, pm_crc_u = system_span(pm_plain, pm_factor, pm_seed, rnd, spec.get('compress_maps', True))
        if align(len(pm_span), 0x20) <= slot and (align(len(pm_span), 0x20) == slot or slot == 0x400):
            break
        slot = align(len(pm_span), 0x20)
    else:
        raise Bad('page map slot does not settle')
    slot = max(slot, align(len(pm_span), 0x20))
    body = bytearray()
    body += pm_span.ljust(slot, b'\0')
    body += pm_span.ljust(slot, b'\0')
    # Free pages sit in the file where the page map lists them.
    seq = [('page', i) for i in order if not pages[i]['drop']]
    for position, size in sorted(spec.get('free', []), reverse=True):
        seq.insert(min(position, len(seq)), ('free', size))
    for kind, v in seq:
        if kind == 'page':
            body += pages[v]['span'][:pages[v]['slot']].ljust(pages[v]['slot'], b'\0')
        else:
            body += bytes(v)
    body += sm_span.ljust(sm_slot, b'\0')
    body += sm_span.ljust(sm_slot, b'\0')
    header2_off = len(body)
    file_len = PAGE_BASE + len(body) + 0x400
    f = [0] * 34
    f[0], f[1] = 0x70, file_len
    f[2], f[3], f[4] = pm_crc_c, pm_factor, encode10(pm_seed, rnd)
    f[5], f[6], f[7], f[8], f[9] = slot, pm2_id, 0, pm_id, header2_off
    f[10], f[11], f[12], f[13] = pm_comp, len(pm_plain), len(entries), max(abs(e[0]) for e in entries)
    f[14], f[15], f[16], f[17], f[18], f[19] = 0x20, 0x40, pm_crc_u, 0xF800, 4, 1
    f[20] = len(spec['sections']) + spec.get('unnamed', 1)
    f[21], f[22], f[23], f[24], f[25], f[26], f[27] = sm_crc_u, sm_comp, sm2_id, sm_id, len(smap_plain), sm_crc_c, sm_factor
    f[28], f[29], f[30] = encode10(sm_seed, rnd), 0x60100, crc_seed
    f[31], f[32] = encode10(crc_seed, rnd), rnd.getrandbits(64)
    for name, k in (('pm_crc_comp', 2), ('pm_crc_unc', 16), ('sm_crc_comp', 26), ('sm_crc_unc', 21)):
        if name in t:
            f[k] = (f[k] + t[name]) & M64
    for k, v in spec.get('fields', {}).items():
        f[k] = v
    plain = bytearray(struct.pack('<34Q', *f))
    plain[0x108:] = bytes(8)
    f[33] = (crc64(bytes(plain), seed2(0, 0x110), False) + t.get('hdr_crc', 0)) & M64
    plain = struct.pack('<34Q', *f)
    packed = lz_encode(plain, rnd, 1) if spec.get('compress_header', True) else b''
    if packed and len(packed) < 0x110:
        payload, clen = packed, len(packed)
        ccrc = crc64(payload, seed2(0, clen), False)
    else:
        payload, clen = plain, -len(plain)
        ccrc = crc64(payload, seed2(0, len(payload)), False)
    ccrc = (ccrc + t.get('ccrc', 0)) & M64
    key = rnd.getrandbits(64)
    crc0 = (crc64(struct.pack('<QQ', key, rol(key, key)), seed1(0, 16), False) + t.get('key_crc', 0)) & M64
    msg = bytearray(struct.pack('<QQQiI', crc0, key, ccrc, clen, 0))
    msg += payload
    msg = bytes(bytearray(msg[:0x20 + len(payload)]) + bytearray(rnd.getrandbits(8) for _ in range(717 - 0x20 - len(payload))))
    head_block = interleave([rs_encode(msg[j * 239:(j + 1) * 239], 16) for j in range(3)])
    head_block += bytes(rnd.getrandbits(8) for _ in range(0x3D8 - len(head_block)))
    r1, r2 = rnd.getrandbits(64), rnd.getrandbits(64)
    normal, mirrored = check_data(r1, r2)
    normal = (normal + t.get('check_normal', 0)) & M64
    mirrored = (mirrored + t.get('check_mirror', 0)) & M64
    head_block += struct.pack('<5Q', normal, mirrored, r1, r2, rnd.getrandbits(64))
    assert len(head_block) == 0x400
    meta = bytearray(b'AC1021' + bytes(0x7A))
    meta[0x0C] = 3
    struct.pack_into('<I', meta, 0x28, 0x80)
    out = bytes(meta) + head_block + bytes(body) + head_block
    assert len(out) == file_len, (len(out), file_len)
    return out
