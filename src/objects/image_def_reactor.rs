//! IMAGEDEF_REACTOR (§20.4.82 IMAGEDEFREACTOR) — the reactor an IMAGEDEF
//! keeps for each IMAGE that uses it.
//!
//! The class is `AcDbRasterImageDefReactor`, DXF name `IMAGEDEF_REACTOR`
//! (the spec spells it `IMAGEDEFREACTOR`). It is a custom class, so its
//! object type code is assigned per file through `AcDb:Classes` and the
//! dispatcher resolves it by DXF class name. The record is the common object
//! data and one field, the class version (`BL`, DXF 90); its parent handle is
//! the IMAGE entity the reactor serves, in the handle stream.
//!
//! The one drawing of every release in the public corpus that holds an IMAGE
//! (ACadSharp's `sample_AC10xx` set) holds one reactor, and each closes on
//! its data-stream boundary from R14 to R2018.

use crate::error::Result;
use crate::objects::modern;
use crate::version::Version;

/// A decoded IMAGEDEF_REACTOR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageDefReactor {
    /// `BL` class version, DXF 90.
    pub class_version: u32,
}

/// Decode an IMAGEDEF_REACTOR from its raw object payload (§20.4.82),
/// checking its one `BL` ends exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<ImageDefReactor> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let class_version = split.data.read_bl_u()?;
    split.finish("IMAGEDEF_REACTOR")?;
    Ok(ImageDefReactor { class_version })
}
