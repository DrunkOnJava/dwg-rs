//! XLINE entity (§19.4.58) — infinite line through a point.
//!
//! Same stream shape as RAY (a point + a direction vector). The only
//! semantic difference is that XLINE extends in both directions
//! from `point`; RAY is one-sided.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct XLine {
    pub point: Point3D,
    pub direction: Vec3D,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<XLine> {
    let point = read_bd3(c)?;
    let direction = read_bd3(c)?;
    Ok(XLine { point, direction })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_xline() {
        let mut w = BitWriter::new();
        w.write_bd(5.0);
        w.write_bd(5.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c).unwrap();
        assert_eq!(x.point.x, 5.0);
        assert_eq!(x.direction.y, 1.0);
    }
}
