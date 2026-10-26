//! R2004-family data pages (ODA v5.4.1 §4.6) and the sections they assemble
//! (§4.5). One walker takes one section description, then one page at a
//! time from the caller, which has the page map's entry and the section
//! map's reference for it. Each page is read twice, as the system pages
//! are: a pass that checks the masked 0x20-byte header's checksum and then
//! the data checksum over the stored bytes and keeps nothing, then a pass
//! that decodes the data (§4.7, or a copy when the section is stored) and
//! hands out section bytes in order. Nothing is derived from a page before
//! its checksums pass. The walker holds no map and no section bytes: the
//! page index is the caller's, and each step's bytes are the caller's to
//! take before the next call.
//!
//! The §4.6 header is not laid out as `section_map::DataPageHeader` labels
//! it. Measured on every data page of 42 real and synthetic files, after
//! the Sec_Mask XOR (`0x4164536B` ^ the page's file offset, per word):
//! `0x00` type `0x4163043B`, `0x04` section number, `0x08` data size (the
//! stored bytes after the header), `0x0C` page size (the page map's size
//! for the page, not the decompressed size), `0x10` start offset (8 bytes),
//! `0x18` header checksum, `0x1C` data checksum. The data checksum is §4.2
//! over the data bytes with seed 0; the header checksum is §4.2 over the
//! 0x20 header bytes with its own field zeroed, seeded with the data
//! checksum.

use super::lz77::Lz77Decoder;
use super::r2004_maps::{CARRY, PageEntry, SectionEntry, SectionPage};
use super::{Request, SourceIdentity, StreamError};
use crate::crc::section_page_checksum;

const HEAD: usize = 0x20;
const PAGE_TYPE: u32 = 0x4163_043b;
const MASK: u32 = 0x4164_536b;
const CHUNK: u32 = 0x15b0;
/// Section bytes one step hands out at most, and the largest stored read.
pub const STAGE: usize = 256;

/// A data page whose header and both checksums verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataPage {
    pub section_id: u32,
    pub page_number: u32,
    pub address: u64,
    pub start_offset: u64,
    /// Stored bytes after the header (compressed for an LZ77 section).
    pub data_size: u32,
    pub page_size: u32,
}

/// What one feed or pump produced. `Hole`, `Data` and `PageEnd` follow a
/// `Page`, whose checksums have passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStep {
    Framing,
    Page(DataPage),
    /// Section bytes no page covers; they read as zeros.
    Hole {
        offset: u64,
        len: u64,
    },
    /// `len` bytes of the section at `offset`, in [`R2004SectionWalker::data`]
    /// until the next call.
    Data {
        offset: u64,
        len: usize,
    },
    /// The page's stream ended. `decompressed` counts everything it
    /// produced, `emitted` what lay inside the section.
    PageEnd {
        decompressed: u32,
        emitted: u32,
    },
    /// Every page was walked and every byte of `size` was handed out or
    /// declared a hole.
    Finished {
        size: u64,
        pages: u32,
    },
}

/// The walker's next need: source bytes, a pump with no source bytes, the
/// caller's next page, or nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionDemand {
    Read(Request),
    Pump,
    NextPage,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Stage {
    Idle = 0,
    Header = 1,
    Check = 2,
    Decode = 3,
    Done = 4,
}

/// Fixed-size state for one section's data pages.
#[derive(Debug, Clone)]
pub struct R2004SectionWalker {
    pub(super) source: SourceIdentity,
    pub(super) index: u32,
    pub(super) size: u64,
    pub(super) page_count: u32,
    pub(super) max_page: u32,
    pub(super) compressed: u32,
    pub(super) section_id: u32,
    pub(super) encrypted: u32,
    pub(super) stage: Stage,
    pub(super) page: PageEntry,
    pub(super) reference: SectionPage,
    pub(super) head: [u8; HEAD],
    pub(super) done: u32,
    pub(super) sum1: u32,
    pub(super) sum2: u32,
    pub(super) in_chunk: u32,
    pub(super) decoder: Lz77Decoder,
    pub(super) carry: [u8; CARRY],
    pub(super) carry_len: u8,
    pub(super) cursor: u64,
    pub(super) refs_done: u32,
    pub(super) last_start: u64,
    pub(super) first_ref: bool,
    out: [u8; STAGE],
    out_len: usize,
    pub(super) failure: Option<StreamError>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | u64::from(u32_at(bytes, at + 4)) << 32
}

const NO_PAGE: PageEntry = PageEntry {
    number: 0,
    size: 0,
    address: 0,
};

const NO_REFERENCE: SectionPage = SectionPage {
    section_id: 0,
    page_number: 0,
    data_size: 0,
    start_offset: 0,
};

impl R2004SectionWalker {
    /// Start one section. Its description comes from the section map walk;
    /// an encrypted section is refused, since its stored bytes are not
    /// the section's.
    pub fn new(source: SourceIdentity, section: SectionEntry) -> Result<Self, StreamError> {
        if !matches!(section.compressed, 1 | 2)
            || section.max_page_size == 0
            || section.encrypted > 2
        {
            return Err(StreamError::SectionEntry(section.index));
        }
        if section.encrypted == 1 {
            return Err(StreamError::SectionEncrypted(section.section_id));
        }
        Ok(Self {
            source,
            index: section.index,
            size: section.size,
            page_count: section.page_count,
            max_page: section.max_page_size,
            compressed: section.compressed,
            section_id: section.section_id,
            encrypted: section.encrypted,
            stage: Stage::Idle,
            page: NO_PAGE,
            reference: NO_REFERENCE,
            head: [0; HEAD],
            done: 0,
            sum1: 0,
            sum2: 0,
            in_chunk: 0,
            decoder: Lz77Decoder::new(0),
            carry: [0; CARRY],
            carry_len: 0,
            cursor: 0,
            refs_done: 0,
            last_start: 0,
            first_ref: true,
            out: [0; STAGE],
            out_len: 0,
            failure: None,
        })
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.source
    }

    /// Index of the section being walked, as the section map numbered it.
    pub fn section_index(&self) -> u32 {
        self.index
    }

    /// The section's id, as its map entry and every page header carry it.
    pub fn section_id(&self) -> u32 {
        self.section_id
    }

    /// Section bytes the last `Data` step produced; empty after any other
    /// step.
    pub fn data(&self) -> &[u8] {
        &self.out[..self.out_len]
    }

    fn body_at(&self) -> u64 {
        self.page.address + HEAD as u64
    }

    fn stored(&self) -> u32 {
        self.reference.data_size
    }

    /// Section bytes this page can contribute: nothing from a page at or
    /// past the section's end.
    fn room(&self) -> u64 {
        self.size.saturating_sub(self.reference.start_offset)
    }

    /// Stored bytes that are section bytes: the rest is padding.
    fn wanted(&self) -> u32 {
        u64::from(self.stored()).min(self.room()) as u32
    }

    fn produced(&self) -> u64 {
        if self.compressed == 2 {
            self.decoder.produced()
        } else {
            u64::from(self.done)
        }
    }

    /// The span before the current page that no page covers, until it has
    /// been handed out as a hole.
    fn hole(&self) -> Option<(u64, u64)> {
        if self.produced() > 0 {
            return None;
        }
        let to = self.reference.start_offset.min(self.size);
        (self.cursor < to).then(|| (self.cursor, to - self.cursor))
    }

    /// The next need. Pure: it changes nothing.
    pub fn demand(&self) -> Result<SectionDemand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let read = |offset: u64, len: usize| Ok(SectionDemand::Read(Request { offset, len }));
        match self.stage {
            Stage::Idle => Ok(if self.refs_done < self.page_count {
                SectionDemand::NextPage
            } else {
                SectionDemand::Pump
            }),
            Stage::Header => {
                let filled = self.done as usize;
                read(self.page.address + filled as u64, HEAD - filled)
            }
            Stage::Check => {
                let left = self.stored() - self.done;
                if left == 0 {
                    return Ok(SectionDemand::Pump);
                }
                read(
                    self.body_at() + u64::from(self.done),
                    left.min(super::CHUNK_BYTES as u32) as usize,
                )
            }
            Stage::Decode => {
                if self.hole().is_some() {
                    return Ok(SectionDemand::Pump);
                }
                if self.compressed == 1 {
                    let left = self.wanted() - self.done;
                    if left == 0 {
                        return Ok(SectionDemand::Pump);
                    }
                    return read(
                        self.body_at() + u64::from(self.done),
                        left.min(STAGE as u32) as usize,
                    );
                }
                if self.carry_len > 0 || self.decoder.has_pending_output() || self.decoder.is_done()
                {
                    return Ok(SectionDemand::Pump);
                }
                let left = self.stored() - self.done;
                if left == 0 {
                    return Ok(SectionDemand::Pump);
                }
                read(
                    self.body_at() + u64::from(self.done),
                    left.min(CARRY as u32) as usize,
                )
            }
            Stage::Done => Ok(SectionDemand::Done),
        }
    }

    /// Accept the next page, which the demand `NextPage` asked for. The
    /// caller supplies the page map's entry and the section map's
    /// reference for it; they must agree with each other and with the
    /// section. A refusal leaves the walker as it was.
    pub fn begin_page(
        &mut self,
        page: PageEntry,
        reference: SectionPage,
    ) -> Result<(), StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let refused = StreamError::SectionPageRef(reference.page_number);
        if self.stage != Stage::Idle
            || self.refs_done >= self.page_count
            || !self.job_ok(&page, &reference)
        {
            return Err(refused);
        }
        self.page = page;
        self.reference = reference;
        self.stage = Stage::Header;
        self.done = 0;
        self.head = [0; HEAD];
        Ok(())
    }

    fn job_ok(&self, page: &PageEntry, reference: &SectionPage) -> bool {
        page.number > 0
            && reference.page_number == page.number as u32
            && reference.section_id == self.section_id
            && page.size >= HEAD as u32
            && page.size % 0x20 == 0
            && page
                .address
                .checked_add(HEAD as u64)
                .is_some_and(|end| end <= self.source.length)
            && reference.start_offset % u64::from(self.max_page) == 0
            && reference
                .start_offset
                .checked_add(u64::from(self.max_page))
                .is_some()
            && (self.first_ref || reference.start_offset > self.last_start)
    }

    /// Consume one nonempty prefix of a `Read` demand. Invalid delivery
    /// leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<SectionStep, StreamError> {
        let SectionDemand::Read(request) = self.demand()? else {
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
        self.out_len = 0;
        let result = self.take(input);
        self.settle(result)
    }

    /// Advance a `Pump` demand: no source bytes are involved.
    pub fn pump(&mut self) -> Result<SectionStep, StreamError> {
        if self.demand()? != SectionDemand::Pump {
            return Err(StreamError::Finished);
        }
        self.out_len = 0;
        let result = self.advance();
        self.settle(result)
    }

    /// Reject the end of the source while a `Read` is outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            SectionDemand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(
        &mut self,
        result: Result<SectionStep, StreamError>,
    ) -> Result<SectionStep, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn take(&mut self, input: &[u8]) -> Result<SectionStep, StreamError> {
        match self.stage {
            Stage::Header => {
                let at = self.done as usize;
                self.head[at..at + input.len()].copy_from_slice(input);
                self.done += input.len() as u32;
                if self.done as usize == HEAD {
                    return self.header();
                }
                Ok(SectionStep::Framing)
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
                Ok(SectionStep::Framing)
            }
            Stage::Decode if self.compressed == 1 => {
                let before = u64::from(self.done);
                self.out[..input.len()].copy_from_slice(input);
                self.out_len = input.len();
                self.done += input.len() as u32;
                let offset = self.reference.start_offset + before;
                self.cursor = offset + input.len() as u64;
                Ok(SectionStep::Data {
                    offset,
                    len: input.len(),
                })
            }
            Stage::Decode => {
                let at = usize::from(self.carry_len);
                self.carry[at..at + input.len()].copy_from_slice(input);
                self.carry_len += input.len() as u8;
                self.done += input.len() as u32;
                self.decode()
            }
            Stage::Idle | Stage::Done => Err(StreamError::Finished),
        }
    }

    fn advance(&mut self) -> Result<SectionStep, StreamError> {
        match self.stage {
            Stage::Idle => self.tail(),
            Stage::Check => self.checked(),
            Stage::Decode => self.decode(),
            _ => Err(StreamError::Finished),
        }
    }

    /// Fields the page's header must carry, given the caller's entry and
    /// reference; the checksum is checked before this runs.
    fn header_fits(&self) -> bool {
        let h = &self.head;
        let data = u64::from(u32_at(h, 8));
        u32_at(h, 0) == PAGE_TYPE
            && u32_at(h, 4) == self.section_id
            && u64_at(h, 0x10) == self.reference.start_offset
            && u32_at(h, 8) == self.reference.data_size
            && u32_at(h, 12) == self.page.size
            && HEAD as u64 + data <= u64::from(self.page.size)
            && self
                .page
                .address
                .checked_add(HEAD as u64 + data)
                .is_some_and(|end| end <= self.source.length)
            && (self.compressed == 2 || u32_at(h, 8) <= self.max_page)
    }

    /// The header's own checksum: §4.2 over the header with that field
    /// zeroed, seeded with the data checksum, which it carries.
    fn header_sum(&self) -> u32 {
        let mut zeroed = self.head;
        zeroed[0x18..0x1c].fill(0);
        section_page_checksum(u32_at(&self.head, 0x1c), &zeroed)
    }

    fn header(&mut self) -> Result<SectionStep, StreamError> {
        let address = self.page.address;
        let mask = MASK ^ address as u32;
        for word in self.head.chunks_exact_mut(4) {
            let plain = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) ^ mask;
            word.copy_from_slice(&plain.to_le_bytes());
        }
        let expected = self.header_sum();
        let actual = u32_at(&self.head, 0x18);
        if expected != actual {
            return Err(StreamError::DataPageHeaderChecksum {
                address,
                expected,
                actual,
            });
        }
        if !self.header_fits() {
            return Err(StreamError::DataPage(address));
        }
        self.sum1 = 0;
        self.sum2 = 0;
        self.in_chunk = 0;
        self.done = 0;
        self.stage = Stage::Check;
        Ok(SectionStep::Framing)
    }

    fn checked(&mut self) -> Result<SectionStep, StreamError> {
        if self.in_chunk > 0 {
            self.sum1 %= 0xfff1;
            self.sum2 %= 0xfff1;
        }
        let expected = (self.sum2 << 16) | (self.sum1 & 0xffff);
        let actual = u32_at(&self.head, 0x1c);
        if expected != actual {
            return Err(StreamError::DataPageChecksum {
                address: self.page.address,
                expected,
                actual,
            });
        }
        self.stage = Stage::Decode;
        self.done = 0;
        self.sum1 = 0;
        self.sum2 = 0;
        self.in_chunk = 0;
        self.carry_len = 0;
        self.decoder = Lz77Decoder::new(if self.compressed == 2 {
            u64::from(self.max_page)
        } else {
            0
        });
        Ok(SectionStep::Page(DataPage {
            section_id: self.section_id,
            page_number: self.reference.page_number,
            address: self.page.address,
            start_offset: self.reference.start_offset,
            data_size: self.stored(),
            page_size: self.page.size,
        }))
    }

    fn decode(&mut self) -> Result<SectionStep, StreamError> {
        if let Some((offset, len)) = self.hole() {
            self.cursor = offset + len;
            return Ok(SectionStep::Hole { offset, len });
        }
        if self.compressed == 1 {
            return self.finish_page();
        }
        let before = self.decoder.produced();
        let carried = usize::from(self.carry_len);
        let progress = self.decoder.decode(&self.carry[..carried], &mut self.out)?;
        self.carry.copy_within(progress.consumed..carried, 0);
        self.carry_len -= progress.consumed as u8;
        let keep = self
            .room()
            .saturating_sub(before)
            .min(progress.produced as u64) as usize;
        if keep > 0 {
            let offset = self.reference.start_offset + before;
            self.out_len = keep;
            self.cursor = offset + keep as u64;
            return Ok(SectionStep::Data { offset, len: keep });
        }
        if self.decoder.is_done() {
            return self.finish_page();
        }
        if self.carry_len == 0 && !self.decoder.has_pending_output() && self.done == self.stored() {
            self.decoder.end_of_input()?;
        }
        Ok(SectionStep::Framing)
    }

    /// The page's stream is over. It must have given every section byte
    /// it was placed to give: a page shorter than that would leave a gap
    /// the page map does not declare.
    fn finish_page(&mut self) -> Result<SectionStep, StreamError> {
        let produced = if self.compressed == 2 {
            self.decoder.produced()
        } else {
            u64::from(self.stored())
        };
        if produced < self.room().min(u64::from(self.max_page)) {
            return Err(StreamError::DataPageLength(self.page.address));
        }
        let emitted = produced.min(self.room());
        self.refs_done += 1;
        self.last_start = self.reference.start_offset;
        self.first_ref = false;
        self.reset_page();
        Ok(SectionStep::PageEnd {
            decompressed: produced as u32,
            emitted: emitted as u32,
        })
    }

    fn reset_page(&mut self) {
        self.stage = Stage::Idle;
        self.page = NO_PAGE;
        self.reference = NO_REFERENCE;
        self.head = [0; HEAD];
        self.done = 0;
        self.carry_len = 0;
    }

    /// Every page is done: hand out what no page covered, then finish.
    fn tail(&mut self) -> Result<SectionStep, StreamError> {
        if self.cursor < self.size {
            let offset = self.cursor;
            self.cursor = self.size;
            return Ok(SectionStep::Hole {
                offset,
                len: self.size - offset,
            });
        }
        self.stage = Stage::Done;
        Ok(SectionStep::Finished {
            size: self.size,
            pages: self.refs_done,
        })
    }
}

impl R2004SectionWalker {
    /// Restore-time check that a decoded state is one the walk can reach,
    /// so a forged or stale checkpoint cannot index outside its buffers,
    /// underflow a counter or request bytes outside the source.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some()
            || !matches!(self.compressed, 1 | 2)
            || self.max_page == 0
            || self.encrypted != 0 && self.encrypted != 2
            || self.refs_done > self.page_count
            || self.first_ref != (self.refs_done == 0)
            || self.last_start % u64::from(self.max_page) != 0
            || (self.first_ref && self.last_start != 0)
            || self.cursor > self.size
        {
            return false;
        }
        let blank = self.page == NO_PAGE
            && self.reference == NO_REFERENCE
            && self.head == [0; HEAD]
            && self.done == 0
            && self.sum1 == 0
            && self.sum2 == 0
            && self.in_chunk == 0
            && self.carry_len == 0;
        let in_page = self.refs_done < self.page_count
            && self.job_ok(&self.page, &self.reference)
            && self.carry_len as usize <= CARRY;
        match self.stage {
            Stage::Idle => {
                // Every page done and the trailing hole handed out leaves the
                // cursor at the end, even for a section with no page.
                blank
                    && (self.refs_done > 0 || self.cursor == 0 || self.refs_done == self.page_count)
            }
            Stage::Done => blank && self.refs_done == self.page_count && self.cursor == self.size,
            Stage::Header => {
                in_page
                    && (self.done as usize) < HEAD
                    && self.head[self.done as usize..].iter().all(|&b| b == 0)
                    && self.sum1 == 0
                    && self.sum2 == 0
                    && self.in_chunk == 0
                    && self.carry_len == 0
                    && (self.refs_done > 0 || self.cursor == 0)
            }
            Stage::Check | Stage::Decode => {
                let header =
                    in_page && self.header_sum() == u32_at(&self.head, 0x18) && self.header_fits();
                if !header {
                    return false;
                }
                if self.stage == Stage::Check {
                    return self.done <= self.stored()
                        && self.in_chunk < CHUNK
                        && self.carry_len == 0
                        && (self.refs_done > 0 || self.cursor == 0);
                }
                let produced = self.produced();
                let cursor_ok = if produced == 0 {
                    self.cursor <= self.reference.start_offset.min(self.size)
                } else {
                    self.cursor
                        == self
                            .reference
                            .start_offset
                            .saturating_add(produced)
                            .min(self.size)
                };
                let stream_ok = if self.compressed == 2 {
                    self.done <= self.stored()
                        && self.decoder.limit() == u64::from(self.max_page)
                        && self
                            .decoder
                            .consumed()
                            .checked_add(u64::from(self.carry_len))
                            == Some(u64::from(self.done))
                } else {
                    self.done <= self.wanted() && self.carry_len == 0 && self.decoder.limit() == 0
                };
                cursor_ok && stream_ok && self.sum1 == 0 && self.sum2 == 0 && self.in_chunk == 0
            }
        }
    }
}
