//! Versioned little-endian header checkpoints. No pointers, heap state or
//! transport handles. CRC detects corruption, not malicious forgery; source
//! identity is verified by the caller against its trusted immutable source.

use super::flat::{FlatWalker, MapCursor, Stage};
use super::lz77::{Ext, Lz77Decoder, Mode, WINDOW};
use super::r2004::{R2004_HEADER_BYTES, R2004HeaderParser};
use super::r2004_maps::{CARRY, PageEntry, R2004MapWalker, RECORD, Stage as MapStage, SystemKind};
use super::r2004_sections::{R2004SectionWalker, Stage as SectionStage};
use super::r2004_walk::{MAX_SECTIONS, R2004Walker, Slot, Stage as WalkStage};
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

const R4_MAX: usize = PREFIX + R2004_HEADER_BYTES + TRAILER;

/// Serialized §4.1 header state: the same layout as [`Checkpoint`] with a
/// `DWG4` marker and room for the whole 0x100-byte header, at most 316
/// bytes. Restore replays the retained bytes; no source byte is read.
#[derive(Debug, Clone)]
pub struct R2004Checkpoint {
    bytes: [u8; R4_MAX],
    len: usize,
}

impl R2004Checkpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl R2004HeaderParser {
    /// Serialize all current §4.1 state, including a partial field or failure.
    pub fn checkpoint(&self) -> R2004Checkpoint {
        let mut result = R2004Checkpoint {
            bytes: [0; R4_MAX],
            len: PREFIX + self.filled + TRAILER,
        };
        let b = &mut result.bytes;
        b[..4].copy_from_slice(b"DWG4");
        b[4] = 1;
        b[5] = u8::from(self.done);
        b[9] = u8::from(self.failure.is_some());
        b[10..12].copy_from_slice(&(self.filled as u16).to_le_bytes());
        b[12..14].copy_from_slice(&(self.target as u16).to_le_bytes());
        b[14..16].copy_from_slice(&(R2004_HEADER_BYTES as u16).to_le_bytes());
        b[16..24].copy_from_slice(&self.source.length.to_le_bytes());
        b[24..56].copy_from_slice(&self.source.digest);
        b[PREFIX..PREFIX + self.filled].copy_from_slice(&self.bytes[..self.filled]);
        let end = result.len - TRAILER;
        let checksum = crate::crc::crc32(0, &b[..end]);
        b[end..result.len].copy_from_slice(&checksum.to_le_bytes());
        result
    }

    /// Restore against an independently supplied source identity by
    /// replaying the bytes inside the checkpoint, then requiring the replayed
    /// state to equal the recorded one.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < PREFIX + TRAILER || input.len() > R4_MAX {
            return Err(StreamError::InvalidCheckpoint);
        }
        if &input[..4] != b"DWG4"
            || input[4] != 1
            || input[6..9] != [0, 0, 0]
            || input[14..16] != (R2004_HEADER_BYTES as u16).to_le_bytes()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let filled = usize::from(u16::from_le_bytes([input[10], input[11]]));
        if filled > R2004_HEADER_BYTES || input.len() != PREFIX + filled + TRAILER {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes() {
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
        if u8::from(parser.done) != input[5]
            || u8::from(parser.failure.is_some()) != input[9]
            || parser.target != usize::from(u16::from_le_bytes([input[12], input[13]]))
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(parser)
    }
}

const MAP_CURSORS: usize =
    2 + 20 + 8 + 4 * 6 + 1 + CARRY + 2 + RECORD + 8 + 4 + 1 + 8 + 4 * 7 + 8 + 1;
const MAP_MAX: usize = 7 + R4_MAX + 4 + LZ_MAX + MAP_CURSORS + TRAILER;

/// Serialized R2004 map-walk state: the header checkpoint (replayed), the
/// decoder checkpoint while a page is being decoded, and the cursors, which
/// must form a state the walk can reach. At most about 50 KB, bounded by the
/// decoder window.
#[derive(Debug, Clone)]
pub struct R2004MapCheckpoint {
    bytes: [u8; MAP_MAX],
    len: usize,
}

impl R2004MapCheckpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct Writer<'a> {
    bytes: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn put(&mut self, data: &[u8]) {
        self.bytes[self.at..self.at + data.len()].copy_from_slice(data);
        self.at += data.len();
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn get(&mut self, n: usize) -> Result<&'a [u8], StreamError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(StreamError::InvalidCheckpoint)?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(StreamError::InvalidCheckpoint)?;
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, StreamError> {
        Ok(self.get(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, StreamError> {
        let b = self.get(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, StreamError> {
        let b = self.get(8)?;
        let mut w = [0; 8];
        w.copy_from_slice(b);
        Ok(u64::from_le_bytes(w))
    }
}

impl R2004MapWalker {
    /// Serialize the walk between steps. A failed walk is terminal and has
    /// no checkpoint; its error is returned instead.
    pub fn checkpoint(&self) -> Result<R2004MapCheckpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let header = self.header.checkpoint();
        let decoder = if self.stage == MapStage::Decode {
            Some(self.decoder.checkpoint()?)
        } else {
            None
        };
        let mut result = R2004MapCheckpoint {
            bytes: [0; MAP_MAX],
            len: 0,
        };
        let mut w = Writer {
            bytes: &mut result.bytes,
            at: 0,
        };
        w.put(b"DWGM");
        w.put(&[1]);
        w.put(&(header.as_bytes().len() as u16).to_le_bytes());
        w.put(header.as_bytes());
        let lz = decoder.as_ref().map_or(&[][..], |d| d.as_bytes());
        w.put(&(lz.len() as u32).to_le_bytes());
        w.put(lz);
        w.put(&[self.stage as u8, self.kind as u8]);
        w.put(&self.sys);
        w.put(&self.page_at.to_le_bytes());
        for v in [
            self.page_size,
            self.done,
            self.sum1,
            self.sum2,
            self.in_chunk,
            self.entries,
        ] {
            w.put(&v.to_le_bytes());
        }
        w.put(&[self.carry_len]);
        w.put(&self.carry);
        w.put(&[self.record_len, self.record_need]);
        w.put(&self.record);
        w.put(&self.address.to_le_bytes());
        w.put(&self.section_map_size.to_le_bytes());
        w.put(&[u8::from(self.self_seen)]);
        w.put(&self.section_map_at.to_le_bytes());
        for v in [
            self.sections_left,
            self.refs_left,
            self.section_index,
            self.section_id,
            self.section_page,
            0,
            0,
        ] {
            w.put(&v.to_le_bytes());
        }
        w.put(&self.last_start.to_le_bytes());
        w.put(&[u8::from(self.first_ref)]);
        let end = w.at;
        let checksum = crate::crc::crc32(0, &result.bytes[..end]);
        result.bytes[end..end + TRAILER].copy_from_slice(&checksum.to_le_bytes());
        result.len = end + TRAILER;
        Ok(result)
    }

    /// Restore against an independently supplied source identity. The
    /// header is replayed, the decoder validated, and the cursors must form
    /// a reachable state. No source bytes are read or requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < 7 + TRAILER || input.len() > MAP_MAX {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if &input[..4] != b"DWGM"
            || input[4] != 1
            || input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut r = Reader {
            bytes: &input[..end],
            at: 5,
        };
        let header_len = usize::from(u16::from_le_bytes([input[5], input[6]]));
        r.at = 7;
        let header = R2004HeaderParser::restore(r.get(header_len)?, expected_source)?;
        let lz_len = r.u32()? as usize;
        let lz = r.get(lz_len)?;
        let mut walker = Self::new(expected_source);
        walker.header = header;
        if lz_len > 0 {
            walker.decoder = Lz77Decoder::restore(lz)?;
        }
        walker.stage = match r.u8()? {
            0 => MapStage::Header,
            1 => MapStage::SysHeader,
            2 => MapStage::Check,
            3 => MapStage::Decode,
            4 => MapStage::Done,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        walker.kind = match r.u8()? {
            0 => SystemKind::PageMap,
            1 => SystemKind::SectionMap,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        walker.sys.copy_from_slice(r.get(20)?);
        walker.page_at = r.u64()?;
        walker.page_size = r.u32()?;
        walker.done = r.u32()?;
        walker.sum1 = r.u32()?;
        walker.sum2 = r.u32()?;
        walker.in_chunk = r.u32()?;
        walker.entries = r.u32()?;
        walker.carry_len = r.u8()?;
        walker.carry.copy_from_slice(r.get(CARRY)?);
        walker.record_len = r.u8()?;
        walker.record_need = r.u8()?;
        walker.record.copy_from_slice(r.get(RECORD)?);
        walker.address = r.u64()?;
        walker.section_map_size = r.u32()?;
        walker.self_seen = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        walker.section_map_at = r.u64()?;
        walker.sections_left = r.u32()?;
        walker.refs_left = r.u32()?;
        walker.section_index = r.u32()?;
        walker.section_id = r.u32()?;
        walker.section_page = r.u32()?;
        if r.u32()? != 0 || r.u32()? != 0 {
            return Err(StreamError::InvalidCheckpoint);
        }
        walker.last_start = r.u64()?;
        walker.first_ref = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        if r.at != end || (lz_len > 0) != (walker.stage == MapStage::Decode) || !walker.consistent()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(walker)
    }
}

const SEC_FIXED: usize = 248;
const SEC_MAX: usize = SEC_FIXED + LZ_MAX + TRAILER;

/// Serialized state of one section's data-page walk: the section's fields,
/// the page in hand, the cursors and, while an LZ77 page is being decoded,
/// the decoder checkpoint. At most about 50 KB, bounded by the decoder
/// window. The caller's page index, and which page of which section comes
/// next, are the caller's to save alongside.
#[derive(Debug, Clone)]
pub struct R2004SectionCheckpoint {
    bytes: [u8; SEC_MAX],
    len: usize,
}

impl R2004SectionCheckpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl R2004SectionWalker {
    /// Serialize the walk between steps. A failed walk is terminal and has
    /// no checkpoint; its error is returned instead. Section bytes already
    /// handed out are the caller's.
    pub fn checkpoint(&self) -> Result<R2004SectionCheckpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let decoder = if self.stage == SectionStage::Decode && self.compressed == 2 {
            Some(self.decoder.checkpoint()?)
        } else {
            None
        };
        let mut result = R2004SectionCheckpoint {
            bytes: [0; SEC_MAX],
            len: 0,
        };
        let mut w = Writer {
            bytes: &mut result.bytes,
            at: 0,
        };
        w.put(b"DWGS");
        w.put(&[1, self.stage as u8]);
        w.put(&self.source.length.to_le_bytes());
        w.put(&self.source.digest);
        w.put(&self.index.to_le_bytes());
        w.put(&self.size.to_le_bytes());
        for v in [
            self.page_count,
            self.max_page,
            self.compressed,
            self.section_id,
            self.encrypted,
        ] {
            w.put(&v.to_le_bytes());
        }
        w.put(&self.page.number.to_le_bytes());
        w.put(&self.page.size.to_le_bytes());
        w.put(&self.page.address.to_le_bytes());
        w.put(&self.reference.page_number.to_le_bytes());
        w.put(&self.reference.data_size.to_le_bytes());
        w.put(&self.reference.start_offset.to_le_bytes());
        w.put(&self.head);
        for v in [self.done, self.sum1, self.sum2, self.in_chunk] {
            w.put(&v.to_le_bytes());
        }
        w.put(&[self.carry_len]);
        w.put(&self.carry);
        w.put(&self.cursor.to_le_bytes());
        w.put(&self.refs_done.to_le_bytes());
        w.put(&self.last_start.to_le_bytes());
        w.put(&[u8::from(self.first_ref)]);
        let lz = decoder.as_ref().map_or(&[][..], |d| d.as_bytes());
        w.put(&(lz.len() as u32).to_le_bytes());
        w.put(lz);
        let end = w.at;
        let checksum = crate::crc::crc32(0, &result.bytes[..end]);
        result.bytes[end..end + TRAILER].copy_from_slice(&checksum.to_le_bytes());
        result.len = end + TRAILER;
        Ok(result)
    }

    /// Restore against an independently supplied source identity. The
    /// decoder is validated and the cursors must form a state the walk can
    /// reach. No source bytes are read or requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < SEC_FIXED + TRAILER || input.len() > SEC_MAX {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if &input[..4] != b"DWGS"
            || input[4] != 1
            || input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut r = Reader {
            bytes: &input[..end],
            at: 5,
        };
        let stage = match r.u8()? {
            0 => SectionStage::Idle,
            1 => SectionStage::Header,
            2 => SectionStage::Check,
            3 => SectionStage::Decode,
            4 => SectionStage::Done,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        let length = r.u64()?;
        let mut digest = [0; 32];
        digest.copy_from_slice(r.get(32)?);
        let source = SourceIdentity { digest, length };
        if source != expected_source {
            return Err(StreamError::SourceMismatch);
        }
        let mut walker = Self::new(
            source,
            super::SectionEntry {
                index: 0,
                size: 0,
                page_count: 0,
                max_page_size: 1,
                compressed: 1,
                section_id: 0,
                encrypted: 0,
                name: [0; 64],
            },
        )?;
        walker.stage = stage;
        walker.index = r.u32()?;
        walker.size = r.u64()?;
        walker.page_count = r.u32()?;
        walker.max_page = r.u32()?;
        walker.compressed = r.u32()?;
        walker.section_id = r.u32()?;
        walker.encrypted = r.u32()?;
        walker.page = PageEntry {
            number: r.u32()? as i32,
            size: r.u32()?,
            address: r.u64()?,
        };
        let in_page = matches!(
            stage,
            SectionStage::Header | SectionStage::Check | SectionStage::Decode
        );
        walker.reference = super::SectionPage {
            section_id: if in_page { walker.section_id } else { 0 },
            page_number: r.u32()?,
            data_size: r.u32()?,
            start_offset: r.u64()?,
        };
        walker.head.copy_from_slice(r.get(32)?);
        walker.done = r.u32()?;
        walker.sum1 = r.u32()?;
        walker.sum2 = r.u32()?;
        walker.in_chunk = r.u32()?;
        walker.carry_len = r.u8()?;
        walker.carry.copy_from_slice(r.get(CARRY)?);
        walker.cursor = r.u64()?;
        walker.refs_done = r.u32()?;
        walker.last_start = r.u64()?;
        walker.first_ref = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        let lz_len = r.u32()? as usize;
        let lz = r.get(lz_len)?;
        if lz_len > 0 {
            walker.decoder = Lz77Decoder::restore(lz)?;
        }
        let decoding = walker.stage == SectionStage::Decode && walker.compressed == 2;
        if r.at != end || (lz_len > 0) != decoding || !walker.consistent() {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(walker)
    }
}

const SLOT_BYTES: usize = 4 + 4 + 8 + 4 + 4 + 4 + 4 + 8 + 4 + 8;
const FILE_FIXED: usize = 4 + 2 + 8 + 32 + 1 + MAX_SECTIONS * SLOT_BYTES + 16 + 1 + 32 + 4 + 4 + 4;
const FILE_MAX: usize = FILE_FIXED + MAP_MAX + SEC_MAX + TRAILER;

/// Serialized state of the whole R2004-family walk: the section table, the
/// map walker's checkpoint (replayed and validated by its own restore), and,
/// while a data page is being walked, the section walker's. At most about
/// 100 KB, bounded by two decoder windows. Self-contained: no page index
/// and no section list are the caller's, only the section bytes it has
/// already been handed and the cursors it keeps over them.
#[derive(Debug, Clone)]
pub struct R2004WalkCheckpoint {
    bytes: [u8; FILE_MAX],
    len: usize,
}

impl R2004WalkCheckpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl R2004Walker {
    /// Serialize the walk between steps. A failed walk is terminal and has
    /// no checkpoint; its error is returned instead.
    pub fn checkpoint(&self) -> Result<R2004WalkCheckpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let live = !matches!(self.stage, WalkStage::Verify | WalkStage::Done);
        // A finished map pass cannot be restored (it has nothing left to
        // walk), but the verified header it holds is still the walk's: keep
        // that alone, as the header checkpoint.
        let maps = if live {
            Some(self.maps.checkpoint()?)
        } else {
            None
        };
        let header = self.maps.header.checkpoint();
        let page = if self.stage == WalkStage::Walk {
            Some(self.page.checkpoint()?)
        } else {
            None
        };
        let mut result = R2004WalkCheckpoint {
            bytes: [0; FILE_MAX],
            len: 0,
        };
        let mut w = Writer {
            bytes: &mut result.bytes,
            at: 0,
        };
        w.put(b"DWGF");
        w.put(&[1, self.stage as u8]);
        w.put(&self.source.length.to_le_bytes());
        w.put(&self.source.digest);
        w.put(&[self.slot_count as u8]);
        for slot in &self.slots[..self.slot_count] {
            w.put(&slot.index.to_le_bytes());
            w.put(&slot.id.to_le_bytes());
            w.put(&slot.size.to_le_bytes());
            for v in [slot.page_count, slot.max_page, slot.compressed, slot.refs] {
                w.put(&v.to_le_bytes());
            }
            w.put(&slot.ref_digest.to_le_bytes());
            w.put(&slot.seen.to_le_bytes());
            w.put(&slot.seen_digest.to_le_bytes());
        }
        w.put(&self.entry.number.to_le_bytes());
        w.put(&self.entry.size.to_le_bytes());
        w.put(&self.entry.address.to_le_bytes());
        w.put(&[self.peeked as u8]);
        w.put(&self.peek);
        w.put(&self.pages.to_le_bytes());
        for part in [
            Some(maps.as_ref().map_or(header.as_bytes(), |c| c.as_bytes())),
            page.as_ref().map(|c| c.as_bytes()),
        ] {
            let bytes = part.unwrap_or(&[]);
            w.put(&(bytes.len() as u32).to_le_bytes());
            w.put(bytes);
        }
        let end = w.at;
        let checksum = crate::crc::crc32(0, &result.bytes[..end]);
        result.bytes[end..end + TRAILER].copy_from_slice(&checksum.to_le_bytes());
        result.len = end + TRAILER;
        Ok(result)
    }

    /// Restore against an independently supplied source identity. Both
    /// embedded walkers are validated by their own restore, and the table
    /// and cursors must form a state the walk can reach. No source bytes
    /// are read or requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        if input.len() < FILE_FIXED - MAX_SECTIONS * SLOT_BYTES + TRAILER || input.len() > FILE_MAX
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let end = input.len() - TRAILER;
        if &input[..4] != b"DWGF"
            || input[4] != 1
            || input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes()
        {
            return Err(StreamError::InvalidCheckpoint);
        }
        let mut r = Reader {
            bytes: &input[..end],
            at: 5,
        };
        let stage = match r.u8()? {
            0 => WalkStage::Maps,
            1 => WalkStage::Pages,
            2 => WalkStage::Peek,
            3 => WalkStage::Walk,
            4 => WalkStage::Verify,
            5 => WalkStage::Done,
            _ => return Err(StreamError::InvalidCheckpoint),
        };
        let length = r.u64()?;
        let mut digest = [0; 32];
        digest.copy_from_slice(r.get(32)?);
        let source = SourceIdentity { digest, length };
        if source != expected_source {
            return Err(StreamError::SourceMismatch);
        }
        let mut walker = Self::new(source);
        walker.stage = stage;
        let count = usize::from(r.u8()?);
        if count > MAX_SECTIONS {
            return Err(StreamError::InvalidCheckpoint);
        }
        walker.slot_count = count;
        for slot in &mut walker.slots[..count] {
            *slot = Slot {
                index: r.u32()?,
                id: r.u32()?,
                size: r.u64()?,
                page_count: r.u32()?,
                max_page: r.u32()?,
                compressed: r.u32()?,
                refs: r.u32()?,
                ref_digest: r.u64()?,
                seen: r.u32()?,
                seen_digest: r.u64()?,
            };
        }
        walker.entry = PageEntry {
            number: r.u32()? as i32,
            size: r.u32()?,
            address: r.u64()?,
        };
        walker.peeked = usize::from(r.u8()?);
        walker.peek.copy_from_slice(r.get(32)?);
        walker.pages = r.u32()?;
        let maps_len = r.u32()? as usize;
        let maps = r.get(maps_len)?;
        let page_len = r.u32()? as usize;
        let page = r.get(page_len)?;
        let live = !matches!(stage, WalkStage::Verify | WalkStage::Done);
        let walking = stage == WalkStage::Walk;
        if r.at != end || maps_len == 0 || (page_len > 0) != walking || walker.peeked > 32 {
            return Err(StreamError::InvalidCheckpoint);
        }
        if live {
            walker.maps = R2004MapWalker::restore(maps, source)?;
            walker.maps.pages_only = stage != WalkStage::Maps;
        } else {
            walker.maps.header = R2004HeaderParser::restore(maps, source)?;
        }
        if walking {
            walker.page = R2004SectionWalker::restore(page, source)?;
        }
        if !walker.consistent() {
            return Err(StreamError::InvalidCheckpoint);
        }
        Ok(walker)
    }
}
