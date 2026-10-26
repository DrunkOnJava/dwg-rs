//! The checks of the R2007 (`AC1021`) container (ODA v5.4.1 §5.2, §5.4.1,
//! §5.12, §5.13), as constants and small accumulators with no allocation:
//! the two Reed-Solomon codes' syndromes (a codeword is intact when all of
//! them are zero), the two 64-bit CRCs with their seed updates, the 32-bit
//! data-page checksum and the §5.11 ten-bit decoder.
//!
//! The byte order the CRCs and the checksum consume is not the order of the
//! bytes: §5.12 and §5.4.1 take an 8-byte block as `6 7 4 5 2 3 0 1` and a
//! 1 to 7 byte remainder from the tables below. Every rule here was
//! measured against the 11 R2007 drawings of the corpus (see ARCHITECTURE.md
//! on the R2007 checks) before being enforced.

/// Bytes in a codeword of either code (§5.13).
pub(super) const CODEWORD: usize = 255;

/// One of §5.13's two Reed-Solomon codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Code {
    /// (255, 239) over `0x169`: the file header, the page map and the
    /// section map.
    System = 0,
    /// (255, 251) over `0x11D`: data pages whose section has encoding 4.
    Data = 1,
}

impl Code {
    pub(super) const fn parity(self) -> usize {
        match self {
            Code::System => 16,
            Code::Data => 4,
        }
    }

    /// Message bytes of a codeword.
    pub(super) const fn message(self) -> usize {
        CODEWORD - self.parity()
    }

    fn field(self) -> &'static Field {
        match self {
            Code::System => &SYSTEM_FIELD,
            Code::Data => &DATA_FIELD,
        }
    }
}

struct Field {
    exp: [u8; 512],
    log: [u8; 256],
}

const fn field(primitive: u16) -> Field {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= primitive;
        }
        i += 1;
    }
    while i < 512 {
        exp[i] = exp[i - 255];
        i += 1;
    }
    Field { exp, log }
}

static SYSTEM_FIELD: Field = field(0x169);
static DATA_FIELD: Field = field(0x11D);

/// Whether the 255 bytes are an intact codeword: the syndromes
/// `S_j = c(alpha^j)`, `j = 1 ..= n - k`, with byte `i` the coefficient of
/// `x^i` (the layout measured on every codeword of the corpus), all vanish.
pub(super) fn codeword_intact(code: Code, codeword: &[u8]) -> bool {
    if codeword.len() != CODEWORD {
        return false;
    }
    match code {
        Code::System => syndromes_vanish::<16>(code.field(), codeword),
        Code::Data => syndromes_vanish::<4>(code.field(), codeword),
    }
}

fn syndromes_vanish<const N: usize>(f: &Field, codeword: &[u8]) -> bool {
    let mut s = [0u8; N];
    for &c in codeword.iter().rev() {
        for (j, y) in s.iter_mut().enumerate() {
            let shifted = if *y == 0 {
                0
            } else {
                f.exp[usize::from(f.log[usize::from(*y)]) + j + 1]
            };
            *y = shifted ^ c;
        }
    }
    s.iter().all(|&y| y == 0)
}

const fn crc_table(polynomial: u64, mirrored: bool) -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = if mirrored { i as u64 } else { (i as u64) << 56 };
        let mut bit = 0;
        while bit < 8 {
            c = if mirrored {
                if c & 1 != 0 {
                    (c >> 1) ^ polynomial
                } else {
                    c >> 1
                }
            } else if c >> 63 != 0 {
                (c << 1) ^ polynomial
            } else {
                c << 1
            };
            bit += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

/// §5.12.1: ECMA-182, most significant bit first.
static NORMAL: [u64; 256] = crc_table(0x42f0_e1eb_a9ea_3693, false);
/// §5.12.2: the same computation shifting the other way.
static MIRRORED: [u64; 256] = crc_table(0x95ac_9329_ac4b_c9b5, true);

/// §5.12 and §5.4.1 take bytes `6 7 4 5 2 3 0 1` of a whole 8-byte block.
const BLOCK_ORDER: [usize; 8] = [6, 7, 4, 5, 2, 3, 0, 1];
/// Bytes of a 1 to 7 byte remainder in the order they are taken.
const TAIL_ORDER: [&[usize]; 8] = [
    &[],
    &[0],
    &[0, 1],
    &[0, 1, 2],
    &[2, 3, 0, 1],
    &[2, 3, 0, 1, 4],
    &[2, 3, 0, 1, 4, 5],
    &[2, 3, 0, 1, 4, 5, 6],
];

/// §5.12 `UpdateSeed1`, for mirrored CRCs and the checking sequence.
pub(super) fn seed1(seed: u64, length: u64) -> u64 {
    let mut s = seed
        .wrapping_add(length)
        .wrapping_mul(0x343fd)
        .wrapping_add(0x269ec3);
    s |= s.wrapping_mul(0x343fd << 32).wrapping_add(0x269ec3 << 32);
    !s
}

/// §5.12 `UpdateSeed2`, for the compressed data and the header.
pub(super) fn seed2(seed: u64, length: u64) -> u64 {
    let mut s = seed
        .wrapping_add(length)
        .wrapping_mul(0x343fd)
        .wrapping_add(0x269ec3);
    s = s
        .wrapping_mul((1 << 32) + 0x343fd)
        .wrapping_add(length.wrapping_add(0x269ec3));
    !s
}

/// A 64-bit CRC that takes its bytes in any number of pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Crc64 {
    pub(super) crc: u64,
    pub(super) buf: [u8; 8],
    pub(super) held: u8,
    pub(super) mirrored: bool,
}

impl Crc64 {
    pub(super) const fn new(seed: u64, mirrored: bool) -> Self {
        Self {
            crc: seed,
            buf: [0; 8],
            held: 0,
            mirrored,
        }
    }

    fn step(&mut self, byte: u8) {
        self.crc = if self.mirrored {
            MIRRORED[((self.crc ^ u64::from(byte)) & 0xff) as usize] ^ (self.crc >> 8)
        } else {
            NORMAL[((u64::from(byte) ^ (self.crc >> 56)) & 0xff) as usize] ^ (self.crc << 8)
        };
    }

    pub(super) fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.buf[usize::from(self.held)] = byte;
            self.held += 1;
            if self.held == 8 {
                for at in BLOCK_ORDER {
                    self.step(self.buf[at]);
                }
                self.held = 0;
            }
        }
    }

    /// The CRC of everything given, inverted for the normal flavour.
    pub(super) fn finish(mut self) -> u64 {
        for &at in TAIL_ORDER[usize::from(self.held)] {
            self.step(self.buf[at]);
        }
        if self.mirrored { self.crc } else { !self.crc }
    }
}

/// §5.4.1: the 32-bit checksum of a page's decompressed bytes, a running
/// pair of sums reduced modulo `0xFFF1` after every `0x15B0` bytes. The
/// length is part of the seed, so it is known up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Sum32 {
    pub(super) s1: u32,
    pub(super) s2: u32,
    pub(super) buf: [u8; 8],
    pub(super) held: u8,
    /// Bytes of whole blocks since the last reduction.
    pub(super) chunk: u16,
}

const SUM_CHUNK: u16 = 0x15b0;
const SUM_MODULUS: u32 = 0xfff1;

impl Sum32 {
    pub(super) fn new(length: u64) -> Self {
        let seed = length.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        Self {
            s1: (seed & 0xffff) as u32,
            s2: ((seed >> 16) & 0xffff) as u32,
            buf: [0; 8],
            held: 0,
            chunk: 0,
        }
    }

    fn add(&mut self, byte: u8) {
        self.s1 = self.s1.wrapping_add(u32::from(byte));
        self.s2 = self.s2.wrapping_add(self.s1);
    }

    fn reduce(&mut self) {
        self.s1 %= SUM_MODULUS;
        self.s2 %= SUM_MODULUS;
    }

    pub(super) fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.buf[usize::from(self.held)] = byte;
            self.held += 1;
            if self.held == 8 {
                for at in BLOCK_ORDER {
                    self.add(self.buf[at]);
                }
                self.held = 0;
                self.chunk += 8;
                if self.chunk == SUM_CHUNK {
                    self.reduce();
                    self.chunk = 0;
                }
            }
        }
    }

    pub(super) fn finish(mut self) -> u32 {
        for &at in TAIL_ORDER[usize::from(self.held)] {
            self.add(self.buf[at]);
        }
        self.reduce();
        (self.s2 << 16) | (self.s1 & 0xffff)
    }
}

/// §5.11: the ten data bits of a CRC seed that the random encoder spread
/// over a 64-bit value.
pub(super) fn decode_seed(value: u64) -> u64 {
    let (hi, lo) = ((value >> 32) as u32, value as u32);
    let mut result = 0u64;
    for (mask, bit) in [
        (0x0800_0000, 0x01),
        (0x0020_0000, 0x02),
        (0x0000_8000, 0x04),
        (0x0000_0200, 0x08),
        (0x0000_0008, 0x10),
    ] {
        if hi & mask != 0 {
            result |= bit;
        }
    }
    for (mask, bit) in [
        (0x2000_0000, 0x20),
        (0x0080_0000, 0x40),
        (0x0002_0000, 0x80),
        (0x0000_0800, 0x100),
        (0x0000_0020, 0x200),
    ] {
        if lo & mask != 0 {
            result |= bit;
        }
    }
    result
}

/// §5.2.1.1.5's `Encode`: a rotation by the low five bits of `control`.
fn encode(value: u64, control: u64) -> u64 {
    value.rotate_left((control & 0x1f) as u32)
}

fn crc_of_words(words: &[u64; 8], seed: u64, mirrored: bool) -> u64 {
    let mut crc = Crc64::new(seed, mirrored);
    for word in words {
        crc.update(&word.to_le_bytes());
    }
    crc.finish()
}

/// §5.2.1.1.5: the five values at `0x3D8` of the file header block hold the
/// normal and mirrored CRCs of two eight-word buffers derived from the two
/// random values. `Err((check, expected, actual))` names the first that
/// fails (3 for the normal one, 4 for the mirrored one).
pub(super) fn check_data(
    normal: u64,
    mirrored: u64,
    r1: u64,
    r2: u64,
) -> Result<(), (u8, u64, u64)> {
    let mut w = [0u64; 8];
    w[0] = encode(r1, r2);
    w[1] = encode(w[0], w[0]);
    w[2] = encode(r2, w[1]);
    w[3] = encode(w[2], w[2]);
    w[4] = encode(r1, w[3]);
    w[5] = encode(w[4], w[4]);
    w[6] = encode(w[5], w[5]);
    w[7] = encode(w[6], w[6]);
    let actual = crc_of_words(&w, !r2, false);
    if actual != normal {
        return Err((3, normal, actual));
    }
    w[0] = encode(r1, r2);
    w[1] = encode(normal, w[0]);
    w[2] = encode(r2, w[1]);
    w[3] = encode(normal, w[2]);
    w[4] = encode(r1, w[3]);
    w[5] = encode(normal, w[4]);
    w[6] = encode(r2, w[5]);
    w[7] = encode(w[6], w[6]);
    let actual = crc_of_words(&w, !r1, true);
    if actual != mirrored {
        return Err((4, mirrored, actual));
    }
    Ok(())
}

/// §5.2.1.4: the CRC of the two-value checking sequence that opens the
/// decoded file header block: the key and the key rotated by itself.
pub(super) fn sequence_crc(key: u64) -> u64 {
    let mut crc = Crc64::new(seed1(0, 16), false);
    crc.update(&key.to_le_bytes());
    crc.update(&encode(key, key).to_le_bytes());
    crc.finish()
}
