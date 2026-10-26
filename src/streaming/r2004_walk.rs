//! The whole R2004-family file walk (ODA v5.4.1 §4.1, §4.3–§4.6) behind one
//! state and one checkpoint. Pass A is the map walk: the header, the page
//! map and the section map, which fill a fixed section table (the caller
//! hears each section; the page references are counted and folded into a
//! digest, not kept). Pass B reads the page map again, entry by entry, and
//! walks every data page it lists in the order the page map gives, through
//! the section walker of [`super::r2004_sections`]: the page's checksummed
//! header names its section and its place in it, so nothing but the section
//! table is needed to decode it, and the walker holds no page index and
//! needs none from its caller. Each page's bytes come out tagged with their
//! section and offset in the section, in page-map order; holes are whatever
//! the consumer never writes. At the end every section's pages are
//! accounted for: the count and digest of the page references the section
//! map listed must equal the count and digest of the pages the page map
//! held. The digest is a sum of a 64-bit avalanche mix of (page number,
//! start offset, data size) over the pages, so a listed page that is
//! missing, one walked twice, or one in the wrong place changes it; it is an
//! integrity check on a file whose page headers are already checksummed,
//! not a security boundary. State is the map walker and the section walker
//! (two decoder windows) and a table of at most [`MAX_SECTIONS`] slots.
//!
//! A page's data size is the length of its stored bytes: a whole page for a
//! stored section (padding included) and the compressed length for an LZ77
//! one, so it cannot be bounded by the section's size, and no such bound is
//! checked. What bounds a page is its start offset (a whole multiple of the
//! section's page size) and the section's size: a page's last bytes may lie
//! past the end of the section (a padded last page), and those bytes are
//! decoded, to reach the stream's end and its checksums, and dropped; a page
//! that starts at or past the end gives no bytes at all.

use super::r2004::R2004Header;
use super::r2004_maps::{MapDemand, MapStep, PageEntry, R2004MapWalker, SectionEntry, SectionPage};
use super::r2004_sections::{DataPage, R2004SectionWalker, SectionDemand, SectionStep};
use super::{Request, SourceIdentity, StreamError};
use crate::crc::section_page_checksum;

/// Sections the table holds. Real drawings have 14 or fewer.
pub const MAX_SECTIONS: usize = 64;
const HEAD: usize = 0x20;
const PAGE_TYPE: u32 = 0x4163_043b;
const MASK: u32 = 0x4164_536b;

/// What the table keeps of one section: what the data pages need of its
/// description, and the page references it listed and the pages seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Slot {
    pub(super) index: u32,
    pub(super) id: u32,
    pub(super) size: u64,
    pub(super) page_count: u32,
    pub(super) max_page: u32,
    pub(super) compressed: u32,
    pub(super) refs: u32,
    pub(super) ref_digest: u64,
    pub(super) seen: u32,
    pub(super) seen_digest: u64,
}

/// What one feed or pump produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkStep {
    Framing,
    /// The verified file header, once.
    Header(R2004Header),
    /// A section description, in the section map's order, before any page.
    Section(SectionEntry),
    /// A data page whose header and checksums verified; its bytes follow.
    Page(DataPage),
    /// `len` bytes of section `section_id` at `offset`, in
    /// [`R2004Walker::data`] until the next call.
    Data {
        section_id: u32,
        offset: u64,
        len: usize,
    },
    /// The page's stream ended; `emitted` of its `decompressed` bytes lay
    /// inside the section.
    PageEnd {
        decompressed: u32,
        emitted: u32,
    },
    /// Every page the page map holds was walked and every section's pages
    /// are exactly the ones the section map listed.
    Finished {
        sections: u32,
        pages: u32,
    },
}

/// The walker's next need: source bytes, a pump with no source bytes, or
/// nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkDemand {
    Read(Request),
    Pump,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Stage {
    Maps = 0,
    Pages = 1,
    Peek = 2,
    Walk = 3,
    Verify = 4,
    Done = 5,
}

/// Fixed-size state for the whole walk.
#[derive(Debug, Clone)]
pub struct R2004Walker {
    pub(super) source: SourceIdentity,
    pub(super) stage: Stage,
    pub(super) maps: R2004MapWalker,
    pub(super) page: R2004SectionWalker,
    pub(super) slots: [Slot; MAX_SECTIONS],
    pub(super) slot_count: usize,
    pub(super) entry: PageEntry,
    pub(super) peek: [u8; HEAD],
    pub(super) peeked: usize,
    pub(super) pages: u32,
    pub(super) failure: Option<StreamError>,
}

pub(super) const NO_ENTRY: PageEntry = PageEntry {
    number: 0,
    size: 0,
    address: 0,
};

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// One page's contribution to a section's digest.
fn mix(page: u32, start: u64, data: u32) -> u64 {
    splitmix(splitmix(splitmix(u64::from(page)) ^ start) ^ u64::from(data))
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | u64::from(u32_at(bytes, at + 4)) << 32
}

fn placeholder() -> SectionEntry {
    SectionEntry {
        index: 0,
        size: 0,
        page_count: 0,
        max_page_size: 1,
        compressed: 1,
        section_id: 0,
        encrypted: 0,
        name: [0; 64],
    }
}

impl R2004Walker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self {
            source,
            stage: Stage::Maps,
            maps: R2004MapWalker::new(source),
            page: R2004SectionWalker::new(source, placeholder())
                .expect("the placeholder section is valid"),
            slots: [Slot::default(); MAX_SECTIONS],
            slot_count: 0,
            entry: NO_ENTRY,
            peek: [0; HEAD],
            peeked: 0,
            pages: 0,
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.source
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<R2004Header> {
        self.maps.header()
    }

    /// Section bytes the last `Data` step produced; empty after any other
    /// step.
    pub fn data(&self) -> &[u8] {
        self.page.data()
    }

    /// The next need. Pure: it changes nothing.
    pub fn demand(&self) -> Result<WalkDemand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        match self.stage {
            Stage::Maps | Stage::Pages => match self.maps.demand()? {
                MapDemand::Read(request) => Ok(WalkDemand::Read(request)),
                MapDemand::Pump => Ok(WalkDemand::Pump),
                MapDemand::Done => Err(StreamError::Finished),
            },
            Stage::Peek => Ok(WalkDemand::Read(Request {
                offset: self.entry.address + self.peeked as u64,
                len: HEAD - self.peeked,
            })),
            Stage::Walk => match self.page.demand()? {
                SectionDemand::Read(request) => Ok(WalkDemand::Read(request)),
                SectionDemand::Pump => Ok(WalkDemand::Pump),
                SectionDemand::NextPage | SectionDemand::Done => Err(StreamError::Finished),
            },
            Stage::Verify => Ok(WalkDemand::Pump),
            Stage::Done => Ok(WalkDemand::Done),
        }
    }

    /// Consume one nonempty prefix of a `Read` demand. Invalid delivery
    /// leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<WalkStep, StreamError> {
        let WalkDemand::Read(request) = self.demand()? else {
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
        let result = match self.stage {
            Stage::Maps | Stage::Pages => match self.maps.feed(offset, input) {
                Ok(step) => self.on_map(step),
                Err(error) => Err(error),
            },
            Stage::Peek => self.take_peek(input),
            Stage::Walk => match self.page.feed(offset, input) {
                Ok(step) => self.on_page(step),
                Err(error) => Err(error),
            },
            Stage::Verify | Stage::Done => Err(StreamError::Finished),
        };
        self.settle(result)
    }

    /// Advance a `Pump` demand: no source bytes are involved.
    pub fn pump(&mut self) -> Result<WalkStep, StreamError> {
        if self.demand()? != WalkDemand::Pump {
            return Err(StreamError::Finished);
        }
        let result = match self.stage {
            Stage::Maps | Stage::Pages => match self.maps.pump() {
                Ok(step) => self.on_map(step),
                Err(error) => Err(error),
            },
            Stage::Walk => match self.page.pump() {
                Ok(step) => self.on_page(step),
                Err(error) => Err(error),
            },
            Stage::Verify => self.verify(),
            Stage::Peek | Stage::Done => Err(StreamError::Finished),
        };
        self.settle(result)
    }

    /// Reject the end of the source while a `Read` is outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            WalkDemand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(&mut self, result: Result<WalkStep, StreamError>) -> Result<WalkStep, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn on_map(&mut self, step: MapStep) -> Result<WalkStep, StreamError> {
        match (self.stage, step) {
            (Stage::Maps, MapStep::Header(header)) => Ok(WalkStep::Header(header)),
            (Stage::Maps, MapStep::Section(section)) => {
                self.add_section(section)?;
                Ok(WalkStep::Section(section))
            }
            (Stage::Maps, MapStep::SectionPage(reference)) => {
                self.add_reference(reference)?;
                Ok(WalkStep::Framing)
            }
            (Stage::Maps, MapStep::Finished) => {
                let again = self.maps.pages()?;
                self.maps = again;
                self.stage = Stage::Pages;
                Ok(WalkStep::Framing)
            }
            (Stage::Pages, MapStep::Page(entry)) => {
                let top = self.maps.header().ok_or(StreamError::Finished)?;
                let number = entry.number as u32;
                if entry.number <= 0 || number == top.page_map_id || number == top.section_map_id {
                    return Ok(WalkStep::Framing);
                }
                let fits = entry
                    .address
                    .checked_add(HEAD as u64)
                    .is_some_and(|end| end <= self.source.length);
                if !fits {
                    return Err(StreamError::DataPage(entry.address));
                }
                self.entry = entry;
                self.peek = [0; HEAD];
                self.peeked = 0;
                self.stage = Stage::Peek;
                Ok(WalkStep::Framing)
            }
            (Stage::Pages, MapStep::Finished) => {
                self.stage = Stage::Verify;
                Ok(WalkStep::Framing)
            }
            _ => Ok(WalkStep::Framing),
        }
    }

    fn add_section(&mut self, section: SectionEntry) -> Result<(), StreamError> {
        if section.encrypted == 1 {
            return Err(StreamError::SectionEncrypted(section.section_id));
        }
        if self.slot_count == MAX_SECTIONS {
            return Err(StreamError::SectionTableFull);
        }
        let known = self.slots[..self.slot_count]
            .iter()
            .any(|slot| slot.id == section.section_id);
        if known {
            return Err(StreamError::SectionEntry(section.index));
        }
        self.slots[self.slot_count] = Slot {
            index: section.index,
            id: section.section_id,
            size: section.size,
            page_count: section.page_count,
            max_page: section.max_page_size,
            compressed: section.compressed,
            ..Slot::default()
        };
        self.slot_count += 1;
        Ok(())
    }

    fn add_reference(&mut self, reference: SectionPage) -> Result<(), StreamError> {
        let slot = self
            .slots
            .get_mut(self.slot_count.wrapping_sub(1))
            .ok_or(StreamError::SectionEntry(0))?;
        slot.refs = slot
            .refs
            .checked_add(1)
            .ok_or(StreamError::CounterOverflow)?;
        slot.ref_digest = slot.ref_digest.wrapping_add(mix(
            reference.page_number,
            reference.start_offset,
            reference.data_size,
        ));
        Ok(())
    }

    /// The 0x20-byte masked header of a data page: after its own checksum
    /// passes it names the section and the page's place in it, which is all
    /// the section walker needs to be started on the page.
    fn take_peek(&mut self, input: &[u8]) -> Result<WalkStep, StreamError> {
        self.peek[self.peeked..self.peeked + input.len()].copy_from_slice(input);
        self.peeked += input.len();
        if self.peeked < HEAD {
            return Ok(WalkStep::Framing);
        }
        let address = self.entry.address;
        let mask = MASK ^ address as u32;
        for word in self.peek.chunks_exact_mut(4) {
            let plain = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) ^ mask;
            word.copy_from_slice(&plain.to_le_bytes());
        }
        let head = self.peek;
        self.peek = [0; HEAD];
        self.peeked = 0;
        let mut zeroed = head;
        zeroed[0x18..0x1c].fill(0);
        let expected = section_page_checksum(u32_at(&head, 0x1c), &zeroed);
        let actual = u32_at(&head, 0x18);
        if expected != actual {
            return Err(StreamError::DataPageHeaderChecksum {
                address,
                expected,
                actual,
            });
        }
        if u32_at(&head, 0) != PAGE_TYPE {
            return Err(StreamError::DataPage(address));
        }
        let id = u32_at(&head, 4);
        let number = self.entry.number as u32;
        let (start, data) = (u64_at(&head, 0x10), u32_at(&head, 8));
        let slot = self.slots[..self.slot_count]
            .iter_mut()
            .find(|slot| slot.id == id)
            .ok_or(StreamError::SectionPageUnknown(number))?;
        slot.seen = slot
            .seen
            .checked_add(1)
            .ok_or(StreamError::CounterOverflow)?;
        slot.seen_digest = slot.seen_digest.wrapping_add(mix(number, start, data));
        let mut page = R2004SectionWalker::new(
            self.source,
            SectionEntry {
                index: slot.index,
                size: slot.size,
                page_count: 1,
                max_page_size: slot.max_page,
                compressed: slot.compressed,
                section_id: id,
                encrypted: 0,
                name: [0; 64],
            },
        )?;
        page.begin_page(
            self.entry,
            SectionPage {
                section_id: id,
                page_number: number,
                data_size: data,
                start_offset: start,
            },
        )?;
        self.page = page;
        self.stage = Stage::Walk;
        Ok(WalkStep::Framing)
    }

    fn on_page(&mut self, step: SectionStep) -> Result<WalkStep, StreamError> {
        match step {
            SectionStep::Framing | SectionStep::Hole { .. } => Ok(WalkStep::Framing),
            SectionStep::Page(page) => Ok(WalkStep::Page(page)),
            SectionStep::Data { offset, len } => Ok(WalkStep::Data {
                section_id: self.page.section_id(),
                offset,
                len,
            }),
            SectionStep::PageEnd {
                decompressed,
                emitted,
            } => {
                self.pages = self
                    .pages
                    .checked_add(1)
                    .ok_or(StreamError::CounterOverflow)?;
                self.entry = NO_ENTRY;
                self.stage = Stage::Pages;
                Ok(WalkStep::PageEnd {
                    decompressed,
                    emitted,
                })
            }
            SectionStep::Finished { .. } => Err(StreamError::Finished),
        }
    }

    /// Every section's pages are the pages its references listed.
    fn verify(&mut self) -> Result<WalkStep, StreamError> {
        for slot in &self.slots[..self.slot_count] {
            if slot.seen != slot.refs || slot.seen_digest != slot.ref_digest {
                return Err(StreamError::SectionDigest(slot.index));
            }
        }
        self.stage = Stage::Done;
        Ok(WalkStep::Finished {
            sections: self.slot_count as u32,
            pages: self.pages,
        })
    }
}

impl R2004Walker {
    /// Restore-time check that a decoded state is one the walk can reach.
    /// The embedded walkers have each been validated by their own restore;
    /// this ties them to each other and to the table.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.slot_count > MAX_SECTIONS {
            return false;
        }
        let table = &self.slots[..self.slot_count];
        for (at, slot) in table.iter().enumerate() {
            let sound = slot.index == at as u32
                && matches!(slot.compressed, 1 | 2)
                && slot.max_page > 0
                && slot.refs <= slot.page_count
                && slot.seen <= slot.refs
                && (slot.seen > 0 || slot.seen_digest == 0)
                && (slot.refs > 0 || slot.ref_digest == 0)
                && table[..at].iter().all(|other| other.id != slot.id);
            if !sound {
                return false;
            }
        }
        if self.slots[self.slot_count..]
            .iter()
            .any(|slot| *slot != Slot::default())
        {
            return false;
        }
        let complete = table.iter().all(|slot| slot.refs == slot.page_count);
        let blank = self.entry == NO_ENTRY && self.peeked == 0 && self.peek == [0; HEAD];
        let system = |number: i32| {
            self.maps.header().is_none_or(|top| {
                let n = number as u32;
                number <= 0 || n == top.page_map_id || n == top.section_map_id
            })
        };
        let entry_ok = self.entry.number > 0
            && self.entry.size != 0
            && self.entry.size % 0x20 == 0
            && !system(self.entry.number)
            && self
                .entry
                .address
                .checked_add(HEAD as u64)
                .is_some_and(|end| end <= self.source.length);
        match self.stage {
            Stage::Maps => {
                let last_ok = table.last().is_none_or(|last| {
                    last.refs.checked_add(self.maps.refs_left) == Some(last.page_count)
                });
                !self.maps.pages_only
                    && self.maps.section_index as usize == self.slot_count
                    && table.iter().all(|slot| slot.seen == 0)
                    && table[..table.len().saturating_sub(1)]
                        .iter()
                        .all(|slot| slot.refs == slot.page_count)
                    && last_ok
                    && blank
                    && self.pages == 0
            }
            Stage::Pages => self.maps.pages_only && complete && blank,
            Stage::Peek => {
                self.maps.pages_only
                    && complete
                    && entry_ok
                    && self.peeked < HEAD
                    && self.peek[self.peeked..].iter().all(|&b| b == 0)
            }
            Stage::Walk => {
                let slot = table.iter().find(|slot| slot.id == self.page.section_id());
                self.maps.pages_only
                    && complete
                    && entry_ok
                    && self.peeked == 0
                    && self.peek == [0; HEAD]
                    && slot.is_some_and(|slot| {
                        slot.seen > 0
                            && self.page.index == slot.index
                            && self.page.size == slot.size
                            && self.page.max_page == slot.max_page
                            && self.page.compressed == slot.compressed
                            && self.page.page_count == 1
                            && self.page.refs_done == 0
                            && self.page.page == self.entry
                    })
            }
            Stage::Verify => complete && blank,
            Stage::Done => {
                complete
                    && blank
                    && table
                        .iter()
                        .all(|slot| slot.seen == slot.refs && slot.seen_digest == slot.ref_digest)
            }
        }
    }
}
