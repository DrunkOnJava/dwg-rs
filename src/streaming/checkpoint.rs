//! Versioned little-endian header checkpoints. No pointers, heap state or
//! transport handles. CRC detects corruption, not malicious forgery; source
//! identity is verified by the caller against its trusted immutable source.

use super::flat::{FlatWalker, MapCursor, Stage};
use super::lz77::{Ext, Lz77Decoder, Mode, WINDOW};
use super::{CHUNK_BYTES, FlatHeaderParser, SourceIdentity, StreamError};

const PREFIX: usize = 56;
const TRAILER: usize = 4;

/// Compact serialized state, held in one declared chunk.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    bytes: [u8; CHUNK_BYTES],
    len: usize,
}

impl Checkpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl FlatHeaderParser {
    /// Serialize all current §3.2 state, including a partial field or failure.
    /// Already-emitted application output is the caller's separate custody.
    pub fn checkpoint(&self) -> Result<Checkpoint, StreamError> {
        if self.filled > CHUNK_BYTES - PREFIX - TRAILER {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut result = Checkpoint {
            bytes: [0; CHUNK_BYTES],
            len: PREFIX + self.filled + TRAILER,
        };
        let b = &mut result.bytes;
        b[..4].copy_from_slice(b"DWGH");
        b[4] = 1;
        b[5] = self.phase as u8;
        b[6] = self.count;
        b[7] = self.index;
        b[8] = self.seen;
        b[9] = u8::from(self.failure.is_some());
        b[10..12].copy_from_slice(&(self.filled as u16).to_le_bytes());
        b[12..14].copy_from_slice(&(self.target as u16).to_le_bytes());
        b[14..16].copy_from_slice(&(CHUNK_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.source.length.to_le_bytes());
        b[24..56].copy_from_slice(&self.source.digest);
        b[PREFIX..PREFIX + self.filled].copy_from_slice(&self.bytes[..self.filled]);
        let end = result.len - TRAILER;
        let checksum = crate::crc::crc32(0, &b[..end]);
        b[end..result.len].copy_from_slice(&checksum.to_le_bytes());
        Ok(result)
    }

    /// Restore a bounded checkpoint against an independently supplied source
    /// identity. Validate every state invariant by deterministic replay of the
    /// bytes *inside the checkpoint*; no source bytes are read or requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < PREFIX + TRAILER || input.len() > CHUNK_BYTES {
            return Err(StreamError::InvalidCheckpoint);
        }
        if &input[..4] != b"DWGH"
            || input[4] != 1
            || input[14..16] != (CHUNK_BYTES as u16).to_le_bytes()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let filled = usize::from(u16::from_le_bytes([input[10], input[11]]));
        if filled > CHUNK_BYTES - PREFIX - TRAILER || input.len() != PREFIX + filled + TRAILER {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        let crc = crate::crc::crc32(0, &input[..end]);
        if input[end..] != crc.to_le_bytes() {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut length_bytes = [0; 8];
        length_bytes.copy_from_slice(&input[16..24]);
        let mut digest = [0; 32];
        digest.copy_from_slice(&input[24..56]);
        let source = SourceIdentity {
            digest,
            length: u64::from_le_bytes(length_bytes),
        };
        if source != expected_source {
            return Err(StreamError::SourceMismatch);
        }
        let mut parser = Self::new(source);
        let mut at = 0;
        while at < filled {
            let request = parser
                .request()
                .map_err(|_| StreamError::InvalidCheckpoint)?
                .ok_or(StreamError::InvalidCheckpoint)?;
            let n = request.len.min(filled - at);
            let result = parser.feed(request.offset, &input[PREFIX + at..PREFIX + at + n]);
            at += n;
            if result.is_err() && at != filled {
                return Err(StreamError::InvalidCheckpoint);
            }
        }
        if parser.phase as u8 != input[5]
            || parser.count != input[6]
            || parser.index != input[7]
            || parser.seen != input[8]
            || u8::from(parser.failure.is_some()) != input[9]
            || parser.target != usize::from(u16::from_le_bytes([input[12], input[13]]))
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(parser)
    }
}

const WALK_FIXED: usize = 6;
const WALK_BODY: usize = 87;

/// Serialized container-walk state: a header checkpoint (validated by
/// replay) followed by the walk cursors (validated against the verified
/// locators). Fits one declared chunk.
#[derive(Debug, Clone)]
pub struct WalkCheckpoint {
    bytes: [u8; CHUNK_BYTES],
    len: usize,
}

impl WalkCheckpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl FlatWalker {
    /// Serialize the walk between feeds. A failed walk is terminal and has
    /// no checkpoint; its error is returned instead. Provisional payload
    /// already handed out, and anything derived from it, is the caller's
    /// to save alongside.
    pub fn checkpoint(&self) -> Result<WalkCheckpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let header = self.header.checkpoint()?;
        let header = header.as_bytes();
        let body = WALK_FIXED + header.len();
        let mut result = WalkCheckpoint {
            bytes: [0; CHUNK_BYTES],
            len: body + WALK_BODY + TRAILER,
        };
        let b = &mut result.bytes;
        b[..4].copy_from_slice(b"DWGW");
        b[4] = 1;
        b[5] = header.len() as u8;
        b[WALK_FIXED..body].copy_from_slice(header);
        let w = &mut b[body..body + WALK_BODY];
        w[0] = self.stage as u8;
        w[1..5].copy_from_slice(&self.done.to_le_bytes());
        w[5..9].copy_from_slice(&self.size.to_le_bytes());
        w[9..11].copy_from_slice(&self.crc.to_le_bytes());
        w[11..15].copy_from_slice(&self.field);
        w[15..31].copy_from_slice(&self.head);
        w[31..39].copy_from_slice(&self.object.to_le_bytes());
        w[39] = self.ms_len;
        w[40] = self.words;
        w[41..49].copy_from_slice(&self.map.chunk.to_le_bytes());
        w[49..53].copy_from_slice(&self.map.len.to_le_bytes());
        w[53..61].copy_from_slice(&self.map.at.to_le_bytes());
        w[61..69].copy_from_slice(&self.map.handle.to_le_bytes());
        w[69..77].copy_from_slice(&self.map.loc.to_le_bytes());
        w[77..85].copy_from_slice(&self.map.mc.to_le_bytes());
        w[85] = self.map.shift;
        w[86] = self.map.field;
        let end = result.len - TRAILER;
        let checksum = crate::crc::crc32(0, &b[..end]);
        b[end..result.len].copy_from_slice(&checksum.to_le_bytes());
        Ok(result)
    }

    /// Restore against an independently supplied source identity. The
    /// embedded header is replayed; the cursors must form a reachable
    /// state over its locators. No source bytes are read or requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < WALK_FIXED + WALK_BODY + TRAILER || input.len() > CHUNK_BYTES {
            return Err(StreamError::InvalidCheckpoint);
        }
        let body = WALK_FIXED + usize::from(input[5]);
        if &input[..4] != b"DWGW" || input[4] != 1 || input.len() != body + WALK_BODY + TRAILER {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes() {
            return Err(StreamError::InvalidCheckpoint);
        }
        let header = FlatHeaderParser::restore(&input[WALK_FIXED..body], expected_source)?;
        let mut walker = Self::from_header(header);
        let w = &input[body..end];
        let u32_at = |at: usize| u32::from_le_bytes([w[at], w[at + 1], w[at + 2], w[at + 3]]);
        let u64_at = |at: usize| {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(&w[at..at + 8]);
            u64::from_le_bytes(bytes)
        };
        walker.stage = match w[0] {
            0 => Stage::Header,
            1 => Stage::Vars,
            2 => Stage::Classes,
            3 => Stage::Chunk,
            4 => Stage::Entry,
            5 => Stage::Object,
            6 => Stage::Done,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        walker.done = u32_at(1);
        walker.size = u32_at(5);
        walker.crc = u16::from_le_bytes([w[9], w[10]]);
        walker.field.copy_from_slice(&w[11..15]);
        walker.head.copy_from_slice(&w[15..31]);
        walker.object = u64_at(31);
        walker.ms_len = w[39];
        walker.words = w[40];
        walker.map = MapCursor {
            chunk: u64_at(41),
            len: u32_at(49),
            at: u64_at(53),
            handle: u64_at(61),
            loc: u64_at(69),
            mc: u64_at(77),
            shift: w[85],
            field: w[86],
        };
        if !walker.consistent() {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(walker)
    }
}

const LZ_FIXED: usize = 54;
const LZ_MAX: usize = LZ_FIXED + WINDOW + TRAILER;

/// Serialized §4.7 decoder state: the opcode cursor, the pending copy or
/// literal run, the totals and the retained window (the last
/// `min(produced, WINDOW)` output bytes, oldest first). At most
/// `54 + WINDOW + 4` bytes. Where the compressed bytes come from, and the
/// output already handed out, are the caller's to save alongside.
#[derive(Debug, Clone)]
pub struct Lz77Checkpoint {
    bytes: [u8; LZ_MAX],
    len: usize,
}

impl Lz77Checkpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Lz77Decoder {
    /// Serialize the decoder between `decode` calls. A failed decoder is
    /// terminal and has no checkpoint; its error is returned instead.
    pub fn checkpoint(&self) -> Result<Lz77Checkpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let kept = self.produced.min(WINDOW as u64) as usize;
        let mut result = Lz77Checkpoint {
            bytes: [0; LZ_MAX],
            len: LZ_FIXED + kept + TRAILER,
        };
        let b = &mut result.bytes;
        b[..4].copy_from_slice(b"DWGZ");
        b[4] = 1;
        b[5] = self.mode as u8;
        b[6] = self.ext as u8;
        b[7] = self.op1;
        b[8] = self.first;
        b[9] = self.lit;
        b[10..14].copy_from_slice(&self.distance.to_le_bytes());
        b[14..22].copy_from_slice(&self.acc.to_le_bytes());
        b[22..30].copy_from_slice(&self.count.to_le_bytes());
        b[30..38].copy_from_slice(&self.produced.to_le_bytes());
        b[38..46].copy_from_slice(&self.consumed.to_le_bytes());
        b[46..54].copy_from_slice(&self.limit.to_le_bytes());
        let oldest = self.produced - kept as u64;
        for (j, slot) in b[LZ_FIXED..LZ_FIXED + kept].iter_mut().enumerate() {
            *slot = self.window[((oldest + j as u64) % WINDOW as u64) as usize];
        }
        let end = result.len - TRAILER;
        let checksum = crate::crc::crc32(0, &b[..end]);
        b[end..result.len].copy_from_slice(&checksum.to_le_bytes());
        Ok(result)
    }

    /// Restore a decoder checkpoint. Length, version and CRC are checked
    /// before anything is copied; the fields must then form a state
    /// `decode` can reach. No compressed bytes are read or requested.
    pub fn restore(input: &[u8]) -> Result<Self, StreamError> {
        if input.len() < LZ_FIXED + TRAILER || input.len() > LZ_MAX {
            return Err(StreamError::InvalidCheckpoint);
        }
        if &input[..4] != b"DWGZ" || input[4] != 1 {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes() {
            return Err(StreamError::InvalidCheckpoint);
        }
        let u64_at = |at: usize| {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(&input[at..at + 8]);
            u64::from_le_bytes(bytes)
        };
        let produced = u64_at(30);
        let kept = produced.min(WINDOW as u64) as usize;
        if input.len() != LZ_FIXED + kept + TRAILER {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut decoder = Self::new(u64_at(46));
        decoder.mode = match input[5] {
            0 => Mode::Start,
            1 => Mode::Ext,
            2 => Mode::Literal,
            3 => Mode::Opcode,
            4 => Mode::Offset1,
            5 => Mode::Offset2,
            6 => Mode::Op2,
            7 => Mode::LitLen,
            8 => Mode::Copy,
            9 => Mode::Done,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        decoder.ext = match input[6] {
            0 => Ext::None,
            1 => Ext::Literal,
            2 => Ext::Count,
            3 => Ext::Long,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        decoder.op1 = input[7];
        decoder.first = input[8];
        decoder.lit = input[9];
        decoder.distance = u32::from_le_bytes([input[10], input[11], input[12], input[13]]);
        decoder.acc = u64_at(14);
        decoder.count = u64_at(22);
        decoder.produced = produced;
        decoder.consumed = u64_at(38);
        let oldest = produced - kept as u64;
        for (j, &byte) in input[LZ_FIXED..LZ_FIXED + kept].iter().enumerate() {
            decoder.window[((oldest + j as u64) % WINDOW as u64) as usize] = byte;
        }
        if !decoder.consistent() {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(decoder)
    }
}
