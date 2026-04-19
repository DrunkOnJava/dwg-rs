//! ENDBLK entity (§19.4.18) — marks the end of a block's entity
//! sublist. Has no type-specific payload beyond the common entity
//! preamble.
//!
//! Included as a no-op entity so the full round-trip walk can still
//! reach it via the entity dispatcher.

use crate::bitcursor::BitCursor;
use crate::error::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndBlk;

pub fn decode(_c: &mut BitCursor<'_>) -> Result<EndBlk> {
    Ok(EndBlk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endblk_is_empty() {
        // Any cursor position is valid since there's nothing to read.
        let buf: [u8; 0] = [];
        let mut c = BitCursor::new(&buf);
        let _ = decode(&mut c).unwrap();
    }
}
