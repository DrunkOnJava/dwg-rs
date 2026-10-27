//! The persistent-subentity managers AutoCAD hangs off a drawing's named
//! object dictionary: ACDBASSOCPERSSUBENTMANAGER and ACDBPERSSUBENTMANAGER.
//!
//! ODA spec v5.4.1 §20.4 prints neither. Their field lists are derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the
//! data bits of the seven corpus records of each class against the DXF object
//! of the same handle. ARCHITECTURE.md §7aq holds the evidence and says what
//! each field was checked against.
//!
//! ```text
//! ACDBPERSSUBENTMANAGER
//!   BL version                     DXF group 90, 2 in every record
//!   array x3                       DXF groups 90
//!
//! ACDBASSOCPERSSUBENTMANAGER
//!   BL version                     DXF group 90, 1 on R14 to R2010, 2 from R2013
//!   array ids                      DXF groups 90
//!   array x ids.len()              DXF groups 90, one array per id
//!   array                          version 2 only, DXF groups 90
//!   B   flag                       DXF group 290
//!
//! array = BL count, then count x BL
//! ```
//!
//! Every DXF group 90 of both objects is one `BL`, in the order the DWG holds
//! them, and the 290 is the final `B`. That much is checked by value on every
//! release the corpus holds (R2000 to R2018; R14 has no twin and is held to
//! its boundary alone). What the numbers mean, and so where each array ends,
//! is not shown by any group: the DXF writes all of them as 90. The array
//! grammar is the reading that keeps every repeat count in the stream and
//! closes all 14 records; two other readings parse the same bits (a fixed
//! number of arrays per version in place of one per id, and the version-2
//! array as a lone `BL` 0), so the array boundaries are unverified by value.
//! Where a width is only claimed, it is the `BL` form (group 90 is a 32-bit
//! integer); every value of the corpus is under 256, so `BS` is bit-identical
//! and the 32-bit form is never exercised.
//!
//! A version word other than the ones listed is refused with
//! [`Error::Unsupported`], never read on a guess. The DXF writes no
//! handle-valued group for either object beyond its owner and its one
//! reactor, so no handle is decoded here.

use crate::bitcursor::BitCursor;
use crate::error::{Error, Result};
use crate::objects::modern;
use crate::version::Version;

/// The class version `ACDBPERSSUBENTMANAGER` carries in every corpus record.
const PERS_VERSION: u32 = 2;

/// The class versions `ACDBASSOCPERSSUBENTMANAGER` carries in the corpus: 1
/// on R14 to R2010, 2 on R2013 and R2018.
const ASSOC_VERSION_FIRST: u32 = 1;
const ASSOC_VERSION_WITH_TAIL_ARRAY: u32 = 2;

/// ACDBPERSSUBENTMANAGER, derived from AutoCAD's output (ARCHITECTURE.md §7aq).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersSubentManager {
    /// `BL`, the first DXF group 90: 2 in every record.
    pub version: u32,
    /// Three arrays of `BL` (DXF groups 90). The first is empty in every
    /// record; the corpus holds `[] [4 0] [3 4]` on R14 to R2010 and
    /// `[] [0 0] []` on R2013 and R2018.
    pub arrays: Vec<Vec<u32>>,
}

/// ACDBASSOCPERSSUBENTMANAGER, derived from AutoCAD's output (ARCHITECTURE.md §7aq).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssocPersSubentManager {
    /// `BL`, the first DXF group 90: 1 on R14 to R2010, 2 on R2013 and R2018.
    pub version: u32,
    /// The ids array (DXF groups 90): `[0 2 4]` in every record.
    pub ids: Vec<u32>,
    /// One array per id, then at version 2 one more (DXF groups 90):
    /// `[] [] []` at version 1, `[] [3 4] [3 4] []` at version 2.
    pub arrays: Vec<Vec<u32>>,
    /// `B`, DXF group 290: false in every record.
    pub flag: bool,
}

/// Read one array: a `BL` count, then that many `BL`. The count is bounded by
/// the bits the payload still holds, two for each `BL`.
fn read_array(c: &mut BitCursor<'_>, what: &str) -> Result<Vec<u32>> {
    let count = c.read_bl_u()? as usize;
    if count > c.remaining_bits() / 2 {
        return Err(Error::SectionMap(format!(
            "{what} array of {count} values exceeds the {} bits the record holds",
            c.remaining_bits()
        )));
    }
    (0..count).map(|_| c.read_bl_u()).collect()
}

fn require_exhausted(
    strings: &Option<crate::string_stream::StringReader<'_>>,
    what: &str,
) -> Result<()> {
    match strings {
        Some(s) if !s.is_exhausted() => Err(Error::SectionMap(format!(
            "{what} string stream holds entries its field list never read"
        ))),
        _ => Ok(()),
    }
}

/// Decode an ACDBPERSSUBENTMANAGER record: `BL` version and three arrays of
/// `BL` (DXF groups 90), derived from AutoCAD's output (ARCHITECTURE.md §7aq),
/// ending on the data-stream boundary.
pub(crate) fn decode_pers_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<PersSubentManager> {
    const WHAT: &str = "ACDBPERSSUBENTMANAGER";
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let class_version = split.data.read_bl_u()?;
    if class_version != PERS_VERSION {
        return Err(Error::Unsupported {
            feature: format!("{WHAT} version {class_version}"),
        });
    }
    let mut arrays = Vec::with_capacity(3);
    for _ in 0..3 {
        arrays.push(read_array(&mut split.data, WHAT)?);
    }
    split.finish(WHAT)?;
    require_exhausted(&split.strings, WHAT)?;
    Ok(PersSubentManager {
        version: class_version,
        arrays,
    })
}

/// Decode an ACDBASSOCPERSSUBENTMANAGER record: `BL` version, an ids array of
/// `BL`, one array per id, at version 2 one more array, and a `B` (DXF groups
/// 90 and 290), derived from AutoCAD's output (ARCHITECTURE.md §7aq), ending
/// on the data-stream boundary.
pub(crate) fn decode_assoc_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AssocPersSubentManager> {
    const WHAT: &str = "ACDBASSOCPERSSUBENTMANAGER";
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let class_version = split.data.read_bl_u()?;
    if !matches!(
        class_version,
        ASSOC_VERSION_FIRST | ASSOC_VERSION_WITH_TAIL_ARRAY
    ) {
        return Err(Error::Unsupported {
            feature: format!("{WHAT} version {class_version}"),
        });
    }
    let ids = read_array(&mut split.data, WHAT)?;
    let mut arrays = Vec::with_capacity(ids.len() + 1);
    for _ in 0..ids.len() {
        arrays.push(read_array(&mut split.data, WHAT)?);
    }
    if class_version == ASSOC_VERSION_WITH_TAIL_ARRAY {
        arrays.push(read_array(&mut split.data, WHAT)?);
    }
    let flag = split.data.read_b()?;
    split.finish(WHAT)?;
    require_exhausted(&split.strings, WHAT)?;
    Ok(AssocPersSubentManager {
        version: class_version,
        ids,
        arrays,
        flag,
    })
}
