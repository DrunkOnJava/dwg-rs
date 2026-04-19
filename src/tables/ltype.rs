//! LTYPE table entry (§19.5.54) — linetype definition (dash/dot pattern).
//!
//! # Stream shape (R2004+)
//!
//! ```text
//! entry header (name + xref bits)
//! TV     description
//! BD     pattern_length
//! RC     alignment          -- always 'A' (65) in practice
//! RC     num_dashes
//! BD ×N  dash_lengths
//! RD ×N  scale_and_shape     -- pairs of (complex_shape_x_offset, y_offset)
//! ...
//! ```
//!
//! This decoder reads through `description`, the total pattern
//! length, and the dash count + dash lengths — enough to reproduce
//! the linetype in a viewer or convert to a DXF representation.
//! Complex linetypes with embedded shapes read the shape offsets as
//! raw BDs and surface them as opaque bytes.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::tables::{TableEntryHeader, read_table_entry_header, read_tv};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct LType {
    pub header: TableEntryHeader,
    pub description: String,
    pub pattern_length: f64,
    pub alignment: u8,
    pub dashes: Vec<f64>,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<LType> {
    let header = read_table_entry_header(c, version)?;
    let description = read_tv(c, version)?;
    let pattern_length = c.read_bd()?;
    let alignment = c.read_rc()?;
    let num_dashes = c.read_rc()? as usize;
    if num_dashes > 1024 {
        return Err(Error::SectionMap(format!(
            "LTYPE num_dashes {num_dashes} exceeds 1024 sanity cap"
        )));
    }
    let mut dashes = Vec::with_capacity(num_dashes);
    for _ in 0..num_dashes {
        dashes.push(c.read_bd()?);
    }
    Ok(LType {
        header,
        description,
        pattern_length,
        alignment,
        dashes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_dashed_ltype() {
        let mut w = BitWriter::new();
        // Entry header
        let s = b"DASHED";
        w.write_bs_u(s.len() as u16);
        for b in s {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
        // Body
        w.write_bs_u(14); // description length
        for b in b"Dashed ___ __ " {
            w.write_rc(*b);
        }
        w.write_bd(0.75); // pattern length
        w.write_rc(b'A'); // alignment
        w.write_rc(3); // 3 dashes
        w.write_bd(0.5);
        w.write_bd(-0.125);
        w.write_bd(0.125);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let l = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(l.header.name, "DASHED");
        assert_eq!(l.dashes.len(), 3);
        assert_eq!(l.dashes[0], 0.5);
        assert_eq!(l.dashes[1], -0.125);
        assert_eq!(l.alignment, b'A');
    }
}
