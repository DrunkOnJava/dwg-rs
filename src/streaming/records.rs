//! The records of an object section (ODA v5.4.1 §20.1) framed from a byte
//! stream, with fixed state. A record is `MS` size, on R2010 and later an
//! `MC` handle-stream size that the `MS` does not count, `MS` bytes of
//! payload and a CRC of the record's bytes from the `MS` through the last
//! payload byte (§2.14.1, seed 0xC0C1). Measured on every object section of
//! the pinned corpus (8,007 records of 28 drawings, ARCHITECTURE.md §7aa) the
//! records tile the section exactly after its four-byte prologue and every
//! CRC and every `MC` agree. The framer takes the stream's bytes in order,
//! in slices of any size; payload is provisional until the step that
//! commits it, which comes after the CRC, the type and the handle check.

use super::StreamError;
use super::flat::ObjectRecord;
use crate::crc::crc8;

const SEED: u16 = 0xc0c1;
/// The prologue every R2004-family object section opens with: an `RL` of
/// 0x0DCA.
const PROLOGUE: [u8; 4] = [0xca, 0x0d, 0x00, 0x00];
/// Payload bytes kept to read the type and the handle from: an `MC` of at
/// most ten bytes, a type of at most eighteen bits, a 32-bit size and a
/// handle of at most nine bytes fit in 32.
pub(super) const HEAD: usize = 32;
const MAX_MC: u8 = 10;
/// The longest section a stream is taken to have: every position is then far
/// below `u64::MAX`, so no addition of a record's few bytes can overflow, even
/// from a forged checkpoint.
const MAX_STREAM: u64 = 1 << 62;
/// The fewest bytes a record can take: an `MS`, one payload byte and the CRC.
const MIN_RECORD: u64 = 5;

/// How a record's payload opens, by release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordFormat {
    /// R13 and R14: `BS` type, then the handle.
    R14 = 0,
    /// R2000 to R2007: `BS` type, an `RL` size in bits, then the handle.
    R2000 = 1,
    /// R2010 and later: an `MC` handle-stream size, a two-bit-tagged type,
    /// then the handle.
    R2010 = 2,
}

impl RecordFormat {
    pub(super) fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::R14),
            1 => Some(Self::R2000),
            2 => Some(Self::R2010),
            _ => None,
        }
    }
}

/// The object type and the handle of a record, read MSB-first from the
/// first payload bytes after `skip` (the `MC`, on R2010 and later).
pub(super) fn prologue(head: &[u8], format: RecordFormat, skip: usize) -> Option<(u16, u64)> {
    let mut at = skip * 8;
    let mut take = |n: usize| -> Option<u64> {
        let mut value = 0u64;
        for _ in 0..n {
            let byte = *head.get(at / 8)?;
            value = (value << 1) | u64::from((byte >> (7 - at % 8)) & 1);
            at += 1;
        }
        Some(value)
    };
    let type_code = if format == RecordFormat::R2010 {
        match take(2)? {
            0 => take(8)? as u16,
            1 => take(8)? as u16 + 0x1f0,
            _ => {
                let low = take(8)?;
                (take(8)? << 8 | low) as u16
            }
        }
    } else {
        match take(2)? {
            0 => {
                let low = take(8)?;
                (take(8)? << 8 | low) as u16
            }
            1 => take(8)? as u16,
            2 => 0,
            _ => 256,
        }
    };
    if format == RecordFormat::R2000 {
        take(32)?;
    }
    take(4)?;
    let counter = take(4)?;
    if counter > 8 {
        return None;
    }
    let mut handle = 0u64;
    for _ in 0..counter {
        handle = handle << 8 | take(8)?;
    }
    Some((type_code, handle))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Frame {
    Prologue = 0,
    Ms = 1,
    Body = 2,
    Crc = 3,
    /// Every byte of the section was consumed, on a record boundary; the
    /// walker has not yet said so.
    Done = 4,
    Ended = 5,
}

/// What one `feed` consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Event {
    Framing,
    /// Payload bytes of the open record, provisional.
    Payload,
    Object(ObjectRecord),
}

/// Fixed-size state of one object stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectFramer {
    pub(super) format: RecordFormat,
    pub(super) total: u64,
    pub(super) at: u64,
    pub(super) stage: Frame,
    pub(super) start: u64,
    pub(super) size: u32,
    pub(super) words: u8,
    pub(super) ms: [u8; 4],
    pub(super) ms_len: u8,
    pub(super) mc: u64,
    pub(super) mc_len: u8,
    pub(super) mc_done: bool,
    pub(super) done: u32,
    pub(super) crc: u16,
    pub(super) field: [u8; 2],
    pub(super) field_len: u8,
    pub(super) head: [u8; HEAD],
    pub(super) records: u64,
}

impl ObjectFramer {
    /// A framer that has not been told its section yet.
    pub(super) fn idle() -> Self {
        Self {
            format: RecordFormat::R2010,
            total: 0,
            at: 0,
            stage: Frame::Prologue,
            start: 0,
            size: 0,
            words: 0,
            ms: [0; 4],
            ms_len: 0,
            mc: 0,
            mc_len: 0,
            mc_done: false,
            done: 0,
            crc: 0,
            field: [0; 2],
            field_len: 0,
            head: [0; HEAD],
            records: 0,
        }
    }

    /// Begin a section of `total` bytes. A section too short for its
    /// prologue is refused.
    pub(super) fn begin(format: RecordFormat, total: u64) -> Result<Self, StreamError> {
        if total < PROLOGUE.len() as u64 {
            return Err(StreamError::ObjectPrologue);
        }
        if total > MAX_STREAM {
            return Err(StreamError::CounterOverflow);
        }
        Ok(Self {
            format,
            total,
            ..Self::idle()
        })
    }

    fn payload(&self) -> u64 {
        if self.format == RecordFormat::R2010 {
            u64::from(self.mc_len) + u64::from(self.size)
        } else {
            u64::from(self.size)
        }
    }

    fn record_end(&self) -> u64 {
        self.start + u64::from(self.ms_len) + self.payload() + 2
    }

    /// Bytes the next transition needs at least, and at most when the
    /// stage's length is known. Zero once the section is consumed.
    pub(super) fn needs(&self) -> u64 {
        match self.stage {
            Frame::Prologue => PROLOGUE.len() as u64 - self.at,
            Frame::Ms => 2 - u64::from(self.field_len),
            Frame::Body => {
                if self.format == RecordFormat::R2010 && !self.mc_done {
                    1
                } else {
                    self.payload() - u64::from(self.done)
                }
            }
            Frame::Crc => 2 - u64::from(self.field_len),
            Frame::Done | Frame::Ended => 0,
        }
    }

    /// Consume the longest prefix of `input` the current stage can use.
    /// Returns how many bytes it took and what they were.
    pub(super) fn feed(&mut self, input: &[u8]) -> Result<(usize, Event), StreamError> {
        let start = self.start;
        match self.stage {
            Frame::Prologue => {
                let d = self.at as usize;
                let n = input.len().min(self.needs() as usize);
                if input[..n] != PROLOGUE[d..d + n] {
                    return Err(StreamError::ObjectPrologue);
                }
                self.at += n as u64;
                if self.at == PROLOGUE.len() as u64 {
                    self.next_record();
                }
                Ok((n, Event::Framing))
            }
            Frame::Ms => {
                let n = input.len().min(self.needs() as usize);
                let l = usize::from(self.field_len);
                self.field[l..l + n].copy_from_slice(&input[..n]);
                let m = usize::from(self.ms_len);
                self.ms[m..m + n].copy_from_slice(&input[..n]);
                self.ms_len += n as u8;
                self.field_len += n as u8;
                self.crc = crc8(self.crc, &input[..n]);
                self.at += n as u64;
                if self.field_len < 2 {
                    return Ok((n, Event::Framing));
                }
                self.field_len = 0;
                let word = u16::from_le_bytes(self.field);
                self.field = [0; 2];
                self.size |= u32::from(word & 0x7fff) << (15 * self.words);
                self.words += 1;
                if word & 0x8000 != 0 {
                    if self.words == 2 {
                        return Err(StreamError::RecordSize(start));
                    }
                    return Ok((n, Event::Framing));
                }
                if self.size == 0 {
                    return Err(StreamError::RecordSize(start));
                }
                self.stage = Frame::Body;
                if self.format != RecordFormat::R2010 {
                    self.check_end()?;
                } else if start + u64::from(self.ms_len) + u64::from(self.size) + 3 > self.total {
                    return Err(StreamError::RecordRange(start));
                }
                Ok((n, Event::Framing))
            }
            Frame::Body => {
                let n = if self.format == RecordFormat::R2010 && !self.mc_done {
                    let mut n = 0;
                    while n < input.len() && !self.mc_done {
                        let byte = input[n];
                        if self.mc_len >= MAX_MC || (self.mc_len == MAX_MC - 1 && byte & 0x7e != 0)
                        {
                            return Err(StreamError::RecordHeader(start));
                        }
                        self.mc |= u64::from(byte & 0x7f) << (7 * u32::from(self.mc_len));
                        self.mc_len += 1;
                        self.mc_done = byte & 0x80 == 0;
                        self.keep(byte);
                        n += 1;
                    }
                    if self.mc_done {
                        self.check_end()?;
                        if self.mc > 8 * self.payload() {
                            return Err(StreamError::RecordHeader(start));
                        }
                    }
                    n
                } else {
                    let n = input.len().min(self.needs() as usize);
                    for &byte in &input[..n] {
                        self.keep(byte);
                    }
                    n
                };
                self.crc = crc8(self.crc, &input[..n]);
                self.at += n as u64;
                let known = self.mc_done || self.format != RecordFormat::R2010;
                if known && u64::from(self.done) == self.payload() {
                    self.stage = Frame::Crc;
                    self.field_len = 0;
                }
                Ok((n, Event::Payload))
            }
            Frame::Crc => {
                let l = usize::from(self.field_len);
                let n = input.len().min(self.needs() as usize);
                self.field[l..l + n].copy_from_slice(&input[..n]);
                self.field_len += n as u8;
                self.at += n as u64;
                if self.field_len < 2 {
                    return Ok((n, Event::Framing));
                }
                let actual = u16::from_le_bytes(self.field);
                if actual != self.crc {
                    return Err(StreamError::RecordCrc {
                        offset: start,
                        expected: self.crc,
                        actual,
                    });
                }
                let skip = if self.format == RecordFormat::R2010 {
                    usize::from(self.mc_len)
                } else {
                    0
                };
                let len = (self.payload() as usize).min(HEAD);
                let (type_code, handle) = prologue(&self.head[..len], self.format, skip)
                    .ok_or(StreamError::RecordHeader(start))?;
                let record = ObjectRecord {
                    handle,
                    offset: start,
                    size: self.size,
                    type_code,
                };
                self.records += 1;
                self.next_record();
                Ok((n, Event::Object(record)))
            }
            Frame::Done | Frame::Ended => Err(StreamError::ObjectsOrder {
                expected: self.total,
                actual: self.at,
            }),
        }
    }

    fn keep(&mut self, byte: u8) {
        let i = self.done as usize;
        if i < HEAD {
            self.head[i] = byte;
        }
        self.done += 1;
    }

    fn check_end(&self) -> Result<(), StreamError> {
        if self.record_end() > self.total {
            return Err(StreamError::RecordRange(self.start));
        }
        Ok(())
    }

    /// The bytes consumed are a whole number of records (or just the
    /// prologue): open the next, or finish when the section is consumed.
    fn next_record(&mut self) {
        let (format, total, at, records) = (self.format, self.total, self.at, self.records);
        *self = Self {
            format,
            total,
            at,
            records,
            start: at,
            crc: SEED,
            stage: if at == total { Frame::Done } else { Frame::Ms },
            ..Self::idle()
        };
    }

    /// The `MS` value held in `ms`, when it is a whole, well-formed field:
    /// one word without the continuation flag, or two with it on the first.
    fn ms_value(&self) -> Option<u32> {
        let word = |i: usize| u16::from_le_bytes([self.ms[2 * i], self.ms[2 * i + 1]]);
        match self.ms_len {
            2 if word(0) & 0x8000 == 0 => Some(u32::from(word(0))),
            4 if word(0) & 0x8000 != 0 && word(1) & 0x8000 == 0 => {
                Some(u32::from(word(0) & 0x7fff) | u32::from(word(1)) << 15)
            }
            _ => None,
        }
    }

    /// The `MC` the bytes held in `head` spell, and whether they end it.
    fn mc_held(&self) -> Option<(u64, bool)> {
        let held = &self.head[..usize::from(self.mc_len)];
        let mut value = 0u64;
        for (i, &byte) in held.iter().enumerate() {
            if (byte & 0x80 == 0 && i + 1 != held.len()) || (i == 9 && byte & 0x7e != 0) {
                return None;
            }
            value |= u64::from(byte & 0x7f) << (7 * i as u32);
        }
        let ended = held.last().is_some_and(|&b| b & 0x80 == 0);
        Some((value, ended))
    }

    /// Restore-time check that a decoded state is one the stream can reach.
    /// The CRC is recomputed wherever the bytes it covers are still held.
    pub(super) fn consistent(&self) -> bool {
        if self.total == 0 {
            return *self == Self::idle();
        }
        let prologue = PROLOGUE.len() as u64;
        if self.total < prologue
            || self.total > MAX_STREAM
            || self.at > self.total
            || self.start > self.at
        {
            return false;
        }
        let held = self.done.min(HEAD as u32) as usize;
        let rest_blank = self.head[held..].iter().all(|&b| b == 0);
        let no_record = self.size == 0
            && self.words == 0
            && self.ms == [0; 4]
            && self.ms_len == 0
            && self.mc == 0
            && self.mc_len == 0
            && !self.mc_done
            && self.done == 0
            && self.field == [0; 2]
            && self.field_len == 0
            && self.head == [0; HEAD];
        let first = self.records == 0;
        match self.stage {
            Frame::Prologue => {
                self.at < prologue && self.start == 0 && self.crc == 0 && first && no_record
            }
            Frame::Done | Frame::Ended => {
                self.at == self.total
                    && self.start == self.at
                    && self.crc == SEED
                    && first == (self.at == prologue)
                    && self.records <= (self.at - prologue) / MIN_RECORD
                    && no_record
            }
            Frame::Ms => {
                let ms_len = usize::from(self.ms_len);
                self.start >= prologue
                    && (!first || self.start == prologue)
                    && self.records <= (self.start - prologue) / MIN_RECORD
                    && self.at == self.start + u64::from(self.ms_len)
                    && self.at < self.total
                    && self.words <= 1
                    && self.field_len <= 1
                    && self.ms_len == 2 * self.words + self.field_len
                    && self.ms[ms_len..].iter().all(|&b| b == 0)
                    && self.crc == crc8(SEED, &self.ms[..ms_len])
                    && if self.field_len == 1 {
                        self.field == [self.ms[ms_len - 1], 0]
                    } else {
                        self.field == [0; 2]
                    }
                    && if self.words == 1 {
                        let word = u16::from_le_bytes([self.ms[0], self.ms[1]]);
                        word & 0x8000 != 0 && self.size == u32::from(word & 0x7fff)
                    } else {
                        self.size == 0
                    }
                    && self.mc == 0
                    && self.mc_len == 0
                    && !self.mc_done
                    && self.done == 0
                    && self.head == [0; HEAD]
            }
            Frame::Body | Frame::Crc => {
                let Some(size) = self.ms_value() else {
                    return false;
                };
                let r2010 = self.format == RecordFormat::R2010;
                let payload = self.payload();
                let crc_pending = self.stage == Frame::Crc;
                let mc_ok = if r2010 {
                    self.mc_len <= MAX_MC
                        && self.mc_held().is_some_and(|(value, ended)| {
                            value == self.mc
                                && ended == self.mc_done
                                && (!self.mc_done || value <= 8 * payload)
                        })
                        && (self.mc_done
                            || (self.mc_len < MAX_MC && self.done == u32::from(self.mc_len)))
                        && (!self.mc_done
                            || (self.mc_len >= 1 && self.done >= u32::from(self.mc_len)))
                } else {
                    self.mc == 0 && self.mc_len == 0 && !self.mc_done
                };
                let bytes_ok = if crc_pending {
                    u64::from(self.done) == payload
                        && self.field_len <= 1
                        && if self.field_len == 1 {
                            self.field[1] == 0
                        } else {
                            self.field == [0; 2]
                        }
                } else {
                    u64::from(self.done) < payload && self.field_len == 0 && self.field == [0; 2]
                };
                let covered = if self.done as usize <= HEAD {
                    let ms_crc = crc8(SEED, &self.ms[..usize::from(self.ms_len)]);
                    self.crc == crc8(ms_crc, &self.head[..self.done as usize])
                } else {
                    true
                };
                let progress =
                    u64::from(self.ms_len) + u64::from(self.done) + u64::from(self.field_len);
                size == self.size
                    && size != 0
                    && self.words == self.ms_len / 2
                    && self.start >= prologue
                    && (!first || self.start == prologue)
                    && self.records <= (self.start - prologue) / MIN_RECORD
                    && self.at == self.start + progress
                    && (!r2010
                        || self.mc_done
                        || self.start + u64::from(self.ms_len) + u64::from(size) + 3 <= self.total)
                    && (r2010 && !self.mc_done || self.record_end() <= self.total)
                    && mc_ok
                    && bytes_ok
                    && covered
                    && rest_blank
            }
        }
    }
}
