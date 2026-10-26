//! VIEWPORT ENTITY HEADER (§20.4.70, type 0x47) — the header the older
//! releases keep for each viewport entity of a paper space layout.
//!
//! Only R14 and R2000 drawings hold one; later releases do not. It is a
//! non-entity object whose data stream is the name and flags a symbol-table
//! entry opens with (§20.4.54), then one more flag:
//!
//! ```text
//! TV    entry name               DXF 2
//! B     64-flag                  DXF 70, the 64 bit
//! BS    xrefindex + 1            DXF 70; subtract one, -1 means not from an xref
//! B     xdep                     DXF 70, the 16 bit; dependent on an xref
//! B     1 flag                   DXF 70, the 1 bit
//! ```
//!
//! Its handle references (the viewport entity control, the extension
//! dictionary, the external reference block, the viewport entity it
//! describes and the previous header of the chain) are in the handle stream
//! and are not read here. The corpus's AutoCAD DXF exports hold no VIEWPORT
//! ENTITY HEADER, so its records are checked against their data-stream
//! boundary alone.

use crate::error::Result;
use crate::objects::modern;
use crate::version::Version;

/// A decoded VIEWPORT ENTITY HEADER.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VpEntHdr {
    /// `TV` entry name, DXF 2.
    pub name: String,
    /// `B` the 64 bit of DXF group 70.
    pub flag_64: bool,
    /// `BS` xrefindex + 1.
    pub xref_index_plus_1: i16,
    /// `B` xdep: dependent on an external reference.
    pub is_xref_dependent: bool,
    /// `B` the 1 bit of DXF group 70.
    pub flag_1: bool,
}

/// Decode a VIEWPORT ENTITY HEADER (§20.4.70) from its raw object payload,
/// checking the data fields end exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<VpEntHdr> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = VpEntHdr {
        name: modern::read_tv(&mut split.data, &mut split.strings, version)?,
        flag_64: split.data.read_b()?,
        xref_index_plus_1: split.data.read_bs()?,
        is_xref_dependent: split.data.read_b()?,
        flag_1: split.data.read_b()?,
    };
    split.finish("VP_ENT_HDR")?;
    Ok(out)
}
