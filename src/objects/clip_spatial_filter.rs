//! SPATIAL_FILTER (§20.4.94) — the clip boundary of an xref or block
//! insert.
//!
//! The class is `AcDbSpatialFilter`, DXF name `SPATIAL_FILTER`, a custom
//! class whose object type code is assigned per file through `AcDb:Classes`.
//! The filter is attached to the extension dictionary of an INSERT, under
//! `ACAD_FILTER`, and clips what the block shows to a polygon.
//!
//! # Field list
//!
//! §20.4.94 prints the data stream as:
//!
//! ```text
//! BS    number of points               DXF 70
//! 2RD   x number of points             DXF 10, 20   a point on the clip boundary
//! 3BD   extrusion                      DXF 210, 220, 230
//! 3BD   clip boundary origin           DXF 11, 21, 31
//! BS    display boundary               DXF 71
//! BS    front clip on                  DXF 72
//! BD    front clip distance            DXF 40       only when front clip is 1
//! BS    back clip on                   DXF 73
//! BD    back clip distance             DXF 40       only when back clip is 1
//! 12BD  inverse block transformation   DXF 40       x 12
//! 12BD  clip boundary transformation   DXF 40       x 12
//! ```
//!
//! # The matrices
//!
//! The spec calls each matrix "double [4][3], column major order". The 12
//! values arrive as three rows of four, the fourth value of each row being
//! the translation: the inverse block transformation of
//! `sample_AC1018.dwg`'s filter reads `25.4 0 0 -1087.95`, `0 25.4 0 942.62`,
//! `0 0 25.4 0`, an inch-to-millimetre scale and a shift, and its clip
//! boundary transformation is the identity, `1 0 0 0`, `0 1 0 0`, `0 0 1 0`.
//! They are kept in stream order, which is the order the DXF writes its
//! twenty-four groups 40.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

/// Bits of one `2RD` clip boundary point.
const POINT_BITS: usize = 128;

/// A decoded SPATIAL_FILTER.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpatialFilter {
    /// `2RD` points of the clip boundary, DXF 10 and 20.
    pub boundary: Vec<(f64, f64)>,
    /// `3BD` extrusion, DXF 210 to 230.
    pub extrusion: [f64; 3],
    /// `3BD` clip boundary origin, DXF 11 to 31.
    pub origin: [f64; 3],
    /// `BS` display boundary, DXF 71.
    pub display_boundary: i16,
    /// `BD` front clip distance, DXF 40, present when `BS` front clip on,
    /// DXF 72, is 1.
    pub front_clip: Option<f64>,
    /// `BD` back clip distance, DXF 40, present when `BS` back clip on,
    /// DXF 73, is 1.
    pub back_clip: Option<f64>,
    /// `12BD` inverse block transformation, three rows of four.
    pub inverse_block_transform: [f64; 12],
    /// `12BD` clip boundary transformation, three rows of four.
    pub clip_boundary_transform: [f64; 12],
}

fn read_3bd(c: &mut BitCursor<'_>) -> Result<[f64; 3]> {
    Ok([c.read_bd()?, c.read_bd()?, c.read_bd()?])
}

fn read_12bd(c: &mut BitCursor<'_>) -> Result<[f64; 12]> {
    let mut out = [0.0; 12];
    for v in &mut out {
        *v = c.read_bd()?;
    }
    Ok(out)
}

fn read_clip(c: &mut BitCursor<'_>) -> Result<Option<f64>> {
    match c.read_bs()? {
        1 => Ok(Some(c.read_bd()?)),
        _ => Ok(None),
    }
}

fn read_body(
    c: &mut BitCursor<'_>,
    _strings: &mut Option<StringReader<'_>>,
) -> Result<SpatialFilter> {
    let count = c.read_bs_u()? as usize;
    if count > c.remaining_bits() / POINT_BITS {
        return Err(Error::SectionMap(format!(
            "SPATIAL_FILTER claims {count} boundary points in {} remaining bits",
            c.remaining_bits()
        )));
    }
    let mut boundary = Vec::with_capacity(count);
    for _ in 0..count {
        boundary.push((c.read_rd()?, c.read_rd()?));
    }
    Ok(SpatialFilter {
        boundary,
        extrusion: read_3bd(c)?,
        origin: read_3bd(c)?,
        display_boundary: c.read_bs()?,
        front_clip: read_clip(c)?,
        back_clip: read_clip(c)?,
        inverse_block_transform: read_12bd(c)?,
        clip_boundary_transform: read_12bd(c)?,
    })
}

/// Decode a SPATIAL_FILTER from its raw object payload (§20.4.94), ending
/// exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<SpatialFilter> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read_body(&mut split.data, &mut split.strings)?;
    split.finish("SPATIAL_FILTER")?;
    Ok(out)
}
