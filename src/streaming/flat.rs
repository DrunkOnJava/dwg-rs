//! R13–R2000 container walk: the file header (§3.2), the header-variable
//! and class sections (§9, §10.1), then the object map (§23.1), visiting
//! each mapped record (§20.1). Index bytes are verified before anything is
//! derived from them; payload is provisional until the step that commits
//! it (docs/streaming/audit.md, decision 7). Locators 3–5, the second
//! header and image data are not walked.

use super::{FlatHeader, FlatHeaderParser, Locator, Phase, Request, SourceIdentity, StreamError};
use crate::crc::crc8;

const SEED: u16 = 0xc0c1;
const FRAME: u64 = 38;
/// §23.1 cuts a chunk at 2032 bytes. The count is the entries: the size field also
/// counts its own two bytes, and real AC1012, AC1014 and AC1015 drawings reach 2034.
const MAX_MAP_CHUNK: u32 = 2034;
pub(super) const HEAD: usize = 16;
/// Start and end sentinels of the §9 header-variable section.
pub const HEADER_VARS_SENTINELS: [[u8; 16]; 2] = [
    [
        0xcf, 0x7b, 0x1f, 0x23, 0xfd, 0xde, 0x38, 0xa9, 0x5f, 0x7c, 0x68, 0xb8, 0x4e, 0x6d, 0x33,
        0x5f,
    ],
    [
        0x30, 0x84, 0xe0, 0xdc, 0x02, 0x21, 0xc7, 0x56, 0xa0, 0x83, 0x97, 0x47, 0xb1, 0x92, 0xcc,
        0xa0,
    ],
];
/// Start and end sentinels of the §10.1 class section.
pub const CLASS_SENTINELS: [[u8; 16]; 2] = [
    [
        0x8d, 0xa1, 0xc4, 0xb8, 0xc4, 0xa9, 0xf8, 0xc5, 0xc0, 0xdc, 0xf4, 0x5f, 0xe7, 0xcf, 0xb6,
        0x8a,
    ],
    [
        0x72, 0x5e, 0x3b, 0x47, 0x3b, 0x56, 0x07, 0x3a, 0x3f, 0x23, 0x0b, 0xa0, 0x18, 0x30, 0x49,
        0x75,
    ],
];

/// Owner of provisional payload bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Part {
    HeaderVars = 0,
    Classes = 1,
    Object = 2,
}

/// A committed §9 or §10.1 section: its data passed size, CRC and sentinels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionRecord {
    pub part: Part,
    pub data_offset: u64,
    pub size: u32,
}

/// A committed §20.1 record: CRC, prologue and map handle all agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRecord {
    pub handle: u64,
    pub offset: u64,
    pub size: u32,
    pub type_code: u16,
}

/// What the bytes of one feed were. `Payload` bytes stay with the caller
/// and are uncommitted until the matching `Section` or `Object` step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Framing,
    Payload(Part),
    Header(FlatHeader),
    Section(SectionRecord),
    Object(ObjectRecord),
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Stage {
    Header,
    Vars,
    Classes,
    Chunk,
    Entry,
    Object,
    Done,
}

/// Object-map position kept while records are visited.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct MapCursor {
    pub(super) chunk: u64,
    pub(super) len: u32,
    pub(super) at: u64,
    pub(super) handle: u64,
    pub(super) loc: u64,
    pub(super) mc: u64,
    pub(super) shift: u8,
    pub(super) field: u8,
}

/// Pure suspendable walk. Like [`FlatHeaderParser`], it performs no I/O:
/// `request` names the exact next demand and `feed` takes a nonempty
/// prefix of it. Requests jump to absolute offsets, so the source must be
/// range-capable (a forward-only stream needs a spool).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatWalker {
    pub(super) header: FlatHeaderParser,
    pub(super) stage: Stage,
    pub(super) done: u32,
    pub(super) size: u32,
    pub(super) crc: u16,
    pub(super) field: [u8; 4],
    pub(super) head: [u8; HEAD],
    pub(super) object: u64,
    pub(super) ms_len: u8,
    pub(super) words: u8,
    pub(super) map: MapCursor,
    pub(super) failure: Option<StreamError>,
}

impl FlatWalker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self::from_header(FlatHeaderParser::new(source))
    }

    pub(super) fn from_header(header: FlatHeaderParser) -> Self {
        Self {
            header,
            stage: Stage::Header,
            done: 0,
            size: 0,
            crc: 0,
            field: [0; 4],
            head: [0; HEAD],
            object: 0,
            ms_len: 0,
            words: 0,
            map: MapCursor::default(),
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.header.source
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<FlatHeader> {
        self.header.header()
    }

    /// Next exact byte demand; `None` once the final map chunk verified.
    pub fn request(&self) -> Result<Option<Request>, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let (offset, len) = match self.stage {
            Stage::Header => return self.header.request(),
            Stage::Vars | Stage::Classes => {
                let d = u64::from(self.done);
                let size = u64::from(self.size);
                let end = match d {
                    0..16 => 16,
                    16..20 => 20,
                    _ if d < 20 + size => 20 + size,
                    _ if d < 22 + size => 22 + size,
                    _ => FRAME + size,
                };
                (u64::from(self.section().offset) + d, end - d)
            }
            Stage::Chunk => {
                let d = self.done;
                let end = if d < 2 {
                    2
                } else if d < self.size {
                    self.size
                } else {
                    self.size + 2
                };
                (self.map.chunk + u64::from(d), u64::from(end - d))
            }
            Stage::Entry => (self.map.at, 1),
            Stage::Object => {
                let d = u32::from(self.ms_len);
                let end = if self.ms_len == 0 {
                    self.done - self.done % 2 + 2
                } else if self.done < d + self.size {
                    d + self.size
                } else {
                    d + self.size + 2
                };
                (
                    self.object + u64::from(self.done),
                    u64::from(end - self.done),
                )
            }
            Stage::Done => return Ok(None),
        };
        Ok(Some(Request {
            offset,
            len: len as usize,
        }))
    }

    /// Consume one nonempty prefix of the current demand. Invalid delivery
    /// leaves state unchanged; a grammar failure becomes a terminal error.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<Step, StreamError> {
        let request = self.request()?.ok_or(StreamError::Finished)?;
        if offset != request.offset {
            return Err(StreamError::WrongOffset {
                expected: request.offset,
                actual: offset,
            });
        }
        if input.is_empty() || input.len() > request.len {
            return Err(StreamError::InvalidFeed {
                maximum: request.len,
                actual: input.len(),
            });
        }
        let result = match self.stage {
            Stage::Header => self.feed_header(offset, input),
            Stage::Vars | Stage::Classes => self.feed_section(input),
            Stage::Chunk => self.feed_chunk(input),
            Stage::Entry => self.feed_entry(input[0]),
            Stage::Object => self.feed_object(input),
            Stage::Done => Err(StreamError::Finished),
        };
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    /// Reject EOF while the current transition still needs bytes.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.request()? {
            Some(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            None => Ok(()),
        }
    }

    fn top(&self) -> FlatHeader {
        self.header.header().unwrap_or(FlatHeader {
            magic: [0; 6],
            maintenance: 0,
            image_seeker: 0,
            codepage: 0,
            locator_count: 0,
            locators: [Locator::default(); 6],
        })
    }

    pub(super) fn locator(&self, number: u8) -> Locator {
        let top = self.top();
        top.locators[..usize::from(top.locator_count)]
            .iter()
            .copied()
            .find(|l| l.number == number)
            .unwrap_or_default()
    }

    fn section(&self) -> Locator {
        self.locator(if self.stage == Stage::Vars { 0 } else { 1 })
    }

    pub(super) fn header_end(&self) -> u64 {
        25 + u64::from(self.header.count) * 9 + 18
    }

    fn map_end(&self) -> u64 {
        let map = self.locator(2);
        u64::from(map.offset) + u64::from(map.size)
    }

    fn feed_header(&mut self, offset: u64, input: &[u8]) -> Result<Step, StreamError> {
        self.header.feed(offset, input)?;
        match self.header.header() {
            Some(top) => {
                let next = self.enter_section(Stage::Vars);
                Ok(self.then(Step::Header(top), next))
            }
            None => Ok(Step::Framing),
        }
    }

    /// A committed step stands even when the unit after it is malformed;
    /// that failure is reported by the next request.
    fn then(&mut self, step: Step, next: Result<(), StreamError>) -> Step {
        if let Err(error) = next {
            self.failure = Some(error);
        }
        step
    }

    fn enter_section(&mut self, stage: Stage) -> Result<(), StreamError> {
        self.stage = stage;
        self.done = 0;
        self.size = 0;
        if u64::from(self.section().size) < FRAME {
            return Err(StreamError::SectionSize(stage as u8 - 1));
        }
        Ok(())
    }

    fn feed_section(&mut self, input: &[u8]) -> Result<Step, StreamError> {
        let part = self.stage as u8 - 1;
        let d = self.done as usize;
        let size = self.size as usize;
        let n = input.len();
        self.done += n as u32;
        if d < 16 {
            if input != &sentinels(part)[0][d..d + n] {
                return Err(StreamError::SectionSentinel(part));
            }
            self.crc = SEED;
        } else if d < 20 {
            self.field[d - 16..d - 16 + n].copy_from_slice(input);
            self.crc = crc8(self.crc, input);
            if self.done == 20 {
                self.size = u32::from_le_bytes(self.field);
                if FRAME + u64::from(self.size) != u64::from(self.section().size) {
                    return Err(StreamError::SectionSize(part));
                }
            }
        } else if d < 20 + size {
            self.crc = crc8(self.crc, input);
            return Ok(Step::Payload(if part == 0 {
                Part::HeaderVars
            } else {
                Part::Classes
            }));
        } else if d < 22 + size {
            self.field[d - 20 - size..d - 20 - size + n].copy_from_slice(input);
            let actual = u16::from_le_bytes([self.field[0], self.field[1]]);
            if self.done as usize == 22 + size && actual != self.crc {
                return Err(StreamError::SectionCrc {
                    part,
                    expected: self.crc,
                    actual,
                });
            }
        } else {
            let at = d - 22 - size;
            if input != &sentinels(part)[1][at..at + n] {
                return Err(StreamError::SectionSentinel(part));
            }
            if self.done as usize == 38 + size {
                let record = SectionRecord {
                    part: if part == 0 {
                        Part::HeaderVars
                    } else {
                        Part::Classes
                    },
                    data_offset: u64::from(self.section().offset) + 20,
                    size: self.size,
                };
                let next = if part == 0 {
                    self.enter_section(Stage::Classes)
                } else {
                    self.enter_chunk(self.locator(2).offset.into())
                };
                return Ok(self.then(Step::Section(record), next));
            }
        }
        Ok(Step::Framing)
    }

    fn enter_chunk(&mut self, at: u64) -> Result<(), StreamError> {
        self.stage = Stage::Chunk;
        self.done = 0;
        self.size = 0;
        self.object = 0;
        self.ms_len = 0;
        self.words = 0;
        self.head = [0; HEAD];
        self.map = MapCursor {
            chunk: at,
            ..MapCursor::default()
        };
        if at + 4 > self.map_end() {
            return Err(StreamError::MapChunk(at));
        }
        Ok(())
    }

    fn feed_chunk(&mut self, input: &[u8]) -> Result<Step, StreamError> {
        let d = self.done;
        let n = input.len() as u32;
        self.done += n;
        let chunk = self.map.chunk;
        if d < 2 {
            self.field[d as usize..(d + n) as usize].copy_from_slice(input);
            if self.done == 2 {
                self.size = u32::from(u16::from_be_bytes([self.field[0], self.field[1]]));
                if !(2..=MAX_MAP_CHUNK).contains(&self.size)
                    || chunk + u64::from(self.size) + 2 > self.map_end()
                {
                    return Err(StreamError::MapChunk(chunk));
                }
                self.crc = crc8(SEED, &self.field[..2]);
            }
        } else if d < self.size {
            self.crc = crc8(self.crc, input);
        } else {
            let at = (d - self.size) as usize;
            self.field[at..at + n as usize].copy_from_slice(input);
            if self.done == self.size + 2 {
                let actual = u16::from_be_bytes([self.field[0], self.field[1]]);
                if actual != self.crc {
                    return Err(StreamError::MapCrc {
                        offset: chunk,
                        expected: self.crc,
                        actual,
                    });
                }
                if self.size == 2 {
                    if chunk + 4 != self.map_end() {
                        return Err(StreamError::MapChunk(chunk));
                    }
                    self.stage = Stage::Done;
                    return Ok(Step::Finished);
                }
                self.map = MapCursor {
                    chunk,
                    len: self.size,
                    at: chunk + 2,
                    ..MapCursor::default()
                };
                self.stage = Stage::Entry;
            }
        }
        Ok(Step::Framing)
    }

    fn feed_entry(&mut self, byte: u8) -> Result<Step, StreamError> {
        let end = self.map.chunk + u64::from(self.map.len);
        let at = self.map.at;
        self.map.at += 1;
        let last = byte & 0x80 == 0;
        let bits = if last && self.map.field == 1 {
            byte & 0x3f
        } else {
            byte & 0x7f
        };
        if self.map.shift > 56 {
            return Err(StreamError::MapEntry(at));
        }
        self.map.mc |= u64::from(bits) << self.map.shift;
        self.map.shift += 7;
        if !last {
            if self.map.at == end {
                return Err(StreamError::MapEntry(at));
            }
            return Ok(Step::Framing);
        }
        let value = self.map.mc;
        self.map.mc = 0;
        self.map.shift = 0;
        if self.map.field == 0 {
            self.map.handle = self
                .map
                .handle
                .checked_add(value)
                .ok_or(StreamError::MapEntry(at))?;
            self.map.field = 1;
            if self.map.at == end {
                return Err(StreamError::MapEntry(at));
            }
            return Ok(Step::Framing);
        }
        self.map.field = 0;
        self.map.loc = if byte & 0x40 != 0 {
            self.map.loc.checked_sub(value)
        } else {
            self.map.loc.checked_add(value)
        }
        .filter(|&loc| loc <= i64::MAX as u64)
        .ok_or(StreamError::MapEntry(at))?;
        self.enter_object()?;
        Ok(Step::Framing)
    }

    fn enter_object(&mut self) -> Result<(), StreamError> {
        let at = self.map.loc;
        self.stage = Stage::Object;
        self.object = at;
        self.done = 0;
        self.size = 0;
        self.ms_len = 0;
        self.words = 0;
        self.crc = SEED;
        let map = self.locator(2);
        if at < self.header_end()
            || at + 2 > self.header.source.length
            || (at >= u64::from(map.offset) && at < self.map_end())
        {
            return Err(StreamError::ObjectRange {
                handle: self.map.handle,
                offset: at,
            });
        }
        Ok(())
    }

    fn feed_object(&mut self, input: &[u8]) -> Result<Step, StreamError> {
        let handle = self.map.handle;
        let d = self.done as usize;
        let n = input.len();
        self.done += n as u32;
        if self.ms_len == 0 {
            self.crc = crc8(self.crc, input);
            self.field[d % 2..d % 2 + n].copy_from_slice(input);
            if self.done % 2 != 0 {
                return Ok(Step::Framing);
            }
            let word = u16::from_le_bytes([self.field[0], self.field[1]]);
            self.size |= u32::from(word & 0x7fff) << (15 * self.words);
            self.words += 1;
            if word & 0x8000 != 0 {
                if self.words == 2 {
                    return Err(StreamError::ObjectSize(handle));
                }
                return Ok(Step::Framing);
            }
            self.ms_len = self.done as u8;
            let map = self.locator(2);
            let end = self.object + u64::from(self.ms_len) + u64::from(self.size) + 2;
            if self.size == 0 {
                return Err(StreamError::ObjectSize(handle));
            }
            if end > self.header.source.length
                || (self.object < self.map_end() && end > u64::from(map.offset))
            {
                return Err(StreamError::ObjectRange {
                    handle,
                    offset: self.object,
                });
            }
            return Ok(Step::Framing);
        }
        let start = usize::from(self.ms_len);
        let data_end = start + self.size as usize;
        if d < data_end {
            self.crc = crc8(self.crc, input);
            let at = d - start;
            if at < HEAD {
                let take = n.min(HEAD - at);
                self.head[at..at + take].copy_from_slice(&input[..take]);
            }
            return Ok(Step::Payload(Part::Object));
        }
        self.field[d - data_end..d - data_end + n].copy_from_slice(input);
        if (self.done as usize) < data_end + 2 {
            return Ok(Step::Framing);
        }
        let actual = u16::from_le_bytes([self.field[0], self.field[1]]);
        if actual != self.crc {
            return Err(StreamError::ObjectCrc {
                handle,
                expected: self.crc,
                actual,
            });
        }
        let r2000 = self.top().magic == *b"AC1015";
        let len = (self.size as usize).min(HEAD);
        let (type_code, record) =
            prologue(&self.head[..len], r2000).ok_or(StreamError::ObjectHeader(handle))?;
        if record != handle {
            return Err(StreamError::ObjectHandle {
                map: handle,
                record,
            });
        }
        let committed = ObjectRecord {
            handle,
            offset: self.object,
            size: self.size,
            type_code,
        };
        let next = if self.map.at == self.map.chunk + u64::from(self.map.len) {
            self.enter_chunk(self.map.at + 2)
        } else {
            self.stage = Stage::Entry;
            Ok(())
        };
        Ok(self.then(Step::Object(committed), next))
    }
}

impl FlatWalker {
    /// Restore-time check that a decoded state is one the walk can reach,
    /// so a forged or stale checkpoint cannot index outside its buffers or
    /// request bytes outside the verified locators.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.header.failure.is_some() {
            return false;
        }
        let header_done = self.header.phase == Phase::Done && self.header.failure.is_none();
        if self.stage == Stage::Header {
            return !header_done && *self == Self::from_header(self.header.clone());
        }
        if !header_done {
            return false;
        }
        let fresh = Self::from_header(self.header.clone());
        let (done, size) = (u64::from(self.done), u64::from(self.size));
        let map = self.locator(2);
        let map_end = self.map_end();
        let chunk = self.map.chunk;
        let chunk_ok = (3..=MAX_MAP_CHUNK).contains(&self.map.len)
            && chunk >= u64::from(map.offset)
            && chunk + u64::from(self.map.len) + 2 <= map_end;
        let untouched = self.object == 0
            && self.ms_len == 0
            && self.words == 0
            && self.head == fresh.head
            && self.map == MapCursor::default();
        match self.stage {
            Stage::Header => false,
            Stage::Vars | Stage::Classes => {
                let total = u64::from(self.section().size);
                untouched
                    && total >= FRAME
                    && done < total
                    && if done < 20 {
                        size == 0
                    } else {
                        FRAME + size == total
                    }
            }
            Stage::Chunk => {
                self.object == 0
                    && self.ms_len == 0
                    && self.words == 0
                    && self.head == fresh.head
                    && self.map
                        == (MapCursor {
                            chunk,
                            ..MapCursor::default()
                        })
                    && chunk >= u64::from(map.offset)
                    && chunk + 4 <= map_end
                    && if done < 2 {
                        size == 0
                    } else {
                        (2..=u64::from(MAX_MAP_CHUNK)).contains(&size)
                            && chunk + size + 2 <= map_end
                            && done < size + 2
                    }
            }
            Stage::Entry => {
                chunk_ok
                    && self.map.at >= chunk + 2
                    && self.map.at < chunk + u64::from(self.map.len)
                    && self.map.field <= 1
                    && self.map.shift <= 56
                    && self.map.shift % 7 == 0
                    && self.map.loc <= i64::MAX as u64
            }
            Stage::Object => {
                let object = self.object;
                let record_end = object + u64::from(self.ms_len) + size + 2;
                chunk_ok
                    && self.map.at > chunk + 2
                    && self.map.at <= chunk + u64::from(self.map.len)
                    && self.map.field == 0
                    && self.map.shift == 0
                    && self.map.mc == 0
                    && object == self.map.loc
                    && object >= self.header_end()
                    && object <= i64::MAX as u64
                    && object + 2 <= self.header.source.length
                    && !(object >= u64::from(map.offset) && object < map_end)
                    && if self.ms_len == 0 {
                        self.words <= 1
                            && done >= 2 * u64::from(self.words)
                            && done < 2 * u64::from(self.words) + 2
                    } else {
                        (1..=2).contains(&self.words)
                            && u64::from(self.ms_len) == 2 * u64::from(self.words)
                            && size > 0
                            && record_end <= self.header.source.length
                            && !(object < map_end && record_end > u64::from(map.offset))
                            && done >= u64::from(self.ms_len)
                            && done < record_end - object
                    }
            }
            Stage::Done => true,
        }
    }
}

fn sentinels(part: u8) -> &'static [[u8; 16]; 2] {
    if part == 0 {
        &HEADER_VARS_SENTINELS
    } else {
        &CLASS_SENTINELS
    }
}

/// Object type (`BS`), the R2000 `RL` bit size and the handle (§20.1),
/// read MSB-first from the record's first bytes.
fn prologue(head: &[u8], r2000: bool) -> Option<(u16, u64)> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<u64> {
        let mut value = 0u64;
        for _ in 0..n {
            let byte = *head.get(at / 8)?;
            value = (value << 1) | u64::from((byte >> (7 - at % 8)) & 1);
            at += 1;
        }
        Some(value)
    };
    let type_code = match take(2)? {
        0 => {
            let low = take(8)?;
            (take(8)? << 8 | low) as u16
        }
        1 => take(8)? as u16,
        2 => 0,
        _ => 256,
    };
    if r2000 {
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
