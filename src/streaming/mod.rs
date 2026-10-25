//! Bounded, pausable DWG grammar. The initial reader covers the complete
//! R13–R2000 file header (ODA v5.4.1 §3.2), including CRC and sentinel.
//! It does not read objects or the R2004+ containers; see issue #205.

use core::fmt;

/// One declared chunk; grammar has no heap allocations or I/O dependencies.
pub const CHUNK_BYTES: usize = 256;
const END_SENTINEL: [u8; 16] = [
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
    WrongOffset { expected: u64, actual: u64 },
    InvalidFeed { maximum: usize, actual: usize },
    Truncated { offset: u64, needed: usize },
    InvalidMagic([u8; 6]),
    UnsupportedLayout([u8; 6]),
    LocatorCount(u32),
    LocatorNumber(u8),
    MissingLocator(u8),
    DuplicateLocator(u8),
    LocatorRange(u8),
    HeaderCrc { expected: u16, actual: u16 },
    HeaderSentinel,
    InvalidCheckpoint,
    SourceMismatch,
    Finished,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "streaming DWG: {self:?}")
    }
}

impl std::error::Error for StreamError {}

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
                    if self.seen & 7 != 7 {
                        return Err(StreamError::MissingLocator(0));
                    }
                    self.phase = Phase::Crc;
                    self.target += 2;
                } else {
                    self.target += 9;
                }
            }
            Phase::Crc => {
                let end = self.filled - 2;
                let xor = match self.count {
                    3 => 0xa598,
                    4 => 0x8101,
                    5 => 0x3cc4,
                    6 => 0x8461,
                    _ => return Err(StreamError::LocatorCount(u32::from(self.count))),
                };
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
