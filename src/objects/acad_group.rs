//! ACAD_GROUP object (spec §19.6.7 — L6-11) — named, ordered set of
//! entity handles.
//!
//! Groups are user-visible selection sets exposed via the GROUP
//! command. They behave like a loose collection: members retain
//! their own ownership (block record, layout) but the group holds
//! soft-pointer references to them.
//!
//! # Stream shape
//!
//! ```text
//! TV      name                  -- group name (often anonymous "*Axx")
//! B       unnamed               -- true for AutoCAD-generated "*A" groups
//! B       selectable            -- true if selecting any member selects all
//! BL      num_handles
//! H × N   member_handles
//! ```
//!
//! The `num_handles` count is capped at 100 000 per spec-safety
//! guidance: real drawings rarely exceed a few hundred group
//! members, and an unbounded count from an adversarial file would
//! otherwise allow a decompression-bomb-adjacent allocation vector.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Maximum GROUP member handle count honored by the decoder.
const MAX_GROUP_HANDLES: usize = 100_000;

/// Decoded ACAD_GROUP body.
#[derive(Debug, Clone, PartialEq)]
pub struct AcadGroup {
    pub name: String,
    pub unnamed: bool,
    pub selectable: bool,
    pub member_handles: Vec<Handle>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<AcadGroup> {
    let name = read_tv(c, version)?;
    let unnamed = c.read_b()?;
    let selectable = c.read_b()?;
    let num_handles = c.read_bl()? as usize;
    if num_handles > MAX_GROUP_HANDLES {
        return Err(Error::SectionMap(format!(
            "ACAD_GROUP claims {num_handles} member handles (>{MAX_GROUP_HANDLES} sanity cap)"
        )));
    }
    let mut member_handles = Vec::with_capacity(num_handles);
    for _ in 0..num_handles {
        member_handles.push(c.read_handle()?);
    }
    Ok(AcadGroup {
        name,
        unnamed,
        selectable,
        member_handles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn encode_tv_r2000(w: &mut BitWriter, s: &[u8]) {
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
    }

    #[test]
    fn roundtrip_empty_group() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"MyGroup");
        w.write_b(false); // named
        w.write_b(true); // selectable
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let g = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(g.name, "MyGroup");
        assert!(!g.unnamed);
        assert!(g.selectable);
        assert!(g.member_handles.is_empty());
    }

    #[test]
    fn roundtrip_populated_group() {
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"*A1");
        w.write_b(true); // unnamed (AutoCAD-generated)
        w.write_b(false); // not selectable as a whole
        w.write_bl(2);
        w.write_handle(5, 0x42);
        w.write_handle(5, 0x7F);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let g = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(g.name, "*A1");
        assert!(g.unnamed);
        assert!(!g.selectable);
        assert_eq!(g.member_handles.len(), 2);
        assert_eq!(g.member_handles[0].value, 0x42);
        assert_eq!(g.member_handles[1].value, 0x7F);
    }

    #[test]
    fn rejects_excessive_handle_count() {
        // Synthesize a stream where num_handles claims a count past the cap.
        let mut w = BitWriter::new();
        encode_tv_r2000(&mut w, b"G");
        w.write_b(false);
        w.write_b(true);
        // BL 0b00 tag then 32-bit LE = 200_001
        w.write_bl((MAX_GROUP_HANDLES + 1) as i32);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("ACAD_GROUP")));
    }
}
