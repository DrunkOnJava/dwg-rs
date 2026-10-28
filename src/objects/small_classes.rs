//! Five small custom classes the ODA specification never prints: the
//! WIPEOUTVARIABLES, ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION,
//! ACDB_BLOCKREPRESENTATION_DATA, PDFDEFINITION and DBCOLOR objects.
//!
//! ODA spec v5.4.1 §20.3 names none of them in §20.4. Their field lists are
//! derived from AutoCAD's own output under the method in `CLEANROOM.md`
//! (2026-10-26): the data bits of every corpus record against the DXF object
//! of the same handle. ARCHITECTURE.md §7bf holds the evidence and says what
//! each field was checked against.
//!
//! ```text
//! WIPEOUTVARIABLES                          BS display frame          (group 70)
//! ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION   BS version                (group 70)
//! ACDB_BLOCKREPRESENTATION_DATA             BS flag                   (group 70)
//!                                           [hard handle, handle stream, group 340]
//! PDFDEFINITION                             TV file name, TV page     (groups 1, 2)
//! DBCOLOR  R14 - R2000                      BS colour index
//!          R2004+                           BS index, BL colour word, RC flags,
//!                                           TV name if flag 1, TV book if flag 2
//! ```
//!
//! Every integer is read in the two short `BS` forms the corpus holds (the
//! zero code and the `RC` form); a wider form is refused with
//! [`Error::Unsupported`] rather than read on a guess. A colour flag other
//! than 0 to 3 is refused the same way.
//!
//! DBCOLOR from R2004 is a true colour: its colour word's low 24 bits are DXF
//! group 420 and the word is the xdata integer 1071. Its leading `BS` is 0 in
//! the corpus record, where DXF group 62 says 42 (the nearest index AutoCAD
//! derives from the RGB), so that `BS` is unverified by value from R2004.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::string_stream::StringReader;
use crate::version::Version;

fn unsupported(what: &str) -> Error {
    Error::Unsupported {
        feature: format!("{what} wider than the short BS forms"),
    }
}

fn read_small(c: &mut BitCursor<'_>, what: &str) -> Result<u8> {
    match c.read_bb()? {
        0b10 => Ok(0),
        0b01 => c.read_rc(),
        _ => Err(unsupported(what)),
    }
}

/// WIPEOUTVARIABLES: whether wipeout frames are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WipeoutVariables {
    /// `BS`, DXF group 70.
    pub display_frame: u8,
}

/// ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicBlockPurgePreventer {
    /// `BS`, DXF group 70.
    pub version: u8,
}

/// ACDB_BLOCKREPRESENTATION_DATA. Its hard handle (group 340) lives in the
/// handle stream and is not read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRepresentationData {
    /// `BS`, DXF group 70.
    pub flag: u8,
}

/// PDFDEFINITION: the PDF an underlay points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDefinition {
    /// `TV`, DXF group 1.
    pub file: String,
    /// `TV`, DXF group 2: the page, as text.
    pub page: String,
}

/// DBCOLOR: a named colour of a colour book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbColor {
    /// `BS` colour index. Before R2004 it is the whole record (42 in the
    /// corpus); from R2004 it is the leading `BS` of the colour, 0 there.
    pub index: u8,
    /// `BL` colour word, from R2004. DXF group 420 is its low 24 bits.
    pub color_word: Option<u32>,
    /// `TV` colour name when flag bit 0 is set, from R2004 (DXF 430 after `$`).
    pub name: Option<String>,
    /// `TV` colour book when flag bit 1 is set, from R2004 (DXF 430 before `$`).
    pub book: Option<String>,
}

fn open_and_finish<T>(
    what: &str,
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
    read: impl FnOnce(&mut BitCursor<'_>, &mut Option<StringReader<'_>>) -> Result<T>,
) -> Result<T> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let out = read(&mut split.data, &mut split.strings)?;
    split.finish(what)?;
    if let Some(strings) = split.strings.as_ref() {
        if !strings.is_exhausted() {
            return Err(Error::SectionMap(format!(
                "{what} string stream holds entries its field list never read"
            )));
        }
    }
    Ok(out)
}

/// Decode a WIPEOUTVARIABLES record: one `BS` (DXF group 70), derived from
/// AutoCAD's output (ARCHITECTURE.md §7bf), ending on the data-stream boundary.
pub(crate) fn decode_wipeout_variables(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<WipeoutVariables> {
    open_and_finish(
        "WIPEOUTVARIABLES",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(WipeoutVariables {
                display_frame: read_small(c, "WIPEOUTVARIABLES display frame")?,
            })
        },
    )
}

/// Decode an ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION record: one `BS` (DXF
/// group 70), derived from AutoCAD's output (ARCHITECTURE.md §7bf).
pub(crate) fn decode_dynamic_block_purge_preventer(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DynamicBlockPurgePreventer> {
    open_and_finish(
        "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(DynamicBlockPurgePreventer {
                version: read_small(c, "purge preventer version")?,
            })
        },
    )
}

/// Decode an ACDB_BLOCKREPRESENTATION_DATA record: one `BS` (DXF group 70)
/// and a hard handle in the handle stream, derived from AutoCAD's output
/// (ARCHITECTURE.md §7bf).
pub(crate) fn decode_block_representation_data(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<BlockRepresentationData> {
    open_and_finish(
        "ACDB_BLOCKREPRESENTATION_DATA",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            Ok(BlockRepresentationData {
                flag: read_small(c, "block representation flag")?,
            })
        },
    )
}

/// Decode a PDFDEFINITION record: `TV` file name and `TV` page (DXF groups 1
/// and 2), derived from AutoCAD's output (ARCHITECTURE.md §7bf).
pub(crate) fn decode_pdf_definition(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<PdfDefinition> {
    open_and_finish(
        "PDFDEFINITION",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let file = modern::read_tv(c, strings, version)?;
            let page = modern::read_tv(c, strings, version)?;
            Ok(PdfDefinition { file, page })
        },
    )
}

/// Decode a DBCOLOR record: a `BS` index before R2004, from R2004 a `BS`, a
/// `BL` colour word, an `RC` flag and the `TV`s the flag names (DXF groups
/// 420 and 430), derived from AutoCAD's output (ARCHITECTURE.md §7bf).
pub(crate) fn decode_db_color(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<DbColor> {
    open_and_finish(
        "DBCOLOR",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, strings| {
            let index = read_small(c, "DBCOLOR index")?;
            if !version.is_r2004_plus() {
                return Ok(DbColor {
                    index,
                    color_word: None,
                    name: None,
                    book: None,
                });
            }
            let color_word = c.read_bl_u()?;
            let flags = c.read_rc()?;
            if flags > 3 {
                return Err(Error::Unsupported {
                    feature: format!("DBCOLOR colour flag {flags}"),
                });
            }
            let name = if flags & 1 != 0 {
                Some(modern::read_tv(c, strings, version)?)
            } else {
                None
            };
            let book = if flags & 2 != 0 {
                Some(modern::read_tv(c, strings, version)?)
            } else {
                None
            };
            Ok(DbColor {
                index,
                color_word: Some(color_word),
                name,
                book,
            })
        },
    )
}
