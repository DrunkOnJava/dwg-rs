//! The `CMC` colour form the R2004+ non-entity objects carry.
//!
//! §2.11 of the ODA *Open Design Specification for .dwg files* v5.4.1
//! describes `CMC` as a `BS` colour index optionally followed by a `BL`
//! true-colour word and an `RC` colour byte. Every R2004+ record this
//! crate has measured writes all three unconditionally — see the
//! evidence note on `crate::tables::modern::read_cmc_full` — so the
//! crate-internal reader here reads all three and then honours the two
//! name-string bits of the colour byte, which are the one part of
//! §2.11 that really is conditional.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::string_stream::StringReader;
use crate::version::Version;

/// One `CMC` colour: `BS` index, `BL` true-colour word, `RC` colour byte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectColor {
    /// Colour index word.
    pub index: u16,
    /// True-colour word; the top octet is the colour method
    /// (`0xC0` ByLayer, `0xC1` ByBlock, `0xC2` RGB, `0xC3` ACI index,
    /// `0xC8` none).
    pub rgb: u32,
    /// Trailing colour byte; bit 0 introduces a colour name, bit 1 a
    /// book name.
    pub color_byte: u8,
    /// Colour name, when bit 0 of [`color_byte`](Self::color_byte) is set.
    pub color_name: String,
    /// Book name, when bit 1 of [`color_byte`](Self::color_byte) is set.
    pub book_name: String,
}

impl ObjectColor {
    /// The colour method octet — the top byte of the true-colour word.
    pub fn method(&self) -> u8 {
        (self.rgb >> 24) as u8
    }

    /// The 24-bit payload of the true-colour word: an RGB triple when
    /// [`method`](Self::method) is `0xC2`, an ACI index when it is `0xC3`.
    pub fn payload(&self) -> u32 {
        self.rgb & 0x00FF_FFFF
    }
}

/// Read one `CMC` (§2.11), taking any name strings from `strings` on
/// the R2007+ split layout and inline before it.
pub(crate) fn read(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<ObjectColor> {
    let index = c.read_bs_u()?;
    if !version.is_r2004_plus() {
        return Ok(ObjectColor {
            index,
            ..ObjectColor::default()
        });
    }
    let rgb = c.read_bl_u()?;
    let color_byte = c.read_rc()?;
    let color_name = if color_byte & 1 != 0 {
        super::modern::read_tv(c, strings, version)?
    } else {
        String::new()
    };
    let book_name = if color_byte & 2 != 0 {
        super::modern::read_tv(c, strings, version)?
    } else {
        String::new()
    };
    Ok(ObjectColor {
        index,
        rgb,
        color_byte,
        color_name,
        book_name,
    })
}
