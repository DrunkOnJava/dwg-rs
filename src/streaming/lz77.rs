//! Bounded, resumable decoder for the R2004-family LZ77 dialect (ODA
//! v5.4.1 §4.7). It performs no I/O and holds no allocation: the caller
//! hands it any prefix of the compressed bytes and an output slice of any
//! size, and it returns how much of each it used. A decoder can stop
//! between any two calls, so a page never has to be held whole.
//!
//! Opcode semantics are the crate's own `lz77::decompress`, which was
//! checked against AutoCAD-written files: back-reference distances are one
//! more than §4.7's text gives (`+0x4000` for `0x10`–`0x1F`, `+1`
//! otherwise), and bit 3 of an `0x12`–`0x1F` opcode is bit 14 of the
//! distance. The largest distance is therefore `0xBFFF`, so the window
//! keeps the last [`WINDOW`] output bytes and no more, however long the
//! stream.

use super::StreamError;

/// Output bytes retained for back-references: the largest encodable
/// distance (`0xBFFF`) plus one.
pub const WINDOW: usize = 0xC000;
const MAX_DISTANCE: u32 = 0xBFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Mode {
    /// First byte: a literal length, or an opcode when its high nibble is set.
    Start = 0,
    /// A run of `0x00` extension bytes (literal length, count or long count).
    Ext = 1,
    /// `count` literal bytes still to copy from input to output.
    Literal = 2,
    Opcode = 3,
    /// Two-byte offset, first byte.
    Offset1 = 4,
    /// Two-byte offset, second byte; `first` holds the first.
    Offset2 = 5,
    /// Second byte of an `0x40`–`0xFF` opcode.
    Op2 = 6,
    /// Literal length after a back-reference, or the next opcode.
    LitLen = 7,
    /// `count` back-reference bytes still to emit from `distance` back.
    Copy = 8,
    Done = 9,
}

/// What a `0x00` extension run is counting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Ext {
    None = 0,
    /// Literal length: the leading `0x00` contributes 0x0F, then +3.
    Literal = 1,
    /// Compressed-byte count of `0x12`–`0x3F`: + opcode mask bits + 2.
    Count = 2,
    /// Long compression count of `0x10` (+9) and `0x20` (+0x21).
    Long = 3,
}

/// Bytes consumed and produced by one [`Lz77Decoder::decode`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub consumed: usize,
    pub produced: usize,
    /// The `0x11` terminator has been read; no further input is taken.
    pub done: bool,
}

/// Fixed-size §4.7 decoder state: the parse position inside the current
/// opcode, the pending copy or literal run, the totals and the window.
/// `limit` is the declared decompressed size; output beyond it is an error,
/// never an allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lz77Decoder {
    pub(super) mode: Mode,
    pub(super) ext: Ext,
    pub(super) op1: u8,
    pub(super) first: u8,
    pub(super) acc: u64,
    pub(super) count: u64,
    pub(super) lit: u8,
    pub(super) distance: u32,
    pub(super) produced: u64,
    pub(super) consumed: u64,
    pub(super) limit: u64,
    pub(super) failure: Option<StreamError>,
    pub(super) window: [u8; WINDOW],
}

impl Lz77Decoder {
    /// Start a stream whose output may not exceed `limit` bytes.
    pub fn new(limit: u64) -> Self {
        Self {
            mode: Mode::Start,
            ext: Ext::None,
            op1: 0,
            first: 0,
            acc: 0,
            count: 0,
            lit: 0,
            distance: 0,
            produced: 0,
            consumed: 0,
            limit,
            failure: None,
            window: [0; WINDOW],
        }
    }

    /// Total output bytes so far.
    pub fn produced(&self) -> u64 {
        self.produced
    }

    /// Total input bytes consumed so far.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Declared output ceiling.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// The terminator has been read.
    pub fn is_done(&self) -> bool {
        self.mode == Mode::Done
    }

    /// Whether more output can be produced without more input (a pending
    /// back-reference copy).
    pub fn has_pending_output(&self) -> bool {
        self.mode == Mode::Copy
    }

    /// Decode from `input` into `out` until the input is used up, `out` is
    /// full or the terminator is read. A grammar error is terminal; output
    /// already returned stays provisional until the caller's commit point.
    pub fn decode(&mut self, input: &[u8], out: &mut [u8]) -> Result<Progress, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let (mut i, mut o) = (0usize, 0usize);
        loop {
            match self.mode {
                Mode::Done => break,
                Mode::Copy => {
                    if o == out.len() {
                        break;
                    }
                    let n = (out.len() - o).min(usize::try_from(self.count).unwrap_or(usize::MAX));
                    for slot in &mut out[o..o + n] {
                        let at = (self.produced - u64::from(self.distance)) % WINDOW as u64;
                        let byte = self.window[at as usize];
                        self.push(byte);
                        *slot = byte;
                    }
                    o += n;
                    self.count -= n as u64;
                    if self.count == 0 {
                        self.distance = 0;
                        if self.lit == 0 {
                            self.mode = Mode::LitLen;
                        } else {
                            self.count = u64::from(self.lit);
                            self.lit = 0;
                            self.mode = Mode::Literal;
                        }
                    }
                }
                Mode::Literal => {
                    if o == out.len() || i == input.len() {
                        break;
                    }
                    let n = (out.len() - o)
                        .min(input.len() - i)
                        .min(usize::try_from(self.count).unwrap_or(usize::MAX));
                    // `produced` cannot overflow: restore and every transition
                    // keep `count <= limit - produced`. `consumed` has no such
                    // bound, so it is checked before anything moves.
                    let Some(consumed) = self.consumed.checked_add(n as u64) else {
                        self.failure = Some(StreamError::CounterOverflow);
                        return Err(StreamError::CounterOverflow);
                    };
                    for (slot, &byte) in out[o..o + n].iter_mut().zip(&input[i..i + n]) {
                        self.push(byte);
                        *slot = byte;
                    }
                    i += n;
                    o += n;
                    self.consumed = consumed;
                    self.count -= n as u64;
                    if self.count == 0 {
                        self.mode = Mode::Opcode;
                    }
                }
                _ => {
                    if i == input.len() {
                        break;
                    }
                    let byte = input[i];
                    let at = self.consumed;
                    let Some(consumed) = at.checked_add(1) else {
                        self.failure = Some(StreamError::CounterOverflow);
                        return Err(StreamError::CounterOverflow);
                    };
                    i += 1;
                    self.consumed = consumed;
                    if let Err(error) = self.step(byte, at) {
                        self.failure = Some(error);
                        return Err(error);
                    }
                }
            }
        }
        Ok(Progress {
            consumed: i,
            produced: o,
            done: self.mode == Mode::Done,
        })
    }

    /// Reject the end of the compressed bytes before the terminator.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.mode == Mode::Done {
            Ok(())
        } else {
            Err(StreamError::Lz77Truncated {
                consumed: self.consumed,
            })
        }
    }

    fn push(&mut self, byte: u8) {
        self.window[(self.produced % WINDOW as u64) as usize] = byte;
        self.produced += 1;
    }

    fn step(&mut self, byte: u8, at: u64) -> Result<(), StreamError> {
        match self.mode {
            Mode::Start | Mode::LitLen => {
                if byte == 0 {
                    self.ext = Ext::Literal;
                    self.acc = 0x0f;
                    self.mode = Mode::Ext;
                } else if byte & 0xf0 != 0 {
                    self.opcode(byte, at)?;
                } else {
                    self.literal(u64::from(byte) + 3)?;
                }
            }
            Mode::Ext => {
                if byte == 0 {
                    self.acc = self.acc.saturating_add(0xff);
                    // Every count must fit the declared output; stop a
                    // runaway extension before it can overflow.
                    if self.acc > self.limit {
                        return Err(StreamError::Lz77Overrun { limit: self.limit });
                    }
                    return Ok(());
                }
                let total = self.acc.saturating_add(u64::from(byte));
                let ext = self.ext;
                self.ext = Ext::None;
                self.acc = 0;
                match ext {
                    Ext::Literal => self.literal(total.saturating_add(3))?,
                    Ext::Count => {
                        // Only 0x18 reaches here: its low three bits are zero.
                        self.count = total.saturating_add(0x07 + 2);
                        self.mode = Mode::Offset1;
                    }
                    Ext::Long => {
                        let base = if self.op1 == 0x10 { 9 } else { 0x21 };
                        self.count = total.saturating_add(base);
                        self.mode = Mode::Offset1;
                    }
                    Ext::None => return Err(StreamError::Finished),
                }
            }
            Mode::Opcode => self.opcode(byte, at)?,
            Mode::Offset1 => {
                self.first = byte;
                self.mode = Mode::Offset2;
            }
            Mode::Offset2 => {
                let base = u32::from(self.first >> 2) | (u32::from(byte) << 6);
                let lit = self.first & 0x03;
                self.first = 0;
                let distance = match self.op1 {
                    0x10 => base + 0x4000,
                    0x12..=0x1f => ((u32::from(self.op1 & 0x08) << 11) | base) + 0x4000,
                    _ => base + 1,
                };
                self.copy(distance, lit, at)?;
            }
            Mode::Op2 => {
                let distance = ((u32::from(byte) << 2) | u32::from((self.op1 & 0x0c) >> 2)) + 1;
                self.copy(distance, self.op1 & 0x03, at)?;
            }
            Mode::Literal | Mode::Copy | Mode::Done => return Err(StreamError::Finished),
        }
        Ok(())
    }

    fn opcode(&mut self, op1: u8, at: u64) -> Result<(), StreamError> {
        self.op1 = op1;
        match op1 {
            0x00..=0x0f => return Err(StreamError::Lz77Opcode { at, opcode: op1 }),
            0x11 => {
                self.op1 = 0;
                self.mode = Mode::Done;
            }
            0x10 | 0x20 => {
                self.ext = Ext::Long;
                self.mode = Mode::Ext;
                // A non-zero first byte is the count itself; the shared
                // extension loop treats it as a zero-length run.
            }
            0x12..=0x3f => {
                let mask = if op1 < 0x20 { 0x07 } else { 0x1f };
                if op1 & mask == 0 {
                    self.ext = Ext::Count;
                    self.mode = Mode::Ext;
                } else {
                    self.count = u64::from(op1 & mask) + 2;
                    self.mode = Mode::Offset1;
                }
            }
            0x40..=0xff => {
                self.count = u64::from(op1 >> 4) - 1;
                self.mode = Mode::Op2;
            }
        }
        Ok(())
    }

    fn literal(&mut self, count: u64) -> Result<(), StreamError> {
        if count > self.limit - self.produced {
            return Err(StreamError::Lz77Overrun { limit: self.limit });
        }
        self.count = count;
        self.mode = Mode::Literal;
        Ok(())
    }

    fn copy(&mut self, distance: u32, lit: u8, at: u64) -> Result<(), StreamError> {
        if u64::from(distance) > self.produced {
            return Err(StreamError::Lz77Offset { at, distance });
        }
        if self.count.saturating_add(u64::from(lit)) > self.limit - self.produced {
            return Err(StreamError::Lz77Overrun { limit: self.limit });
        }
        self.op1 = 0;
        self.distance = distance;
        self.lit = lit;
        self.mode = Mode::Copy;
        Ok(())
    }

    /// Restore-time check that a decoded state is one `decode` can reach:
    /// fields a mode does not use are zero, counts fit the limit, and a
    /// pending copy points inside the retained window.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.produced > self.limit {
            return false;
        }
        let room = self.limit - self.produced;
        let quiet = |d: &Self| d.ext == Ext::None && d.acc == 0 && d.first == 0;
        match self.mode {
            Mode::Start => {
                self.produced == 0
                    && self.consumed == 0
                    && quiet(self)
                    && self.op1 == 0
                    && self.count == 0
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Opcode | Mode::LitLen | Mode::Done => {
                self.consumed > 0
                    && quiet(self)
                    && self.op1 == 0
                    && self.count == 0
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Ext => {
                let op_ok = match self.ext {
                    Ext::Literal => self.op1 == 0 && self.acc >= 0x0f,
                    Ext::Count => self.op1 == 0x18,
                    Ext::Long => matches!(self.op1, 0x10 | 0x20),
                    Ext::None => false,
                };
                op_ok
                    && self.consumed > 0
                    && self.acc <= self.limit
                    && (self.ext == Ext::Literal || (self.acc % 0xff == 0))
                    && (self.ext != Ext::Literal || (self.acc - 0x0f) % 0xff == 0)
                    && self.first == 0
                    && self.count == 0
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Literal => {
                self.consumed > 0
                    && quiet(self)
                    && self.op1 == 0
                    && self.count >= 1
                    && self.count <= room
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Offset1 | Mode::Offset2 => {
                matches!(self.op1, 0x10 | 0x12..=0x3f)
                    && self.consumed > 0
                    && self.ext == Ext::None
                    && self.acc == 0
                    && (self.mode == Mode::Offset2 || self.first == 0)
                    && self.count >= 3
                    && self.count <= self.limit.saturating_add(0x200)
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Op2 => {
                self.op1 >= 0x40
                    && self.consumed > 0
                    && quiet(self)
                    && self.count == u64::from(self.op1 >> 4) - 1
                    && self.lit == 0
                    && self.distance == 0
            }
            Mode::Copy => {
                self.consumed > 0
                    && quiet(self)
                    && self.op1 == 0
                    && self.count >= 1
                    && self.count.saturating_add(u64::from(self.lit)) <= room
                    && self.lit <= 3
                    && self.distance >= 1
                    && self.distance <= MAX_DISTANCE
                    && u64::from(self.distance) <= self.produced
            }
        }
    }
}
