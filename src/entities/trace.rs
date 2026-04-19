//! TRACE entity (§19.4.50) — identical stream to SOLID.
//!
//! TRACE predates SOLID and shares the same serialization. Most
//! modern drawings don't write TRACE, but the entity-type dispatch
//! still needs to recognize it.

use crate::bitcursor::BitCursor;
use crate::entities::solid::Solid;
use crate::error::Result;

pub use crate::entities::solid::decode as decode_as_solid;

/// TRACE is a type alias for SOLID on the wire; this wrapper just
/// renames the returned struct for call sites that care.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace(pub Solid);

pub fn decode(c: &mut BitCursor<'_>) -> Result<Trace> {
    Ok(Trace(decode_as_solid(c)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;
    use crate::entities::Point2D;

    #[test]
    fn trace_shares_solid_encoding() {
        let mut w = BitWriter::new();
        w.write_b(true); // default thickness
        w.write_bd(0.0); // elevation
        w.write_rd(0.0); w.write_rd(0.0);
        w.write_rd(1.0); w.write_rd(0.0);
        w.write_rd(0.0); w.write_rd(1.0);
        w.write_rd(1.0); w.write_rd(1.0);
        w.write_b(true); // default ext
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let t = decode(&mut c).unwrap();
        assert_eq!(t.0.corners[0], Point2D { x: 0.0, y: 0.0 });
        assert_eq!(t.0.corners[3], Point2D { x: 1.0, y: 1.0 });
    }
}
