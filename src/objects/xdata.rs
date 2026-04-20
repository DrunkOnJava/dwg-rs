//! XData — application-registered extended entity data (spec §3.5).
//!
//! XData is the per-entity escape hatch that lets third-party
//! applications attach typed metadata to any graphical object without
//! changing the DWG schema. Each blob is scoped to a registered
//! application name (APPID handle) and contains a list of typed
//! (group-code, value) pairs that mirror the DXF 1000-series codes.
//!
//! # Stream shape
//!
//! ```text
//! H            app_handle             -- points to an APPID symbol-table entry
//! // Then a sequence of (group_code, value) pairs until end-of-blob.
//! // Each pair begins with a BS group code in 1000..=1071; the decoder
//! // dispatches on the code to read the typed value (TV string, binary
//! // chunk, handle, double, short, long, or 3D point component).
//! ```
//!
//! # Group-code type map (spec §3.5 Table 3-4)
//!
//! | Codes          | Type              | Semantics |
//! |----------------|-------------------|-----------|
//! | 1000..=1003    | TV                | Short strings (≤ 255 chars) |
//! | 1004           | RC count + bytes  | Binary chunk |
//! | 1005           | H                 | Database handle |
//! | 1010..=1013    | BD × 3            | 3D point / displacement |
//! | 1020..=1029    | BD                | Y component or scalar |
//! | 1030..=1039    | BD                | Z component or scalar |
//! | 1040..=1042    | BD                | Scalar (distance / scale) |
//! | 1070           | BS                | Short integer flag |
//! | 1071           | BL                | Long integer |
//!
//! # Safety caps
//!
//! Item count is capped at [`MAX_XDATA_ITEMS`] to bound allocation
//! from adversarial inputs. Binary chunks inside a code-1004 item use
//! a per-chunk byte count read as an `RC` (≤ 255 bytes).

use crate::bitcursor::{BitCursor, Handle};
use crate::entities::Point3D;
use crate::error::{Error, Result};
use crate::tables::read_tv;
use crate::version::Version;

/// Sanity cap on XData item count per blob. Real drawings rarely exceed
/// a few dozen; this bound exists to defeat decompression-bomb-style
/// adversarial files that advertise billions of items.
pub const MAX_XDATA_ITEMS: usize = 100_000;

/// Decoded XData blob attached to a single entity / object.
#[derive(Debug, Clone, PartialEq)]
pub struct XData {
    /// Handle to the APPID symbol-table entry that registered this blob.
    pub app_handle: Handle,
    /// Ordered list of typed (group-code, value) pairs.
    pub items: Vec<XDataItem>,
}

/// A single typed value inside an XData blob, per the 1000-series
/// group-code type map. The decoder preserves the original group
/// code so round-trip write (future phase) has enough information
/// to reconstruct the byte stream.
#[derive(Debug, Clone, PartialEq)]
pub enum XDataItem {
    /// Group codes 1000..=1003 — bounded-length TV strings.
    String { code: u16, value: String },
    /// Group code 1004 — opaque byte chunk.
    Binary { bytes: Vec<u8> },
    /// Group code 1005 — soft-pointer handle reference.
    Handle { code: u16, handle: Handle },
    /// Group codes 1010..=1013 — 3D point / displacement.
    Point { code: u16, point: Point3D },
    /// Group codes 1020..=1029, 1030..=1039, 1040..=1042 — scalar.
    Real { code: u16, value: f64 },
    /// Group code 1070 — 16-bit short.
    Short { code: u16, value: i16 },
    /// Group code 1071 — 32-bit long.
    Long { code: u16, value: i32 },
}

/// Decode an XData blob given the number of items advertised by the
/// enclosing object (the object record carries the count — this
/// decoder consumes exactly `num_items` pairs and stops).
///
/// Note: in the real object stream the pair sequence is delimited by
/// the parent object's total XData byte length; this signature takes
/// an explicit count so the decoder composes cleanly in tests and
/// higher-level walkers. Phase E+ will add a byte-delimited variant
/// keyed off the object's `xdata_size` field.
pub fn decode(c: &mut BitCursor<'_>, version: Version, num_items: usize) -> Result<XData> {
    if num_items > MAX_XDATA_ITEMS {
        return Err(Error::SectionMap(format!(
            "XData claims {num_items} items (>{MAX_XDATA_ITEMS} sanity cap)"
        )));
    }
    let app_handle = c.read_handle()?;
    let mut items = Vec::with_capacity(num_items);
    for _ in 0..num_items {
        let code = c.read_bs_u()?;
        items.push(decode_item(c, version, code)?);
    }
    Ok(XData { app_handle, items })
}

fn decode_item(c: &mut BitCursor<'_>, version: Version, code: u16) -> Result<XDataItem> {
    match code {
        1000..=1003 => {
            let value = read_tv(c, version)?;
            Ok(XDataItem::String { code, value })
        }
        1004 => {
            let chunk_count = c.read_rc()? as usize;
            let mut bytes = Vec::with_capacity(chunk_count);
            for _ in 0..chunk_count {
                bytes.push(c.read_rc()?);
            }
            Ok(XDataItem::Binary { bytes })
        }
        1005 => {
            let handle = c.read_handle()?;
            Ok(XDataItem::Handle { code, handle })
        }
        1010..=1013 => {
            let x = c.read_bd()?;
            let y = c.read_bd()?;
            let z = c.read_bd()?;
            Ok(XDataItem::Point {
                code,
                point: Point3D { x, y, z },
            })
        }
        1020..=1042 => {
            let value = c.read_bd()?;
            Ok(XDataItem::Real { code, value })
        }
        1070 => {
            let value = c.read_bs()?;
            Ok(XDataItem::Short { code, value })
        }
        1071 => {
            let value = c.read_bl()?;
            Ok(XDataItem::Long { code, value })
        }
        _ => Err(Error::SectionMap(format!(
            "XData group code {code} outside the 1000..=1071 range defined by spec §3.5"
        ))),
    }
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
    fn roundtrip_empty_xdata() {
        let mut w = BitWriter::new();
        w.write_handle(5, 0x2A);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000, 0).unwrap();
        assert_eq!(x.app_handle.value, 0x2A);
        assert!(x.items.is_empty());
    }

    #[test]
    fn roundtrip_mixed_items() {
        let mut w = BitWriter::new();
        w.write_handle(5, 0x3B);
        // 1000: string
        w.write_bs_u(1000);
        encode_tv_r2000(&mut w, b"hello");
        // 1070: short
        w.write_bs_u(1070);
        w.write_bs(42);
        // 1071: long
        w.write_bs_u(1071);
        w.write_bl(100_000);
        // 1040: real
        w.write_bs_u(1040);
        w.write_bd(2.5);
        // 1010: point
        w.write_bs_u(1010);
        w.write_bd(1.0);
        w.write_bd(2.0);
        w.write_bd(3.0);
        // 1005: handle
        w.write_bs_u(1005);
        w.write_handle(3, 0x7F);
        // 1004: binary
        w.write_bs_u(1004);
        w.write_rc(3);
        w.write_rc(0xDE);
        w.write_rc(0xAD);
        w.write_rc(0xBE);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let x = decode(&mut c, Version::R2000, 7).unwrap();
        assert_eq!(x.items.len(), 7);
        assert!(matches!(&x.items[0], XDataItem::String { value, .. } if value == "hello"));
        assert!(matches!(&x.items[1], XDataItem::Short { value: 42, .. }));
        assert!(matches!(
            &x.items[2],
            XDataItem::Long { value: 100_000, .. }
        ));
        assert!(
            matches!(&x.items[3], XDataItem::Real { value, .. } if (*value - 2.5).abs() < 1e-9)
        );
        assert!(matches!(
            &x.items[4],
            XDataItem::Point { point, .. } if point.x == 1.0 && point.y == 2.0 && point.z == 3.0
        ));
        assert!(matches!(&x.items[5], XDataItem::Handle { handle, .. } if handle.value == 0x7F));
        assert!(matches!(&x.items[6], XDataItem::Binary { bytes } if bytes == &[0xDE, 0xAD, 0xBE]));
    }

    #[test]
    fn rejects_excessive_item_count() {
        let mut w = BitWriter::new();
        w.write_handle(5, 0x01);
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000, MAX_XDATA_ITEMS + 1).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("XData")));
    }

    #[test]
    fn rejects_unknown_group_code() {
        let mut w = BitWriter::new();
        w.write_handle(5, 0x01);
        w.write_bs_u(999); // below the 1000-series range
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode(&mut c, Version::R2000, 1).unwrap_err();
        assert!(matches!(&err, Error::SectionMap(msg) if msg.contains("group code")));
    }
}
