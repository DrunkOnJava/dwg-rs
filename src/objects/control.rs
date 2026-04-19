//! Control objects (§19.5.1..§19.5.10) — table owners.
//!
//! A `*_CONTROL` object owns all entries in its corresponding
//! symbol table:
//!
//! | Control type    | Owned entries                     | Spec |
//! |-----------------|-----------------------------------|------|
//! | BLOCK_CONTROL   | BLOCK_HEADER (block definitions)  | §19.5.1 |
//! | LAYER_CONTROL   | LAYER                             | §19.5.2 |
//! | STYLE_CONTROL   | STYLE                             | §19.5.3 |
//! | LTYPE_CONTROL   | LTYPE                             | §19.5.4 |
//! | VIEW_CONTROL    | VIEW                              | §19.5.5 |
//! | UCS_CONTROL     | UCS                               | §19.5.6 |
//! | VPORT_CONTROL   | VPORT                             | §19.5.7 |
//! | APPID_CONTROL   | APPID                             | §19.5.8 |
//! | DIMSTYLE_CONTROL| DIMSTYLE                          | §19.5.9 |
//! | VP_ENT_HDR_CONTROL | VIEWPORT_ENTITY_HEADER         | §19.5.10 |
//!
//! Every control type has the same on-wire body:
//!
//! ```text
//! BL  num_entries
//! ```
//!
//! The handles to the owned entries live *after* the body and are
//! collected by the generic object-handle reader; this decoder only
//! reads the count.

use crate::bitcursor::BitCursor;
use crate::error::Result;

/// A control object — holds only the entry count. The actual entry
/// handles are attached via the object's handle-reference list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    pub num_entries: u32,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Control> {
    let num_entries = c.read_bl()? as u32;
    Ok(Control { num_entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_layer_control() {
        let mut w = BitWriter::new();
        w.write_bl(42);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ctrl = decode(&mut c).unwrap();
        assert_eq!(ctrl.num_entries, 42);
    }

    #[test]
    fn roundtrip_empty_control() {
        let mut w = BitWriter::new();
        w.write_bl(0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let ctrl = decode(&mut c).unwrap();
        assert_eq!(ctrl.num_entries, 0);
    }
}
