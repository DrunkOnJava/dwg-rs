//! R2004-family file header (ODA v5.4.1 §4.1): AC1018, AC1024, AC1027 and
//! AC1032. The header is the first 0x100 bytes: 0x80 plain bytes, the
//! 0x6C-byte block XORed with the magic sequence, then 0x14 bytes of that
//! sequence continued to 0x100. Like the flat header, it is a pure state
//! machine with exact byte demands and no output until every check passes.

use super::{Request, SourceIdentity, StreamError};

/// Bytes in the R2004-family file header.
pub const R2004_HEADER_BYTES: usize = 0x100;
const BLOCK: usize = 0x80;
const BLOCK_LEN: usize = 0x6C;
const FILE_ID: &[u8; 12] = b"AcFssFcAJMB\0";

/// §4.1 magic sequence, run on to 0x100 bytes.
const fn magic_sequence() -> [u8; R2004_HEADER_BYTES] {
    let mut out = [0u8; R2004_HEADER_BYTES];
    let mut seed: u32 = 1;
    let mut index = 0;
    while index < R2004_HEADER_BYTES {
        seed = seed.wrapping_mul(0x0003_43fd).wrapping_add(0x0026_9ec3);
        out[index] = (seed >> 16) as u8;
        index += 1;
    }
    out
}
const MAGIC: [u8; R2004_HEADER_BYTES] = magic_sequence();

/// Fixed fields of the decrypted block (§4.1), as block offset and value.
const FIXED: [(usize, u32); 6] = [
    (0x0c, 0),
    (0x10, 0x6c),
    (0x14, 0x04),
    (0x44, 0x20),
    (0x48, 0x80),
    (0x4c, 0x40),
];

/// A verified R2004-family file header: the plain fields and the decrypted
/// block. `page_map_address` is a file offset (the stored value + 0x100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R2004Header {
    pub magic: [u8; 6],
    pub maintenance: u8,
    pub preview_address: u32,
    pub app_version: u8,
    pub app_maintenance: u8,
    pub codepage: u16,
    pub security_flags: u32,
    pub summary_info_address: u32,
    pub vba_project_address: u32,
    pub last_page_id: u32,
    pub last_page_end: u64,
    pub second_header_address: u64,
    pub gap_amount: u32,
    pub page_amount: u32,
    pub page_map_id: u32,
    pub page_map_address: u64,
    pub section_map_id: u32,
    pub page_array_size: u32,
    pub gap_array_size: u32,
}

/// Pure suspendable state for §4.1. Call `request`, supply at most that
/// many bytes at its offset, and repeat; a short feed keeps its partial
/// field. Grammar failures are terminal values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2004HeaderParser {
    pub(super) source: SourceIdentity,
    pub(super) bytes: [u8; R2004_HEADER_BYTES],
    pub(super) filled: usize,
    pub(super) target: usize,
    pub(super) done: bool,
    pub(super) failure: Option<StreamError>,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | u64::from(u32_at(bytes, at + 4)) << 32
}

impl R2004HeaderParser {
    /// Start the §4.1 grammar without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self {
            source,
            bytes: [0; R2004_HEADER_BYTES],
            filled: 0,
            target: 6,
            done: false,
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.source
    }

    /// Absolute first unconsumed source byte, including partial fields.
    pub fn position(&self) -> u64 {
        self.filled as u64
    }

    /// Next exact byte demand; `None` once the header passed every check.
    pub fn request(&self) -> Result<Option<Request>, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.done {
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

    fn block(&self) -> [u8; BLOCK_LEN] {
        let mut block = [0; BLOCK_LEN];
        for (index, slot) in block.iter_mut().enumerate() {
            *slot = self.bytes[BLOCK + index] ^ MAGIC[index];
        }
        block
    }

    /// Verified output, available only after every §4.1 check passed.
    pub fn header(&self) -> Option<R2004Header> {
        if !self.done || self.failure.is_some() {
            return None;
        }
        let b = &self.bytes;
        let k = self.block();
        let mut magic = [0; 6];
        magic.copy_from_slice(&b[..6]);
        Some(R2004Header {
            magic,
            maintenance: b[0x0b],
            preview_address: u32_at(b, 0x0d),
            app_version: b[0x11],
            app_maintenance: b[0x12],
            codepage: u16::from_le_bytes([b[0x13], b[0x14]]),
            security_flags: u32_at(b, 0x18),
            summary_info_address: u32_at(b, 0x20),
            vba_project_address: u32_at(b, 0x24),
            last_page_id: u32_at(&k, 0x28),
            last_page_end: u64_at(&k, 0x2c),
            second_header_address: u64_at(&k, 0x34),
            gap_amount: u32_at(&k, 0x3c),
            page_amount: u32_at(&k, 0x40),
            page_map_id: u32_at(&k, 0x50),
            page_map_address: u64_at(&k, 0x54) + 0x100,
            section_map_id: u32_at(&k, 0x5c),
            page_array_size: u32_at(&k, 0x60),
            gap_array_size: u32_at(&k, 0x64),
        })
    }

    fn transition(&mut self) -> Result<(), StreamError> {
        if self.target == 6 {
            let mut magic = [0; 6];
            magic.copy_from_slice(&self.bytes[..6]);
            match &magic {
                b"AC1018" | b"AC1024" | b"AC1027" | b"AC1032" => {
                    self.target = R2004_HEADER_BYTES;
                    Ok(())
                }
                b"AC1012" | b"AC1014" | b"AC1015" | b"AC1021" => {
                    Err(StreamError::UnsupportedLayout(magic))
                }
                _ => Err(StreamError::InvalidMagic(magic)),
            }
        } else {
            self.verify()?;
            self.done = true;
            Ok(())
        }
    }

    /// Every §4.1 check; a failing field is named by its file offset.
    fn verify(&self) -> Result<(), StreamError> {
        if u32_at(&self.bytes, 0x28) != 0x80 {
            return Err(StreamError::R2004Field(0x28));
        }
        let mut block = self.block();
        if &block[..12] != FILE_ID {
            return Err(StreamError::R2004Field(BLOCK as u16));
        }
        let actual = u32_at(&block, 0x68);
        block[0x68..0x6c].fill(0);
        let expected = crate::crc::crc32(0, &block);
        if actual != expected {
            return Err(StreamError::R2004HeaderCrc { expected, actual });
        }
        for (at, value) in FIXED {
            if u32_at(&block, at) != value {
                return Err(StreamError::R2004Field((BLOCK + at) as u16));
            }
        }
        let trailer = BLOCK + BLOCK_LEN;
        if self.bytes[trailer..] != MAGIC[trailer..] {
            return Err(StreamError::R2004Field(trailer as u16));
        }
        // §4.3: the page map is a system page on a 0x20 boundary, with its
        // 0x14-byte header inside the source.
        let page_map = u64_at(&block, 0x54);
        let start = page_map.checked_add(0x100);
        if start.is_none_or(|at| at % 0x20 != 0 || at.saturating_add(0x14) > self.source.length) {
            return Err(StreamError::R2004Field((BLOCK + 0x54) as u16));
        }
        let second = u64_at(&block, 0x34);
        if second != 0 && second.saturating_add(BLOCK_LEN as u64) > self.source.length {
            return Err(StreamError::R2004Field((BLOCK + 0x34) as u16));
        }
        Ok(())
    }
}
