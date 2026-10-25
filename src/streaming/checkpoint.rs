//! Versioned little-endian header checkpoints. No pointers, heap state or
//! transport handles. CRC detects corruption, not malicious forgery; source
//! identity is verified by the caller against its trusted immutable source.

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
