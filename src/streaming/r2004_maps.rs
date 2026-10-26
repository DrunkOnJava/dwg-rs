//! R2004-family system pages (ODA v5.4.1 §4.3–§4.5): after the §4.1 file
//! header, the section page map and then the section map. Each system page
//! is read twice, as the flat walk reads an object-map chunk twice: a pass
//! that checks the §4.2/§4.3 checksum over its compressed bytes and keeps
//! nothing, then a pass that decodes it (§4.7) and hands each record to the
//! caller as a committed step. Nothing is derived from a page before its
//! checksum passes. The walker holds no map: the page index and the section
//! list are the caller's.

use super::lz77::Lz77Decoder;
use super::r2004::{R2004Header, R2004HeaderParser};
use super::{Request, SourceIdentity, StreamError};
use crate::crc::section_page_checksum;

const SYS_HEADER: usize = 0x14;
const PAGE_MAP_TYPE: u32 = 0x4163_0e3b;
const SECTION_MAP_TYPE: u32 = 0x4163_003b;
/// Compressed bytes the walker accepts ahead of the decoder.
pub(super) const CARRY: usize = 64;
/// Largest record: a §4.5 section description.
pub(super) const RECORD: usize = 0x60;
const CHUNK: u32 = 0x15b0;

/// Which system page a step belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SystemKind {
    PageMap = 0,
    SectionMap = 1,
}

/// A §4.4 page map entry. A negative number is a gap; `address` is the
/// file offset the entries' sizes accumulate to (0x100 for the first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageEntry {
    pub number: i32,
    pub size: u32,
    pub address: u64,
}

/// A §4.5 section description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionEntry {
    pub index: u32,
    pub size: u64,
    pub page_count: u32,
    pub max_page_size: u32,
    pub compressed: u32,
    pub section_id: u32,
    pub encrypted: u32,
    pub name: [u8; 64],
}

/// A §4.5 page reference of the section described last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionPage {
    pub section_id: u32,
    pub page_number: u32,
    pub data_size: u32,
    pub start_offset: u64,
}

/// What one feed or pump produced. Records are committed: their page's
/// checksum has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapStep {
    Framing,
    Header(R2004Header),
    /// A system page whose header and checksum verified.
    SystemPage {
        kind: SystemKind,
        address: u64,
        compressed: u32,
        decompressed: u32,
    },
    Page(PageEntry),
    Sections(u32),
    Section(SectionEntry),
    SectionPage(SectionPage),
    Finished,
}

/// The walker's next need: source bytes, a pump with no source bytes, or
/// nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapDemand {
    Read(Request),
    Pump,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Stage {
    Header = 0,
    SysHeader = 1,
    Check = 2,
    Decode = 3,
    Done = 4,
}

/// Fixed-size state for the page map and section map walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2004MapWalker {
    pub(super) header: R2004HeaderParser,
    pub(super) decoder: Lz77Decoder,
    pub(super) stage: Stage,
    pub(super) kind: SystemKind,
    pub(super) sys: [u8; SYS_HEADER],
    pub(super) page_at: u64,
    pub(super) page_size: u32,
    pub(super) done: u32,
    pub(super) sum1: u32,
    pub(super) sum2: u32,
    pub(super) in_chunk: u32,
    pub(super) carry: [u8; CARRY],
    pub(super) carry_len: u8,
    pub(super) record: [u8; RECORD],
    pub(super) record_len: u8,
    pub(super) record_need: u8,
    pub(super) address: u64,
    pub(super) entries: u32,
    pub(super) self_seen: bool,
    pub(super) section_map_at: u64,
    pub(super) section_map_size: u32,
    pub(super) sections_left: u32,
    pub(super) refs_left: u32,
    pub(super) section_index: u32,
    pub(super) section_id: u32,
    pub(super) section_page: u32,
    pub(super) last_start: u64,
    pub(super) first_ref: bool,
    pub(super) failure: Option<StreamError>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | u64::from(u32_at(bytes, at + 4)) << 32
}

impl R2004MapWalker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self {
            header: R2004HeaderParser::new(source),
            decoder: Lz77Decoder::new(0),
            stage: Stage::Header,
            kind: SystemKind::PageMap,
            sys: [0; SYS_HEADER],
            page_at: 0,
            page_size: 0,
            done: 0,
            sum1: 0,
            sum2: 0,
            in_chunk: 0,
            carry: [0; CARRY],
            carry_len: 0,
            record: [0; RECORD],
            record_len: 0,
            record_need: 0,
            address: 0x100,
            entries: 0,
            self_seen: false,
            section_map_at: 0,
            section_map_size: 0,
            sections_left: 0,
            refs_left: 0,
            section_index: 0,
            section_id: 0,
            section_page: 0,
            last_start: 0,
            first_ref: true,
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.header.source
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<R2004Header> {
        self.header.header()
    }

    fn top(&self) -> Result<R2004Header, StreamError> {
        self.header.header().ok_or(StreamError::Finished)
    }

    fn compressed(&self) -> u32 {
        u32_at(&self.sys, 8)
    }

    fn decompressed(&self) -> u32 {
        u32_at(&self.sys, 4)
    }

    fn data_at(&self) -> u64 {
        self.page_at + SYS_HEADER as u64
    }

    /// The next need. Pure: it changes nothing.
    pub fn demand(&self) -> Result<MapDemand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let read = |offset: u64, len: usize| Ok(MapDemand::Read(Request { offset, len }));
        match self.stage {
            Stage::Header => match self.header.request()? {
                Some(request) => Ok(MapDemand::Read(request)),
                None => Ok(MapDemand::Pump),
            },
            Stage::SysHeader => {
                let filled = self.done as usize;
                read(self.page_at + filled as u64, SYS_HEADER - filled)
            }
            Stage::Check => {
                let left = self.compressed() - self.done;
                if left == 0 {
                    return Ok(MapDemand::Pump);
                }
                read(
                    self.data_at() + u64::from(self.done),
                    left.min(256) as usize,
                )
            }
            Stage::Decode => {
                let record_open = self.record_len < self.record_need;
                if self.carry_len > 0 || self.decoder.has_pending_output() {
                    return Ok(MapDemand::Pump);
                }
                if self.decoder.is_done() || !record_open && self.record_need > 0 {
                    return Ok(MapDemand::Pump);
                }
                let left = self.compressed() - self.done;
                if left == 0 {
                    return Ok(MapDemand::Pump);
                }
                read(
                    self.data_at() + u64::from(self.done),
                    left.min(CARRY as u32) as usize,
                )
            }
            Stage::Done => Ok(MapDemand::Done),
        }
    }

    /// Consume one nonempty prefix of a `Read` demand. Invalid delivery
    /// leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<MapStep, StreamError> {
        let MapDemand::Read(request) = self.demand()? else {
            return Err(StreamError::Finished);
        };
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
        let result = self.take(offset, input);
        self.settle(result)
    }

    /// Advance a `Pump` demand: no source bytes are involved.
    pub fn pump(&mut self) -> Result<MapStep, StreamError> {
        if self.demand()? != MapDemand::Pump {
            return Err(StreamError::Finished);
        }
        let result = self.advance();
        self.settle(result)
    }

    /// Reject the end of the source while a `Read` is outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            MapDemand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(&mut self, result: Result<MapStep, StreamError>) -> Result<MapStep, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn take(&mut self, offset: u64, input: &[u8]) -> Result<MapStep, StreamError> {
        match self.stage {
            Stage::Header => {
                self.header.feed(offset, input)?;
                Ok(MapStep::Framing)
            }
            Stage::SysHeader => {
                let at = self.done as usize;
                self.sys[at..at + input.len()].copy_from_slice(input);
                self.done += input.len() as u32;
                if self.done as usize == SYS_HEADER {
                    return self.system_header();
                }
                Ok(MapStep::Framing)
            }
            Stage::Check => {
                for &byte in input {
                    self.sum1 = self.sum1.wrapping_add(u32::from(byte));
                    self.sum2 = self.sum2.wrapping_add(self.sum1);
                    self.in_chunk += 1;
                    if self.in_chunk == CHUNK {
                        self.sum1 %= 0xfff1;
                        self.sum2 %= 0xfff1;
                        self.in_chunk = 0;
                    }
                }
                self.done += input.len() as u32;
                Ok(MapStep::Framing)
            }
            Stage::Decode => {
                let at = usize::from(self.carry_len);
                self.carry[at..at + input.len()].copy_from_slice(input);
                self.carry_len += input.len() as u8;
                self.done += input.len() as u32;
                self.advance()
            }
            Stage::Done => Err(StreamError::Finished),
        }
    }

    fn advance(&mut self) -> Result<MapStep, StreamError> {
        match self.stage {
            Stage::Header => {
                let top = self.top()?;
                self.enter_page(SystemKind::PageMap, top.page_map_address, 0);
                Ok(MapStep::Header(top))
            }
            Stage::Check => self.checked(),
            Stage::Decode => self.decode(),
            _ => Err(StreamError::Finished),
        }
    }

    fn enter_page(&mut self, kind: SystemKind, at: u64, size: u32) {
        self.stage = Stage::SysHeader;
        self.kind = kind;
        self.page_at = at;
        self.page_size = size;
        self.done = 0;
        self.sys = [0; SYS_HEADER];
        self.carry_len = 0;
        self.record_len = 0;
        self.record_need = 0;
    }

    /// §4.3 header: type, sizes and compression type, then the page must
    /// lie inside the source (and, for the section map, inside its page).
    fn system_header(&mut self) -> Result<MapStep, StreamError> {
        let at = self.page_at;
        let tag = match self.kind {
            SystemKind::PageMap => PAGE_MAP_TYPE,
            SystemKind::SectionMap => SECTION_MAP_TYPE,
        };
        let comp = self.compressed();
        let end = self
            .data_at()
            .checked_add(u64::from(comp))
            .ok_or(StreamError::SystemPage(at))?;
        let fits_page = self.kind == SystemKind::PageMap
            || SYS_HEADER as u64 + u64::from(comp) <= u64::from(self.page_size);
        if u32_at(&self.sys, 0) != tag
            || u32_at(&self.sys, 12) != 2
            || comp == 0
            || self.decompressed() == 0
            || end > self.source().length
            || !fits_page
        {
            return Err(StreamError::SystemPage(at));
        }
        let mut zeroed = self.sys;
        zeroed[16..20].fill(0);
        let seed = section_page_checksum(0, &zeroed);
        self.sum1 = seed & 0xffff;
        self.sum2 = seed >> 16;
        self.in_chunk = 0;
        self.done = 0;
        self.stage = Stage::Check;
        Ok(MapStep::Framing)
    }

    fn checked(&mut self) -> Result<MapStep, StreamError> {
        if self.in_chunk > 0 {
            self.sum1 %= 0xfff1;
            self.sum2 %= 0xfff1;
        }
        let expected = (self.sum2 << 16) | (self.sum1 & 0xffff);
        let actual = u32_at(&self.sys, 16);
        if expected != actual {
            return Err(StreamError::SystemPageChecksum {
                address: self.page_at,
                expected,
                actual,
            });
        }
        self.stage = Stage::Decode;
        self.done = 0;
        self.sum1 = 0;
        self.sum2 = 0;
        self.in_chunk = 0;
        self.decoder = Lz77Decoder::new(u64::from(self.decompressed()));
        self.carry_len = 0;
        self.record_len = 0;
        self.record_need = match self.kind {
            SystemKind::PageMap => 8,
            SystemKind::SectionMap => 20,
        };
        Ok(MapStep::SystemPage {
            kind: self.kind,
            address: self.page_at,
            compressed: self.compressed(),
            decompressed: self.decompressed(),
        })
    }

    /// Decode carried bytes into the open record; commit it when whole.
    fn decode(&mut self) -> Result<MapStep, StreamError> {
        let need = usize::from(self.record_need);
        let have = usize::from(self.record_len);
        if have < need || need == 0 {
            // With no record open the output slice is empty: the decoder may
            // only read on to its terminator, and any further output is an
            // overrun of the declared size.
            let carried = usize::from(self.carry_len);
            let progress = self
                .decoder
                .decode(&self.carry[..carried], &mut self.record[have..need])?;
            self.carry.copy_within(progress.consumed..carried, 0);
            self.carry_len -= progress.consumed as u8;
            self.record_len += progress.produced as u8;
        }
        if need == 0 || self.record_len < self.record_need {
            if self.decoder.is_done() {
                return self.finish_page();
            }
            if self.carry_len == 0
                && !self.decoder.has_pending_output()
                && self.done == self.compressed()
            {
                self.decoder.end_of_input()?;
            }
            return Ok(MapStep::Framing);
        }
        match self.kind {
            SystemKind::PageMap => self.page_record(),
            SystemKind::SectionMap => self.section_record(),
        }
    }

    fn open(&mut self, need: u8) {
        self.record_len = 0;
        self.record_need = need;
    }

    fn page_record(&mut self) -> Result<MapStep, StreamError> {
        let number = u32_at(&self.record, 0) as i32;
        let size = u32_at(&self.record, 4);
        if number < 0 && self.record_need == 8 {
            // A gap carries four more longs (§4.4) before it is committed.
            self.record_need = 24;
            return Ok(MapStep::Framing);
        }
        let index = self.entries;
        if number == 0 || size == 0 || size % 0x20 != 0 {
            return Err(StreamError::PageEntry(index));
        }
        let address = self.address;
        self.address = address
            .checked_add(u64::from(size))
            .ok_or(StreamError::PageEntry(index))?;
        self.entries = index.checked_add(1).ok_or(StreamError::CounterOverflow)?;
        let top = self.top()?;
        if number > 0 && number as u32 == top.page_map_id {
            if address != top.page_map_address
                || SYS_HEADER as u64 + u64::from(self.compressed()) > u64::from(size)
            {
                return Err(StreamError::PageMapSelf);
            }
            self.self_seen = true;
        }
        if number > 0 && number as u32 == top.section_map_id {
            self.section_map_at = address;
            self.section_map_size = size;
        }
        self.open(8);
        Ok(MapStep::Page(PageEntry {
            number,
            size,
            address,
        }))
    }

    fn section_record(&mut self) -> Result<MapStep, StreamError> {
        let r = &self.record;
        if self.record_need == 20 {
            // §4.5 header: count, 0x02, 0x7400, 0x00, then a long ODA sets
            // to the count and other writers to 0.
            let count = u32_at(r, 0);
            if u32_at(r, 4) != 2 || u32_at(r, 8) != 0x7400 || u32_at(r, 12) != 0 {
                return Err(StreamError::SectionMapHeader);
            }
            self.sections_left = count;
            self.open(if count == 0 { 0 } else { RECORD as u8 });
            if count == 0 {
                self.record_need = 0;
            }
            return Ok(MapStep::Sections(count));
        }
        if self.record_need == RECORD as u8 {
            let index = self.section_index;
            let mut name = [0u8; 64];
            name.copy_from_slice(&r[32..96]);
            let text = name.iter().position(|&b| b == 0).unwrap_or(64);
            let entry = SectionEntry {
                index,
                size: u64_at(r, 0),
                page_count: u32_at(r, 8),
                max_page_size: u32_at(r, 12),
                compressed: u32_at(r, 20),
                section_id: u32_at(r, 24),
                encrypted: u32_at(r, 28),
                name,
            };
            if !matches!(entry.compressed, 1 | 2)
                || entry.encrypted > 2
                || entry.max_page_size == 0
                || !name[..text].iter().all(|b| b.is_ascii_graphic())
            {
                return Err(StreamError::SectionEntry(index));
            }
            self.section_index = index.checked_add(1).ok_or(StreamError::CounterOverflow)?;
            self.section_id = entry.section_id;
            self.section_page = entry.max_page_size;
            self.refs_left = entry.page_count;
            self.first_ref = true;
            self.sections_left -= 1;
            self.next_record();
            return Ok(MapStep::Section(entry));
        }
        // A 16-byte page reference: start offsets rise and are whole pages.
        let index = self.section_index - 1;
        let start = u64_at(r, 8);
        if start % u64::from(self.section_page) != 0 || !self.first_ref && start <= self.last_start
        {
            return Err(StreamError::SectionEntry(index));
        }
        let reference = SectionPage {
            section_id: self.section_id,
            page_number: u32_at(r, 0),
            data_size: u32_at(r, 4),
            start_offset: start,
        };
        self.last_start = start;
        self.first_ref = false;
        self.refs_left -= 1;
        self.next_record();
        Ok(MapStep::SectionPage(reference))
    }

    fn next_record(&mut self) {
        if self.refs_left > 0 {
            self.open(16);
        } else if self.sections_left > 0 {
            self.open(RECORD as u8);
        } else {
            self.open(0);
        }
    }

    /// The stream reached 0x11: it must have produced exactly the declared
    /// size and ended between records.
    fn finish_page(&mut self) -> Result<MapStep, StreamError> {
        let whole = self.decoder.produced() == u64::from(self.decompressed());
        let between = self.record_len == 0;
        let complete = match self.kind {
            SystemKind::PageMap => true,
            SystemKind::SectionMap => self.record_need == 0,
        };
        // Bytes after the terminator (AutoCAD writes two zeros) are inside
        // the compressed size and are not interpreted.
        if !whole || !between || !complete {
            return Err(StreamError::MapLength(self.page_at));
        }
        self.carry_len = 0;
        match self.kind {
            SystemKind::PageMap => {
                if !self.self_seen {
                    return Err(StreamError::PageMapSelf);
                }
                if self.section_map_size == 0 {
                    return Err(StreamError::SectionMapMissing);
                }
                let (at, size) = (self.section_map_at, self.section_map_size);
                self.enter_page(SystemKind::SectionMap, at, size);
                Ok(MapStep::Framing)
            }
            SystemKind::SectionMap => {
                self.stage = Stage::Done;
                Ok(MapStep::Finished)
            }
        }
    }
}

impl R2004MapWalker {
    /// Restore-time check that a decoded state is one the walk can reach,
    /// so a forged or stale checkpoint cannot index outside its buffers,
    /// underflow a counter or request bytes outside the source.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.header.failure.is_some() {
            return false;
        }
        if self.stage == Stage::Header {
            // Before the Header step is emitted, whether or not the header
            // parser has finished, nothing else has moved.
            let mut same = Self::new(self.source());
            same.header = self.header.clone();
            return *self == same;
        }
        let Some(top) = self.header.header() else {
            return false;
        };
        let length = self.source().length;
        let usize_ok = usize::from(self.carry_len) <= CARRY
            && self.record_len <= self.record_need
            && usize::from(self.record_need) <= RECORD;
        let page_ok = match self.kind {
            SystemKind::PageMap => self.page_at == top.page_map_address && self.page_size == 0,
            SystemKind::SectionMap => {
                self.self_seen
                    && self.page_at == self.section_map_at
                    && self.page_size == self.section_map_size
                    && self.page_size > 0
            }
        };
        let in_source = self
            .page_at
            .checked_add(SYS_HEADER as u64)
            .is_some_and(|end| end <= length);
        if !usize_ok || !page_ok || !in_source || self.address < 0x100 {
            return false;
        }
        let comp = self.compressed();
        let sys_ok = || {
            let tag = match self.kind {
                SystemKind::PageMap => PAGE_MAP_TYPE,
                SystemKind::SectionMap => SECTION_MAP_TYPE,
            };
            u32_at(&self.sys, 0) == tag
                && u32_at(&self.sys, 12) == 2
                && comp > 0
                && self.decompressed() > 0
                && self
                    .data_at()
                    .checked_add(u64::from(comp))
                    .is_some_and(|e| e <= length)
                && (self.kind == SystemKind::PageMap
                    || SYS_HEADER as u64 + u64::from(comp) <= u64::from(self.page_size))
        };
        let quiet_decode = self.carry_len == 0 && self.record_len == 0 && self.record_need == 0;
        match self.stage {
            Stage::Header => false,
            Stage::SysHeader => {
                (self.done as usize) < SYS_HEADER
                    && self.sys[self.done as usize..].iter().all(|&b| b == 0)
                    && quiet_decode
            }
            Stage::Check => sys_ok() && self.done <= comp && self.in_chunk < CHUNK && quiet_decode,
            Stage::Decode => {
                let records_ok = match self.kind {
                    SystemKind::PageMap => {
                        matches!(self.record_need, 8 | 24)
                            && (self.record_need == 8 || (u32_at(&self.record, 0) as i32) < 0)
                    }
                    SystemKind::SectionMap => match self.record_need {
                        20 => {
                            self.section_index == 0
                                && self.sections_left == 0
                                && self.refs_left == 0
                        }
                        16 => self.refs_left > 0 && self.section_index > 0 && self.section_page > 0,
                        96 => self.refs_left == 0 && self.sections_left > 0,
                        0 => self.refs_left == 0 && self.sections_left == 0,
                        _ => false,
                    },
                };
                sys_ok()
                    && records_ok
                    && self.done <= comp
                    && self.decoder.limit() == u64::from(self.decompressed())
                    && self
                        .decoder
                        .consumed()
                        .checked_add(u64::from(self.carry_len))
                        == Some(u64::from(self.done))
                    && self.sum1 == 0
                    && self.sum2 == 0
                    && self.in_chunk == 0
            }
            Stage::Done => self.kind == SystemKind::SectionMap,
        }
    }
}
