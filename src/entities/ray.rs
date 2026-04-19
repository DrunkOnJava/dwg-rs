//! RAY entity (§19.4.48) — infinite half-line.
//!
//! # Stream shape
//!
//! ```text
//! BD3 point            -- start point
//! BD3 vector           -- direction vector (unnormalized in stream,
//!                         but conventionally unit-length)
//! ```

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, Vec3D, read_bd3};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct Ray {
    pub start: Point3D,
    pub direction: Vec3D,
}

pub fn decode(c: &mut BitCursor<'_>) -> Result<Ray> {
    let start = read_bd3(c)?;
    let direction = read_bd3(c)?;
    Ok(Ray { start, direction })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn roundtrip_ray() {
        let mut w = BitWriter::new();
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let r = decode(&mut c).unwrap();
        assert_eq!(r.start, Point3D { x: 1.0, y: 2.0, z: 3.0 });
        assert_eq!(r.direction, Vec3D { x: 1.0, y: 0.0, z: 0.0 });
    }
}
