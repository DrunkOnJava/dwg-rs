//! Bounded, resumable decoder for the R2007 (`AC1021`) LZ dialect (ODA
//! v5.4.1 §5.10). It performs no I/O and holds no allocation: the caller
//! hands it any prefix of the compressed bytes and an output slice of any
//! size, and it returns how much of each it used. It can stop between any
//! two calls, so a page never has to be held whole.
//!
//! The stream is a literal run, then back-references, then a literal run and
//! so on. The semantics are the crate's own `r21_lz::decompress`, which §5.10
//! and the corpus fix: a literal run is copied out of order, in blocks of 32
//! bytes whose bytes are permuted by the §5.10.1 table (the decoder holds one
//! block of input and one of output), and a back-reference reaches at most
//! 65,536 bytes back (a 16-bit offset plus one), so the window keeps the last
//! [`WINDOW`] output bytes and no more, however long the stream.
//!
//! Where the legacy decoder stops quietly when the input runs out, this one
//! reports it: the container always says how many bytes a stream must
//! produce, and [`R2007Lz::end_of_input`] turns a stream that ends before
//! them into `Lz77Truncated`.

use super::StreamError;

/// Output bytes retained for back-references: the largest encodable
/// distance.
pub(super) const WINDOW: usize = 0x1_0000;

/// §5.10.1: `(block size, source index)` pairs that make up the copy of
/// `n` bytes; a block's own bytes follow the same table.
const COPY_BLOCKS: [&[(usize, usize)]; 33] = [
    &[],
    &[(1, 0)],
    &[(1, 1), (1, 0)],
    &[(1, 2), (1, 1), (1, 0)],
    &[(1, 0), (1, 1), (1, 2), (1, 3)],
    &[(1, 4), (4, 0)],
    &[(1, 5), (4, 1), (1, 0)],
    &[(2, 5), (4, 1), (1, 0)],
    &[(4, 0), (4, 4)],
    &[(1, 8), (8, 0)],
    &[(1, 9), (8, 1), (1, 0)],
    &[(2, 9), (8, 1), (1, 0)],
    &[(4, 8), (8, 0)],
    &[(1, 12), (4, 8), (8, 0)],
    &[(1, 13), (4, 9), (8, 1), (1, 0)],
    &[(2, 13), (4, 9), (8, 1), (1, 0)],
    &[(8, 8), (8, 0)],
    &[(8, 9), (1, 8), (8, 0)],
    &[(1, 17), (16, 1), (1, 0)],
    &[(3, 16), (16, 0)],
    &[(4, 16), (16, 0)],
    &[(1, 20), (4, 16), (16, 0)],
    &[(2, 20), (4, 16), (16, 0)],
    &[(3, 20), (4, 16), (16, 0)],
    &[(8, 16), (16, 0)],
    &[(8, 17), (1, 16), (16, 0)],
    &[(1, 25), (8, 17), (1, 16), (16, 0)],
    &[(2, 25), (8, 17), (1, 16), (16, 0)],
    &[(4, 24), (8, 16), (16, 0)],
    &[(1, 28), (4, 24), (8, 16), (16, 0)],
    &[(2, 28), (4, 24), (8, 16), (16, 0)],
    &[(1, 30), (4, 26), (8, 18), (16, 2), (2, 0)],
    &[(16, 16), (16, 0)],
];

/// Writes into `out`, from `at`, the source indices of the bytes of the
/// `count`-byte copy at `base`, in the order §5.10.1's table hands them out.
const fn expand(count: usize, base: usize, out: &mut [u8; 32], at: &mut usize) {
    if count == 1 {
        out[*at] = base as u8;
        *at += 1;
        return;
    }
    let blocks = COPY_BLOCKS[count];
    let mut k = 0;
    while k < blocks.len() {
        let (size, index) = blocks[k];
        expand(size, base + index, out, at);
        k += 1;
    }
}

/// For a block of `n` bytes, output byte `k` is input byte `PERMUTATION[n][k]`:
/// the §5.10.1 table flattened at compile time, so a block is one gather.
static PERMUTATION: [[u8; 32]; 33] = {
    let mut table = [[0u8; 32]; 33];
    let mut n = 1;
    while n <= 32 {
        let mut at = 0;
        expand(n, 0, &mut table[n], &mut at);
        n += 1;
    }
    table
};

fn emit(src: &[u8; 32], count: usize, out: &mut [u8; 32]) {
    for (slot, &index) in out[..count].iter_mut().zip(&PERMUTATION[count][..count]) {
        *slot = src[usize::from(index)];
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Mode {
    /// First byte: an opcode whose high nibble is 2, or a literal-length opcode.
    Start = 0,
    /// Two bytes to skip after a first opcode of `0x2_`.
    Skip = 1,
    /// The byte whose low three bits are the first literal length.
    LenByte = 2,
    /// A literal-length opcode: the length is the opcode plus 8.
    LitOp = 3,
    /// The byte after a length of `0x17`.
    LitExt = 4,
    /// Low byte of a 16-bit length word.
    LitWord0 = 5,
    /// High byte of a 16-bit length word; `lo` holds the low byte.
    LitWord1 = 6,
    /// Collecting one block of a literal run.
    Block = 7,
    /// Handing out a permuted block.
    Emit = 8,
    /// The opcode after a literal run.
    Opcode = 9,
    /// The operand bytes of a back-reference.
    Operands = 10,
    /// `count` back-reference bytes still to hand out from `dist` back.
    Copy = 11,
    /// The opcode after a back-reference that carried no literal length.
    AfterCopy = 12,
    Done = 13,
}

impl Mode {
    pub(super) fn from_u8(byte: u8) -> Option<Mode> {
        Some(match byte {
            0 => Mode::Start,
            1 => Mode::Skip,
            2 => Mode::LenByte,
            3 => Mode::LitOp,
            4 => Mode::LitExt,
            5 => Mode::LitWord0,
            6 => Mode::LitWord1,
            7 => Mode::Block,
            8 => Mode::Emit,
            9 => Mode::Opcode,
            10 => Mode::Operands,
            11 => Mode::Copy,
            12 => Mode::AfterCopy,
            13 => Mode::Done,
            _ => return None,
        })
    }
}

/// Bytes consumed and produced by one [`R2007Lz::decode`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Progress {
    pub(super) consumed: usize,
    pub(super) produced: usize,
}

/// Fixed-size §5.10 decoder state. `limit` is the declared decompressed
/// size: reaching it ends the stream, and a run or copy that would pass it
/// is an error, never an allocation.
#[derive(Debug, Clone)]
pub(super) struct R2007Lz {
    pub(super) mode: Mode,
    /// The opcode the operands belong to, then the one that follows it.
    pub(super) op: u8,
    pub(super) skip: u8,
    pub(super) lo: u8,
    pub(super) ins: [u8; 4],
    pub(super) ins_have: u8,
    pub(super) ins_need: u8,
    /// Literal bytes not yet collected, or copy bytes not yet handed out.
    pub(super) count: u64,
    /// A literal length being read.
    pub(super) acc: u64,
    pub(super) dist: u32,
    pub(super) blk: [u8; 32],
    pub(super) blk_have: u8,
    pub(super) blk_need: u8,
    pub(super) pend: [u8; 32],
    pub(super) pend_len: u8,
    pub(super) pend_pos: u8,
    pub(super) produced: u64,
    pub(super) consumed: u64,
    pub(super) limit: u64,
    pub(super) failure: Option<StreamError>,
    pub(super) window: [u8; WINDOW],
}

impl R2007Lz {
    pub(super) fn blank() -> Self {
        Self {
            mode: Mode::Start,
            op: 0,
            skip: 0,
            lo: 0,
            ins: [0; 4],
            ins_have: 0,
            ins_need: 0,
            count: 0,
            acc: 0,
            dist: 0,
            blk: [0; 32],
            blk_have: 0,
            blk_need: 0,
            pend: [0; 32],
            pend_len: 0,
            pend_pos: 0,
            produced: 0,
            consumed: 0,
            limit: 0,
            failure: None,
            window: [0; WINDOW],
        }
    }

    /// Begin a stream that may not produce more than `limit` bytes, keeping
    /// the window's storage (its old bytes are never read before they are
    /// overwritten: a distance cannot pass `produced`).
    pub(super) fn reset(&mut self, limit: u64) {
        self.mode = Mode::Start;
        self.op = 0;
        self.skip = 0;
        self.lo = 0;
        self.ins = [0; 4];
        self.ins_have = 0;
        self.ins_need = 0;
        self.count = 0;
        self.acc = 0;
        self.dist = 0;
        self.blk = [0; 32];
        self.blk_have = 0;
        self.blk_need = 0;
        self.pend = [0; 32];
        self.pend_len = 0;
        self.pend_pos = 0;
        self.produced = 0;
        self.consumed = 0;
        self.limit = limit;
        self.failure = None;
    }

    pub(super) fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Whether more output can be produced without more input.
    pub(super) fn has_pending_output(&self) -> bool {
        matches!(self.mode, Mode::Emit | Mode::Copy)
    }

    /// Decode from `input` into `out` until the input is used up, `out` is
    /// full or the stream ends. A grammar error is terminal; output already
    /// returned stays provisional until the caller's commit point.
    pub(super) fn decode(&mut self, input: &[u8], out: &mut [u8]) -> Result<Progress, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let (mut i, mut o) = (0usize, 0usize);
        loop {
            match self.mode {
                Mode::Done => break,
                Mode::Emit => {
                    if o == out.len() {
                        break;
                    }
                    let at = usize::from(self.pend_pos);
                    let n = (out.len() - o).min(usize::from(self.pend_len) - at);
                    for (slot, &byte) in out[o..o + n].iter_mut().zip(&self.pend[at..at + n]) {
                        self.window[(self.produced as usize) & (WINDOW - 1)] = byte;
                        self.produced += 1;
                        *slot = byte;
                    }
                    o += n;
                    self.pend_pos += n as u8;
                    if self.pend_pos == self.pend_len {
                        self.after_block();
                    }
                }
                Mode::Copy => {
                    if o == out.len() {
                        break;
                    }
                    let n = (out.len() - o).min(usize::try_from(self.count).unwrap_or(usize::MAX));
                    for slot in &mut out[o..o + n] {
                        let from = (self.produced - u64::from(self.dist)) as usize & (WINDOW - 1);
                        let byte = self.window[from];
                        self.window[(self.produced as usize) & (WINDOW - 1)] = byte;
                        self.produced += 1;
                        *slot = byte;
                    }
                    o += n;
                    self.count -= n as u64;
                    if self.count == 0 {
                        if let Err(error) = self.after_copy() {
                            return self.fail(error);
                        }
                    }
                }
                Mode::Block => {
                    if i == input.len() {
                        break;
                    }
                    let want = usize::from(self.blk_need - self.blk_have);
                    let n = want.min(input.len() - i);
                    let Some(consumed) = self.consumed.checked_add(n as u64) else {
                        return self.fail(StreamError::CounterOverflow);
                    };
                    let at = usize::from(self.blk_have);
                    self.blk[at..at + n].copy_from_slice(&input[i..i + n]);
                    self.blk_have += n as u8;
                    self.consumed = consumed;
                    i += n;
                    if self.blk_have == self.blk_need {
                        let need = usize::from(self.blk_need);
                        let mut pend = [0u8; 32];
                        emit(&self.blk, need, &mut pend);
                        self.pend = pend;
                        self.pend_len = self.blk_need;
                        self.pend_pos = 0;
                        self.count -= need as u64;
                        self.blk = [0; 32];
                        self.blk_have = 0;
                        self.blk_need = 0;
                        self.mode = Mode::Emit;
                    }
                }
                _ => {
                    if i == input.len() {
                        break;
                    }
                    let byte = input[i];
                    let at = self.consumed;
                    let Some(consumed) = at.checked_add(1) else {
                        return self.fail(StreamError::CounterOverflow);
                    };
                    i += 1;
                    self.consumed = consumed;
                    if let Err(error) = self.step(byte, at) {
                        return self.fail(error);
                    }
                }
            }
        }
        Ok(Progress {
            consumed: i,
            produced: o,
        })
    }

    fn fail(&mut self, error: StreamError) -> Result<Progress, StreamError> {
        self.failure = Some(error);
        Err(error)
    }

    fn start_literal(&mut self, length: u64) -> Result<(), StreamError> {
        if length > self.limit - self.produced {
            return Err(StreamError::Lz77Overrun { limit: self.limit });
        }
        self.count = length;
        self.acc = 0;
        self.blk_have = 0;
        self.blk_need = length.min(32) as u8;
        self.mode = Mode::Block;
        Ok(())
    }

    fn literal_opcode(&mut self, op: u8) -> Result<(), StreamError> {
        let length = u64::from(op) + 8;
        if length == 0x17 {
            self.acc = length;
            self.mode = Mode::LitExt;
            Ok(())
        } else {
            self.start_literal(length)
        }
    }

    /// A block has been handed out in full.
    fn after_block(&mut self) {
        self.pend = [0; 32];
        self.pend_len = 0;
        self.pend_pos = 0;
        if self.produced == self.limit {
            self.mode = Mode::Done;
        } else if self.count > 0 {
            self.blk_have = 0;
            self.blk_need = self.count.min(32) as u8;
            self.mode = Mode::Block;
        } else {
            self.mode = Mode::Opcode;
        }
    }

    /// A back-reference has been handed out in full.
    fn after_copy(&mut self) -> Result<(), StreamError> {
        self.dist = 0;
        self.count = 0;
        let literal = u64::from(self.op & 7);
        self.op = 0;
        if self.produced == self.limit {
            self.mode = Mode::Done;
        } else if literal != 0 {
            self.start_literal(literal)?;
        } else {
            self.mode = Mode::AfterCopy;
        }
        Ok(())
    }

    fn begin_operands(&mut self, op: u8) {
        self.op = op;
        self.ins = [0; 4];
        self.ins_have = 0;
        self.ins_need = match op >> 4 {
            0 | 1 => 2,
            2 if op & 8 == 0 => 3,
            2 => 4,
            _ => 1,
        };
        self.mode = Mode::Operands;
    }

    fn step(&mut self, byte: u8, at: u64) -> Result<(), StreamError> {
        match self.mode {
            Mode::Start => {
                if byte >> 4 == 2 {
                    self.skip = 2;
                    self.mode = Mode::Skip;
                } else {
                    self.literal_opcode(byte)?;
                }
            }
            Mode::Skip => {
                self.skip -= 1;
                if self.skip == 0 {
                    self.mode = Mode::LenByte;
                }
            }
            Mode::LenByte => {
                let length = byte & 7;
                if length == 0 {
                    self.mode = Mode::LitOp;
                } else {
                    self.start_literal(u64::from(length))?;
                }
            }
            Mode::LitOp => self.literal_opcode(byte)?,
            Mode::LitExt => {
                self.acc = self.acc.saturating_add(u64::from(byte));
                if byte == 0xff {
                    self.mode = Mode::LitWord0;
                } else {
                    self.start_literal(self.acc)?;
                }
            }
            Mode::LitWord0 => {
                self.lo = byte;
                self.mode = Mode::LitWord1;
            }
            Mode::LitWord1 => {
                let word = u64::from(self.lo) | (u64::from(byte) << 8);
                self.lo = 0;
                self.acc = self.acc.saturating_add(word);
                if word == 0xffff {
                    // Every length must fit the declared output; stop a
                    // runaway chain before it can overflow.
                    if self.acc > self.limit {
                        return Err(StreamError::Lz77Overrun { limit: self.limit });
                    }
                    self.mode = Mode::LitWord0;
                } else {
                    self.start_literal(self.acc)?;
                }
            }
            Mode::Opcode => self.begin_operands(byte),
            Mode::AfterCopy => {
                if byte >> 4 == 0 {
                    self.literal_opcode(byte)?;
                } else if byte >> 4 == 15 {
                    self.begin_operands(byte & 15);
                } else {
                    self.begin_operands(byte);
                }
            }
            Mode::Operands => {
                self.ins[usize::from(self.ins_have)] = byte;
                self.ins_have += 1;
                if self.ins_have == self.ins_need {
                    self.finish_operands(at)?;
                }
            }
            Mode::Block | Mode::Emit | Mode::Copy | Mode::Done => {
                return Err(StreamError::Finished);
            }
        }
        Ok(())
    }

    /// §5.10.2 `ReadInstructions`, field for field.
    fn finish_operands(&mut self, at: u64) -> Result<(), StreamError> {
        let op = self.op;
        let ins = self.ins.map(u64::from);
        let (length, offset, next) = match op >> 4 {
            0 => {
                let next = ins[1];
                let length = u64::from(op & 0x0f) + 0x13 + ((next >> 3) & 0x10);
                (length, ((next & 0x78) << 5) + 1 + ins[0], next as u8)
            }
            1 => {
                let next = ins[1];
                let length = u64::from(op & 0x0f) + 3;
                (length, ((next & 0xf8) << 5) + 1 + ins[0], next as u8)
            }
            2 => {
                let offset = (ins[1] << 8) | ins[0];
                let low = u64::from(op & 7);
                if op & 8 == 0 {
                    let next = ins[2];
                    (low + (next & 0xf8), offset, next as u8)
                } else {
                    let next = ins[3];
                    let length = (ins[2] << 3) + low + ((next & 0xf8) << 8) + 0x100;
                    (length, offset + 1, next as u8)
                }
            }
            _ => {
                let next = ins[0];
                (
                    u64::from(op >> 4),
                    ((next & 0xf8) << 1) + u64::from(op & 15) + 1,
                    next as u8,
                )
            }
        };
        if offset == 0 || offset > self.produced {
            return Err(StreamError::Lz77Offset {
                at,
                distance: offset as u32,
            });
        }
        if length > self.limit - self.produced {
            return Err(StreamError::Lz77Overrun { limit: self.limit });
        }
        self.op = next;
        self.ins = [0; 4];
        self.ins_have = 0;
        self.ins_need = 0;
        self.dist = offset as u32;
        self.count = length;
        self.mode = Mode::Copy;
        if length == 0 {
            self.after_copy()?;
        }
        Ok(())
    }

    /// Restore-time check that a decoded state is one `decode` can reach:
    /// fields a mode does not use are zero, counts fit the limit, a pending
    /// copy points inside the retained window.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.produced > self.limit {
            return false;
        }
        let room = self.limit - self.produced;
        let quiet = |d: &Self| {
            d.skip == 0
                && d.lo == 0
                && d.ins == [0; 4]
                && d.ins_have == 0
                && d.ins_need == 0
                && d.dist == 0
                && d.blk == [0; 32]
                && d.blk_have == 0
                && d.blk_need == 0
                && d.pend == [0; 32]
                && d.pend_len == 0
                && d.pend_pos == 0
        };
        match self.mode {
            Mode::Start => {
                quiet(self)
                    && self.op == 0
                    && self.count == 0
                    && self.acc == 0
                    && self.produced == 0
                    && self.consumed == 0
            }
            Mode::Skip => {
                (1..=2).contains(&self.skip)
                    && self.lo == 0
                    && self.ins == [0; 4]
                    && self.op == 0
                    && self.count == 0
                    && self.acc == 0
                    && self.dist == 0
                    && self.blk == [0; 32]
                    && self.pend == [0; 32]
                    && self.produced == 0
                    && self.consumed == 3 - u64::from(self.skip)
            }
            Mode::LenByte => {
                quiet(self)
                    && self.op == 0
                    && self.count == 0
                    && self.acc == 0
                    && self.produced == 0
                    && self.consumed == 3
            }
            Mode::LitOp | Mode::Opcode | Mode::AfterCopy | Mode::Done => {
                quiet(self)
                    && self.op == 0
                    && self.count == 0
                    && self.acc == 0
                    && self.consumed > 0
                    && (self.mode != Mode::Done || self.produced == self.limit)
            }
            Mode::LitExt => {
                quiet(self)
                    && self.op == 0
                    && self.count == 0
                    && self.acc == 0x17
                    && self.consumed > 0
            }
            Mode::LitWord0 | Mode::LitWord1 => {
                self.skip == 0
                    && self.ins == [0; 4]
                    && self.ins_have == 0
                    && self.ins_need == 0
                    && self.dist == 0
                    && self.blk == [0; 32]
                    && self.blk_have == 0
                    && self.blk_need == 0
                    && self.pend == [0; 32]
                    && self.pend_len == 0
                    && self.pend_pos == 0
                    && self.op == 0
                    && self.count == 0
                    && self.acc >= 0x17 + 0xff
                    && self.acc <= self.limit.saturating_add(0xffff)
                    && (self.mode == Mode::LitWord1 || self.lo == 0)
                    && self.consumed > 0
            }
            Mode::Block => {
                self.skip == 0
                    && self.lo == 0
                    && self.ins == [0; 4]
                    && self.ins_have == 0
                    && self.ins_need == 0
                    && self.dist == 0
                    && self.pend == [0; 32]
                    && self.pend_len == 0
                    && self.pend_pos == 0
                    && self.op == 0
                    && self.acc == 0
                    && self.count >= 1
                    && self.count <= room
                    && u64::from(self.blk_need) == self.count.min(32)
                    && self.blk_have < self.blk_need
                    && self.blk[usize::from(self.blk_have)..]
                        == [0; 32][usize::from(self.blk_have)..]
                    && self.consumed > 0
            }
            Mode::Emit => {
                self.skip == 0
                    && self.lo == 0
                    && self.ins == [0; 4]
                    && self.ins_have == 0
                    && self.ins_need == 0
                    && self.dist == 0
                    && self.blk == [0; 32]
                    && self.blk_have == 0
                    && self.blk_need == 0
                    && self.op == 0
                    && self.acc == 0
                    && (1..=32).contains(&self.pend_len)
                    && self.pend_pos < self.pend_len
                    && self.pend[usize::from(self.pend_len)..]
                        == [0; 32][usize::from(self.pend_len)..]
                    && self.count <= room.saturating_sub(u64::from(self.pend_len - self.pend_pos))
                    && room >= u64::from(self.pend_len - self.pend_pos)
                    && self.consumed > 0
            }
            Mode::Operands => {
                let need = match self.op >> 4 {
                    0 | 1 => 2,
                    2 if self.op & 8 == 0 => 3,
                    2 => 4,
                    _ => 1,
                };
                self.skip == 0
                    && self.lo == 0
                    && self.ins_need == need
                    && self.ins_have < self.ins_need
                    && self.ins[usize::from(self.ins_have)..]
                        == [0; 4][usize::from(self.ins_have)..]
                    && self.dist == 0
                    && self.blk == [0; 32]
                    && self.blk_have == 0
                    && self.blk_need == 0
                    && self.pend == [0; 32]
                    && self.pend_len == 0
                    && self.pend_pos == 0
                    && self.count == 0
                    && self.acc == 0
                    && self.consumed > 0
            }
            Mode::Copy => {
                self.skip == 0
                    && self.lo == 0
                    && self.ins == [0; 4]
                    && self.ins_have == 0
                    && self.ins_need == 0
                    && self.blk == [0; 32]
                    && self.blk_have == 0
                    && self.blk_need == 0
                    && self.pend == [0; 32]
                    && self.pend_len == 0
                    && self.pend_pos == 0
                    && self.acc == 0
                    && self.count >= 1
                    && self.count <= room
                    && self.dist >= 1
                    && self.dist as usize <= WINDOW
                    && u64::from(self.dist) <= self.produced
                    && self.consumed > 0
            }
        }
    }
}
