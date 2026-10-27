//! Bounded, pausable DWG grammar. It covers the complete R13–R2000 file
//! header (ODA v5.4.1 §3.2) and the R13–R2000 container walk: header
//! variables, classes, the object map and every mapped record's framing,
//! type and handle. For the R2004 family it verifies the §4.1 file header
//! and has a bounded, resumable decoder for the
//! R2004-family LZ77 dialect (§4.7); it walks the R2004 page map and
//! section map, assembles sections from their data pages (§4.6) and frames
//! the object section into records (§20.1). R2007 has its own whole-file walk
//! and frames its object section into records the same way. The decoding of an
//! object's fields remains open; see issue #205.

use core::fmt;

mod checkpoint;
mod flat;
mod lz77;
mod r2004;
mod r2004_maps;
mod r2004_objects;
mod r2004_sections;
mod r2004_walk;
mod r2007;
mod r2007_checkpoint;
mod r2007_codes;
mod r2007_lz;
mod r2007_objects;
mod records;
pub use checkpoint::{
    Checkpoint, Lz77Checkpoint, R2004Checkpoint, R2004MapCheckpoint, R2004ObjectCheckpoint,
    R2004SectionCheckpoint, R2004WalkCheckpoint, R2007ObjectCheckpoint, WalkCheckpoint,
};
pub use flat::{
    CLASS_SENTINELS, FlatWalker, HEADER_VARS_SENTINELS, ObjectRecord, Part, SectionRecord, Step,
};
pub use lz77::{Lz77Decoder, Progress, WINDOW};
pub use r2004::{R2004_HEADER_BYTES, R2004Header, R2004HeaderParser};
pub use r2004_maps::{
    MapDemand, MapStep, PageEntry, R2004MapWalker, SectionEntry, SectionPage, SystemKind,
};
pub use r2004_objects::{OBJECTS_SECTION, ObjectDemand, ObjectStep, R2004ObjectWalker};
pub use r2004_sections::{DataPage, R2004SectionWalker, STAGE, SectionDemand, SectionStep};
pub use r2004_walk::{MAX_SECTIONS, R2004Walker, WalkDemand, WalkStep};
pub use r2007::{
    Gather, MAX_NAME, R2007_WINDOW, R2007Demand, R2007Entry, R2007Header, R2007Page, R2007Section,
    R2007Step, R2007Walker,
};
pub use r2007_checkpoint::R2007Checkpoint;
pub use r2007_objects::{R2007ObjectDemand, R2007ObjectStep, R2007ObjectWalker};
pub use records::RecordFormat;

/// One declared chunk; grammar has no heap allocations or I/O dependencies.
pub const CHUNK_BYTES: usize = 256;
/// The 16 bytes that close an R13–R2000 file header (§3.2).
pub const END_SENTINEL: [u8; 16] = [
    0x95, 0xa0, 0x4e, 0x28, 0x99, 0x82, 0x1a, 0xe5, 0x5e, 0x41, 0xe0, 0x5f, 0x9d, 0x3a, 0x4d, 0x00,
];

/// Immutable identity supplied by the transport owner (prefer a content hash).
/// The parser never hashes the whole source to manufacture this identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceIdentity {
    pub digest: [u8; 32],
    pub length: u64,
}

/// Exact remaining demand of the current transition, in source coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub offset: u64,
    pub len: usize,
}

/// Errors are values with no heap-backed diagnostic buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    WrongOffset {
        expected: u64,
        actual: u64,
    },
    InvalidFeed {
        maximum: usize,
        actual: usize,
    },
    Truncated {
        offset: u64,
        needed: usize,
    },
    InvalidMagic([u8; 6]),
    UnsupportedLayout([u8; 6]),
    LocatorCount(u32),
    LocatorNumber(u8),
    MissingLocator(u8),
    DuplicateLocator(u8),
    LocatorRange(u8),
    HeaderCrc {
        expected: u16,
        actual: u16,
    },
    HeaderSentinel,
    InvalidCheckpoint,
    SourceMismatch,
    Finished,
    /// Part 0 is the header-variable section (§9), 1 the class section (§10.1).
    SectionSentinel(u8),
    SectionSize(u8),
    SectionCrc {
        part: u8,
        expected: u16,
        actual: u16,
    },
    MapChunk(u64),
    MapCrc {
        offset: u64,
        expected: u16,
        actual: u16,
    },
    MapEntry(u64),
    ObjectRange {
        handle: u64,
        offset: u64,
    },
    ObjectSize(u64),
    ObjectCrc {
        handle: u64,
        expected: u16,
        actual: u16,
    },
    ObjectHeader(u64),
    ObjectHandle {
        map: u64,
        record: u64,
    },
    /// §4.7: an opcode in the reserved range 0x00–0x0F, at input byte `at`.
    Lz77Opcode {
        at: u64,
        opcode: u8,
    },
    /// §4.7: a back-reference reaching before the first output byte.
    Lz77Offset {
        at: u64,
        distance: u32,
    },
    /// Output would pass the declared decompressed size.
    Lz77Overrun {
        limit: u64,
    },
    /// The compressed bytes ended before the 0x11 terminator.
    Lz77Truncated {
        consumed: u64,
    },
    /// A position or count would pass `u64::MAX`. No real input reaches
    /// this; a forged but CRC-valid checkpoint can.
    CounterOverflow,
    /// §4.1: the R2004-family header field at this file offset is not what
    /// the spec fixes (0x28, the file ID at 0x80, a fixed decrypted field,
    /// the trailer at 0xEC) or points outside the source (0xD4, 0xB4).
    R2004Field(u16),
    /// §4.1: CRC-32 of the decrypted 0x6C-byte block.
    R2004HeaderCrc {
        expected: u32,
        actual: u32,
    },
    /// §4.3: the system page at this address has the wrong type or
    /// compression, an empty size, or lies outside the source or its page.
    SystemPage(u64),
    /// §4.3: the system page's checksum.
    SystemPageChecksum {
        address: u64,
        expected: u32,
        actual: u32,
    },
    /// §4.4: page map entry `n` has number 0 or a size that is zero, not a
    /// multiple of 0x20, or overflows the address.
    PageEntry(u32),
    /// §4.4: the page map does not list itself at the header's address, or
    /// its own entry is smaller than the page.
    PageMapSelf,
    /// §4.4: the page map has no entry for the header's section map id.
    SectionMapMissing,
    /// §4.5: the section map header is not (count, 2, 0x7400, 0, ...).
    SectionMapHeader,
    /// §4.5: section description `n` or one of its page references is
    /// malformed (compression or encryption flag, page size, name, or a
    /// start offset that is not a rising multiple of the page size).
    SectionEntry(u32),
    /// A system page decoded to a size other than declared, or ended
    /// inside a record.
    MapLength(u64),
    /// §4.6: the data page at this address has a masked header that is not
    /// what the page map and section map say of it (type, section number,
    /// start offset, data size or page size), or whose data runs outside the
    /// page or the source.
    DataPage(u64),
    /// §4.6: the data page header's own checksum.
    DataPageHeaderChecksum {
        address: u64,
        expected: u32,
        actual: u32,
    },
    /// §4.6: the checksum over the data page's stored bytes.
    DataPageChecksum {
        address: u64,
        expected: u32,
        actual: u32,
    },
    /// §4.6: the data page's stream ended with fewer bytes than the page
    /// must give for its place in the section, which would leave an
    /// undeclared gap.
    DataPageLength(u64),
    /// A page offered to a section walker with this page number is not the
    /// next one it expects: no page is due, or the page map entry, the
    /// section map reference and the section disagree.
    SectionPageRef(u32),
    /// §4.5: the section with this id is encrypted, so its stored bytes
    /// are not its content.
    SectionEncrypted(u32),
    /// The section map lists more sections than the whole-file walker's
    /// table holds.
    SectionTableFull,
    /// A data page in the page map names a section the section map does
    /// not describe; the number is the page's.
    SectionPageUnknown(u32),
    /// §20.1: the object section does not open with the four-byte prologue
    /// (an `RL` of 0x0DCA), or is too short to hold it.
    ObjectPrologue,
    /// §20.1: the record at this section offset has an `MS` size of zero or
    /// one that does not fit two words.
    RecordSize(u64),
    /// §20.1: the record at this section offset would run past the end of
    /// the section.
    RecordRange(u64),
    /// §20.1: the CRC of the record at this section offset.
    RecordCrc {
        offset: u64,
        expected: u16,
        actual: u16,
    },
    /// §20.1: the `MC` handle-stream size, the type or the handle of the
    /// record at this section offset cannot be read from its first bytes, or
    /// the `MC` claims more bits than the record holds.
    RecordHeader(u64),
    /// The object section's bytes did not arrive in section order and
    /// without a gap: the next byte due is at `expected`.
    ObjectsOrder {
        expected: u64,
        actual: u64,
    },
    /// The walk ended before the object stream was consumed; this is the
    /// section offset it reached.
    ObjectsIncomplete(u64),
    /// The section map does not describe `AcDb:AcDbObjects`.
    ObjectsMissing,
    /// The data pages the page map holds for section `n` are not the pages
    /// the section map lists for it: a listed page is missing from the page
    /// map, or a page is walked that is not listed.
    SectionDigest(u32),
    /// §5.2: the R2007 header field at this place is not what the spec
    /// fixes or points outside the source. Below `0x1000` it is a file
    /// offset in the meta data, `0x1000 + n` is byte `n` of the decoded file
    /// header block and `0x2000 + n` byte `n` of the 0x110-byte header.
    R2007Header(u16),
    /// §5.13: a Reed-Solomon codeword of the page at this address has a
    /// non-zero syndrome; `codeword` counts from the page's first.
    R2007Parity {
        at: u64,
        codeword: u64,
    },
    /// §5.12: a 64-bit CRC of the page or header block at this address.
    /// `check` is 0 the file header's checking sequence, 1 its compressed
    /// data, 2 the header, 3 and 4 the check data at 0x3D8 (normal and
    /// mirrored), 5 and 6 a system page's stored and decompressed bytes, 7 a
    /// data page's stored bytes.
    R2007Crc64 {
        at: u64,
        check: u8,
        expected: u64,
        actual: u64,
    },
    /// §5.4.1: the 32-bit checksum of a data page's decompressed bytes.
    R2007Checksum {
        at: u64,
        expected: u32,
        actual: u32,
    },
    /// A page at this address has sizes that cannot be walked: a zero or
    /// oversized stored size, a zero repeat factor, an entry too small for
    /// its codewords.
    R2007Page(u64),
    /// A section reference names a page id the page map does not list.
    R2007PageMissing(i64),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "streaming DWG: {self:?}")
    }
}

impl core::error::Error for StreamError {}

/// A section locator; unknown record 5 remains opaque rather than guessed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Locator {
    pub number: u8,
    pub offset: u32,
    pub size: u32,
}

/// A verified flat file header, not a parsed drawing or object census.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlatHeader {
    pub magic: [u8; 6],
    pub maintenance: u8,
    pub image_seeker: u32,
    pub codepage: u16,
    pub locator_count: u8,
    pub locators: [Locator; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Phase {
    Magic,
    Prefix,
    Count,
    Locator,
    Crc,
    Sentinel,
    Done,
}

/// Pure suspendable state for §3.2. No source reads occur inside this type.
/// Call `request`, supply at most that many bytes, and repeat. A short feed
/// retains its partial field and reduces the next request by exactly its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatHeaderParser {
    pub(super) source: SourceIdentity,
    pub(super) bytes: [u8; CHUNK_BYTES],
    pub(super) filled: usize,
    pub(super) target: usize,
    pub(super) count: u8,
    pub(super) index: u8,
    pub(super) seen: u8,
    pub(super) phase: Phase,
    pub(super) failure: Option<StreamError>,
}

impl FlatHeaderParser {
    /// Start the §3.2 grammar without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self {
            source,
            bytes: [0; CHUNK_BYTES],
            filled: 0,
            target: 6,
            count: 0,
            index: 0,
            seen: 0,
            phase: Phase::Magic,
            failure: None,
        }
    }

    /// Next exact byte demand; `None` means the header passed all checks.
    pub fn request(&self) -> Result<Option<Request>, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.phase == Phase::Done {
            return Ok(None);
        }
        let needed = self.target - self.filled;
        if self.target as u64 > self.source.length {
            return Err(StreamError::Truncated {
                offset: self.filled as u64,
                needed,
            });
        }
        Ok(Some(Request {
            offset: self.filled as u64,
            len: needed,
        }))
    }

    /// Consume one nonempty slice at the declared offset. Invalid delivery
    /// leaves state unchanged; a grammar failure becomes a terminal error.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<(), StreamError> {
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
        let end = self.filled + input.len();
        self.bytes[self.filled..end].copy_from_slice(input);
        self.filled = end;
        if self.filled == self.target {
            if let Err(error) = self.transition() {
                self.failure = Some(error);
                return Err(error);
            }
        }
        Ok(())
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

    /// Verified output becomes available only after the complete sentinel.
    pub fn header(&self) -> Option<FlatHeader> {
        if self.phase != Phase::Done || self.failure.is_some() {
            return None;
        }
        let mut locators = [Locator::default(); 6];
        for (index, item) in locators.iter_mut().enumerate().take(self.count as usize) {
            *item = self.locator(index);
        }
        Some(FlatHeader {
            magic: self.magic(),
            maintenance: self.bytes[11],
            image_seeker: self.u32_at(13),
            codepage: u16::from_le_bytes([self.bytes[19], self.bytes[20]]),
            locator_count: self.count,
            locators,
        })
    }

    /// Absolute first unconsumed source byte, including partial fields.
    pub fn position(&self) -> u64 {
        self.filled as u64
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.source
    }

    fn magic(&self) -> [u8; 6] {
        let mut magic = [0; 6];
        magic.copy_from_slice(&self.bytes[..6]);
        magic
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_le_bytes([
            self.bytes[at],
            self.bytes[at + 1],
            self.bytes[at + 2],
            self.bytes[at + 3],
        ])
    }

    fn locator(&self, index: usize) -> Locator {
        let at = 25 + index * 9;
        Locator {
            number: self.bytes[at],
            offset: self.u32_at(at + 1),
            size: self.u32_at(at + 5),
        }
    }

    fn transition(&mut self) -> Result<(), StreamError> {
        match self.phase {
            Phase::Magic => {
                match &self.magic() {
                    b"AC1012" | b"AC1014" | b"AC1015" => (),
                    b"AC1018" | b"AC1021" | b"AC1024" | b"AC1027" | b"AC1032" => {
                        return Err(StreamError::UnsupportedLayout(self.magic()));
                    }
                    _ => return Err(StreamError::InvalidMagic(self.magic())),
                }
                self.phase = Phase::Prefix;
                self.target = 21;
            }
            Phase::Prefix => {
                self.phase = Phase::Count;
                self.target = 25;
            }
            Phase::Count => {
                let count = self.u32_at(21);
                if !(3..=6).contains(&count) {
                    return Err(StreamError::LocatorCount(count));
                }
                self.count = count as u8;
                self.phase = Phase::Locator;
                self.target += 9;
            }
            Phase::Locator => {
                let locator = self.locator(self.index as usize);
                if locator.number > 5 {
                    return Err(StreamError::LocatorNumber(locator.number));
                }
                if self.seen & (1 << locator.number) != 0 {
                    return Err(StreamError::DuplicateLocator(locator.number));
                }
                let header_end = 25 + u64::from(self.count) * 9 + 18;
                let end = u64::from(locator.offset) + u64::from(locator.size);
                if end > self.source.length
                    || (locator.size != 0 && u64::from(locator.offset) < header_end)
                {
                    return Err(StreamError::LocatorRange(locator.number));
                }
                self.seen |= 1 << locator.number;
                self.index += 1;
                if self.index == self.count {
                    // The first three records have defined mandatory roles (§3.2.6).
                    for number in 0..3 {
                        if self.seen & (1 << number) == 0 {
                            return Err(StreamError::MissingLocator(number));
                        }
                    }
                    self.phase = Phase::Crc;
                    self.target += 2;
                } else {
                    self.target += 9;
                }
            }
            Phase::Crc => {
                let end = self.filled - 2;
                let xor = crate::crc::r13_locator_seed(usize::from(self.count))
                    .ok_or(StreamError::LocatorCount(u32::from(self.count)))?;
                let expected = crate::crc::crc8(0, &self.bytes[..end]) ^ xor;
                let actual = u16::from_le_bytes([self.bytes[end], self.bytes[end + 1]]);
                if actual != expected {
                    return Err(StreamError::HeaderCrc { expected, actual });
                }
                self.phase = Phase::Sentinel;
                self.target += 16;
            }
            Phase::Sentinel => {
                if self.bytes[self.filled - 16..self.filled] != END_SENTINEL {
                    return Err(StreamError::HeaderSentinel);
                }
                self.phase = Phase::Done;
            }
            Phase::Done => return Err(StreamError::Finished),
        }
        Ok(())
    }
}
