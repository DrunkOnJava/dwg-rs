//! UCS table entry (§19.5.60) — user coordinate system.
//!
//! A UCS stores three 3D points: an origin plus two axes (X and Y)
//! that collectively define a new coordinate frame relative to WCS.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3};
use crate::error::Result;
use crate::tables::{TableEntryHeader, read_table_entry_header};
use crate::version::Version;

#[derive(Debug, Clone, PartialEq)]
pub struct Ucs {
    pub header: TableEntryHeader,
    pub origin: Point3D,
    pub x_axis: Point3D,
    pub y_axis: Point3D,
}

pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<Ucs> {
    let header = read_table_entry_header(c, version)?;
    let origin = read_bd3(c)?;
    let x_axis = read_bd3(c)?;
    let y_axis = read_bd3(c)?;
    Ok(Ucs {
        header,
        origin,
        x_axis,
        y_axis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_front_ucs() {
        let mut w = BitWriter::new();
        let s = b"Front";
        w.write_bs_u(s.len() as u16);
        for b in s { w.write_rc(*b); }
        w.write_b(false); w.write_bs(0); w.write_b(false);
        // Origin (0,0,0)
        for _ in 0..3 { w.write_bd(0.0); }
        // X axis (1,0,0)
        w.write_bd(1.0); w.write_bd(0.0); w.write_bd(0.0);
        // Y axis (0,0,1) — makes Z "down" in the local frame
        w.write_bd(0.0); w.write_bd(0.0); w.write_bd(1.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let u = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(u.header.name, "Front");
        assert_eq!(u.x_axis, Point3D { x: 1.0, y: 0.0, z: 0.0 });
        assert_eq!(u.y_axis, Point3D { x: 0.0, y: 0.0, z: 1.0 });
    }
}
