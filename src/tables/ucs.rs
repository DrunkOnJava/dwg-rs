//! UCS table entry (ODA Open Design Specification v5.4.1 §19.5.6,
//! L6-06) — user coordinate system.
//!
//! A UCS stores three 3D points (origin + X axis + Y axis) that
//! collectively define a new coordinate frame relative to WCS, plus an
//! optional orthographic view type and an optional base-UCS handle for
//! inheritance.
//!
//! # Stream shape
//!
//! ```text
//! entry header (TV name + xref bits)
//! BD3    origin
//! BD3    x_axis_direction
//! BD3    y_axis_direction
//! BD     elevation            -- WCS Z of the origin plane
//! BS     ortho_view_type      -- 0..=6 per §19.5.6 (0 none, 1 top, 2 bottom,
//!                                 3 front, 4 back, 5 left, 6 right)
//! H      base_ucs_handle      -- 0 when this UCS is not inherited
//! ```

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::{Point3D, read_bd3};
use crate::error::{Error, Result};
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
use crate::version::Version;

/// Highest valid `ortho_view_type` value per ODA §19.5.6.
pub const MAX_ORTHO_VIEW_TYPE: i16 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct UcsEntry {
    pub header: TableEntryHeader,
    pub origin: Point3D,
    pub x_axis: Point3D,
    pub y_axis: Point3D,
    pub elevation: f64,
    pub ortho_view_type: i16,
    pub base_ucs_handle: Handle,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`UcsEntry`].
pub type Ucs = UcsEntry;

/// Decodes a `UcsEntry` table entry that follows the common object header.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<UcsEntry> {
    let header = read_table_entry_header(c, version)?;
    let origin = read_bd3(c)?;
    let x_axis = read_bd3(c)?;
    let y_axis = read_bd3(c)?;
    let elevation = c.read_bd()?;
    let ortho_view_type = c.read_bs()?;
    if !(0..=MAX_ORTHO_VIEW_TYPE).contains(&ortho_view_type) {
        return Err(Error::SectionMap(format!(
            "UCS ortho_view_type {ortho_view_type} outside 0..={MAX_ORTHO_VIEW_TYPE}"
        )));
    }
    let base_ucs_handle = c.read_handle()?;
    Ok(UcsEntry {
        header,
        origin,
        x_axis,
        y_axis,
        elevation,
        ortho_view_type,
        base_ucs_handle,
    })
}

/// Decode an R2007+ UCS whose name lives in the object's string stream
/// (ODA v5.4.1 §19.1 split layout, §20.4.6 UCS field table).
///
/// Data stream after the common object prefix:
///
/// ```text
/// B    64-flag
/// B    xref dependent
/// BS   xref index + 1
/// BD3  origin
/// BD3  x-axis direction
/// BD3  y-axis direction
/// BD   elevation
/// BS   orthographic view type
/// ```
///
/// The string stream carries only the entry name; the base-UCS handle
/// moved to the handle stream in R2007+, so it is reported as null.
///
/// # Unverified against a real file
///
/// The three sample corpora used to develop this port
/// (`sample_AC1032.dwg`, `line_2013.dwg`, `arc_2010.dwg`) contain no
/// UCS records, so this layout has not been confirmed on real bytes.
/// It is safe regardless: [`modern::SplitStream::finish`] rejects the
/// decode unless the field body lands exactly on the string stream, so
/// a wrong layout errors rather than returning invented values.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<UcsEntry> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let origin = read_bd3(&mut split.data)?;
    let x_axis = read_bd3(&mut split.data)?;
    let y_axis = read_bd3(&mut split.data)?;
    let elevation = split.data.read_bd()?;
    let ortho_view_type = split.data.read_bs()?;
    split.finish("UCS")?;
    if !(0..=MAX_ORTHO_VIEW_TYPE).contains(&ortho_view_type) {
        return Err(Error::SectionMap(format!(
            "UCS ortho_view_type {ortho_view_type} outside 0..={MAX_ORTHO_VIEW_TYPE}"
        )));
    }
    let name = split.strings.read_tv()?;
    Ok(UcsEntry {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        origin,
        x_axis,
        y_axis,
        elevation,
        ortho_view_type,
        base_ucs_handle: Handle {
            code: 0,
            counter: 0,
            value: 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_front_ucs() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"Front");
        // Origin (0,0,0)
        for _ in 0..3 {
            w.write_bd(0.0);
        }
        // X axis (1,0,0)
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_bd(0.0);
        // Y axis (0,0,1)
        w.write_bd(0.0);
        w.write_bd(0.0);
        w.write_bd(1.0);
        w.write_bd(0.0); // elevation
        w.write_bs(3); // ortho = front
        w.write_handle(2, 0); // null base_ucs
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let u = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(u.header.name, "Front");
        assert_eq!(
            u.x_axis,
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(
            u.y_axis,
            Point3D {
                x: 0.0,
                y: 0.0,
                z: 1.0
            }
        );
        assert_eq!(u.elevation, 0.0);
        assert_eq!(u.ortho_view_type, 3);
        assert_eq!(u.base_ucs_handle.value, 0);
    }

    #[test]
    fn r2007_split_stream_ucs_reads_name_from_string_stream() {
        let mut body = BitWriter::new();
        body.write_bs_u(0); // no EED
        body.write_b(true); // no xdictionary
        body.write_b(false); // no binary data
        body.write_b(false); // 64-flag
        body.write_b(false); // xref dependent
        body.write_bs(0); // xref index + 1
        for _ in 0..3 {
            body.write_bd(0.0); // origin
        }
        body.write_bd(1.0); // x axis
        body.write_bd(0.0);
        body.write_bd(0.0);
        body.write_bd(0.0); // y axis
        body.write_bd(0.0);
        body.write_bd(1.0);
        body.write_bd(0.0); // elevation
        body.write_bs(3); // ortho = front
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload = crate::string_stream::tests::build_payload(&bits, &["Front"]);
        let u = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(u.header.name, "Front");
        assert_eq!(u.ortho_view_type, 3);
        assert_eq!(
            u.x_axis,
            Point3D {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }
        );
        assert_eq!(u.base_ucs_handle.value, 0);
    }

    #[test]
    fn rejects_bad_ortho_view_type() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"Bad");
        for _ in 0..9 {
            w.write_bd(0.0);
        }
        w.write_bd(0.0); // elevation
        w.write_bs(42); // out-of-range
        w.write_handle(0, 0);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000).unwrap_err();
        assert!(
            matches!(&err, Error::SectionMap(msg) if msg.contains("ortho_view_type")),
            "got {err:?}"
        );
    }
}
