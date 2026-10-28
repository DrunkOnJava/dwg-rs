//! 3DSOLID entity (§20.4.41) — ACIS-modeled 3D solid body.
//!
//! REGION (37), 3DSOLID (38) and BODY (39) are one entry in the ODA
//! Open Design Specification v5.4.1: §20.4.41 prescribes a single
//! record shape for all three. This module owns the ACIS *envelope*
//! — the empty bit, the unknown bit, the version and the
//! length-prefixed block loop — and [`crate::entities::modeler`] owns
//! the rest of the record (wireframe block, second empty bit, the
//! R2007+ `BL`, and the measured R2013+ data-store block).
//!
//! The concatenated block payloads form a Standard ACIS Text (SAT)
//! blob — a textual representation of the ACIS geometry kernel's B-rep
//! data. This decoder extracts the opaque blob and the version header
//! but does NOT parse the SAT text itself: SAT/SAB is a separate file
//! format with its own specification, and §24 of the ODA document is
//! explicit that "more detailed description of the ACIS/SAB data falls
//! outside the scope of this document". Callers who need solid
//! geometry should feed [`ThreeDSolid::sat_blob`] to a SAT parser
//! downstream.
//!
//! # Stream shape (§20.4.41 envelope)
//!
//! ```text
//! B   ACIS empty                  -- 1 bit: true = no data follows
//! (if !ACIS empty:)
//!   B   unknown
//!   BS  version                   -- 1 or 2
//!   (version 1:) repeat until block size is 0:
//!     BL  block size              -- bytes of SAT data in this block
//!     RC  data[block size]
//! ```
//!
//! Version 2 is followed by a raw ACIS file with no length field. A
//! binary (SAB) one opens with `ACIS BinaryFile` and ends with
//! `End\x0E\x02of\x0E\x04ACIS\x0D\x04data` (§20.4.41), so the envelope
//! reader reads it byte by byte up to that terminator. A text (SAT) one
//! "must be parsed to find the end", and is refused rather than guessed.
//!
//! Measured 2026-10-26 (#228): AutoCAD 2025's R2004, R2007 and R2010
//! saves of one drawing hold their two 3DSOLIDs and one REGION as
//! version-2 SAB envelopes. Read to the terminator, all nine records
//! close exactly on their data-stream boundary, and the §20.4.41 tail
//! after each envelope (wireframe point, isolines, wires, silhouettes)
//! is identical across the three releases; the SAB files themselves
//! differ only as AutoCAD writes a different ACIS file version for each
//! target release (ARCHITECTURE.md §7q).
//!
//! # Where the R2013+ payload actually lives
//!
//! From R2013 the SAB stream of every ACIS entity is moved out of the
//! entity record into the data-storage section
//! (`AcDb:AcDsPrototype_1b`, §24.2.2.3: "For each ACIS entity (REGION,
//! 3DSOLID), a data record is created with the SAB stream of the
//! object"), and the record's `has AcDs binary data` bit in the common
//! entity data flags it. All three ACIS records of this crate's corpus
//! are of that kind — see [`crate::entities::modeler`] for the
//! measurement.
//!
//! # Defensive cap
//!
//! Accumulated SAT payload is capped at [`MAX_SAT_BLOB_BYTES`] (32 MiB).
//! Real-world ACIS solids in DWG files typically fall under 1 MiB; the
//! 32 MiB ceiling is a generous safety bound chosen to accommodate
//! unusually complex assemblies while refusing adversarial input that
//! would force an unbounded allocation.

use crate::bitcursor::BitCursor;
use crate::entities::modeler::{self, ModelerRecord, ModelerTail};
use crate::error::{Error, Result};
use crate::version::Version;

/// Maximum accumulated SAT blob size across all chunks (32 MiB).
///
/// Exceeding this cap returns [`Error::SectionMap`]. See module docs
/// for rationale; also referenced by [`crate::entities::region`] and
/// [`crate::entities::body`] which share the same decoder.
pub const MAX_SAT_BLOB_BYTES: usize = 32 * 1024 * 1024;

/// 3DSOLID entity — §20.4.41.
///
/// `sat_blob` is `None` when the on-wire `acis_empty` flag is set.
/// When present, `sat_blob` is the concatenation of every block's raw
/// payload bytes in stream order and `version` is the ACIS format
/// version reported by the writer.
#[derive(Debug, Clone, PartialEq)]
pub struct ThreeDSolid {
    /// `true` when the entity was written with no inline body
    /// (`ACIS empty` bit set), which includes every R2013+ record
    /// whose payload moved to the data-storage section.
    pub acis_empty: bool,
    /// ACIS format version reported before the block loop, `1` or `2`.
    /// `None` when `acis_empty` is `true`.
    pub version: Option<u16>,
    /// Concatenated SAT payload bytes. `None` when `acis_empty` is
    /// `true`; `Some(vec![])` when the block loop terminated
    /// immediately (valid but unusual).
    pub sat_blob: Option<Vec<u8>>,
    /// R2013+: the record's `has AcDs binary data` bit was set, so the
    /// SAB stream is a data record in the data-storage section (§24).
    pub in_data_store: bool,
    /// The §20.4.41 tail — wireframe block, second ACIS-empty bit and
    /// the R2013+ revision GUID. `None` from the envelope-only
    /// [`decode`] entry point, which reads no tail.
    pub tail: Option<ModelerTail>,
}

/// Decode a 3DSOLID **envelope** per ODA spec v5.4.1 §20.4.41.
///
/// This reads only the ACIS envelope and leaves the cursor at the
/// start of the §20.4.41 tail; [`decode_record`] reads the whole
/// record and is what the dispatcher uses.
pub fn decode(c: &mut BitCursor<'_>) -> Result<ThreeDSolid> {
    let (acis_empty, version, sat_blob) = read_sat_blob(c)?;
    Ok(ThreeDSolid {
        acis_empty,
        version,
        sat_blob,
        in_data_store: false,
        tail: None,
    })
}

/// Build a 3DSOLID from a decoded [`ModelerRecord`].
pub fn from_record(record: ModelerRecord) -> ThreeDSolid {
    ThreeDSolid {
        acis_empty: record.blob.empty,
        version: (!record.blob.empty).then_some(record.blob.version),
        sat_blob: (!record.blob.empty).then_some(record.blob.bytes),
        in_data_store: record.in_data_store,
        tail: Some(record.tail),
    }
}

/// Decode a whole 3DSOLID record — envelope plus §20.4.41 tail.
pub fn decode_record(
    c: &mut BitCursor<'_>,
    version: Version,
    in_data_store: bool,
) -> Result<ThreeDSolid> {
    Ok(from_record(modeler::decode_record(
        c,
        version,
        in_data_store,
    )?))
}

/// Shared ACIS-envelope reader used by 3DSOLID, REGION, and BODY.
///
/// Returns `(acis_empty, version, blob)`. `version` and `blob` are
/// both `None` when `acis_empty` is `true`.
///
/// # Errors
///
/// - [`Error::Unsupported`] if a version-2 envelope holds a text (SAT)
///   file rather than a binary (SAB) one.
/// - [`Error::SectionMap`] if the envelope reports a version other than
///   1 or 2, if the accumulated blob exceeds [`MAX_SAT_BLOB_BYTES`], or
///   if a declared block size exceeds the remaining bytes in the cursor.
/// - Any [`BitCursor`] read error propagated from an underlying field.
pub(crate) fn read_sat_blob(c: &mut BitCursor<'_>) -> Result<(bool, Option<u16>, Option<Vec<u8>>)> {
    let acis_empty = c.read_b()?;
    if acis_empty {
        return Ok((true, None, None));
    }
    let _unknown = c.read_b()?;
    let version = c.read_bs_u()?;
    let blob = read_sat_blocks(c, version)?;
    Ok((false, Some(version), Some(blob)))
}

/// The bytes a version-2 SAB stream ends with (§20.4.41).
const SAB_END: &[u8] = b"End\x0E\x02of\x0E\x04ACIS\x0D\x04data";

/// The header a version-2 binary (SAB) stream opens with (§20.4.41).
const SAB_HEADER: &[u8] = b"ACIS BinaryFile";

/// Read the data of an ACIS envelope (§20.4.41).
///
/// Version 1 is a loop of a `BL` block size and that many `RC`s, ending
/// at a block size of `0`. Version 2 is an ACIS file written straight
/// into the stream with no length: a binary (SAB) one opens with
/// `ACIS BinaryFile` and ends with `End\x0E\x02of\x0E\x04ACIS\x0D\x04data`,
/// so it is read byte by byte up to that terminator. A version-2 text
/// (SAT) file "must be parsed to find the end", which this crate does
/// not do, so it is refused.
pub(crate) fn read_sat_blocks(c: &mut BitCursor<'_>, version: u16) -> Result<Vec<u8>> {
    match version {
        1 => {}
        2 => return read_sab_file(c),
        _ => {
            return Err(Error::SectionMap(format!(
                "ACIS envelope reports version {version}; §20.4.41 knows 1 and 2"
            )));
        }
    }
    let mut blob: Vec<u8> = Vec::new();
    loop {
        // `read_bl` returns i32; negative counts are invalid here.
        let block_size_signed = c.read_bl()?;
        if block_size_signed == 0 {
            return Ok(blob);
        }
        if block_size_signed < 0 {
            return Err(Error::SectionMap(format!(
                "SAT block size is negative ({block_size_signed}); \
                 entity stream is malformed"
            )));
        }
        let block_size = block_size_signed as usize;

        // A block size that exceeds the remaining cursor bytes cannot
        // be real. Each RC consumes 8 bits, so divide remaining bits by
        // 8 for the byte-level ceiling.
        let remaining_bytes = c.remaining_bits() / 8;
        if block_size > remaining_bytes {
            return Err(Error::SectionMap(format!(
                "SAT block size ({block_size}) exceeds remaining cursor \
                 bytes ({remaining_bytes})"
            )));
        }

        // Check the accumulated total before growing, so an over-sized
        // first block is rejected without a large allocation.
        if blob.len().saturating_add(block_size) > MAX_SAT_BLOB_BYTES {
            return Err(Error::SectionMap(format!(
                "SAT blob exceeds {MAX_SAT_BLOB_BYTES}-byte cap \
                 (accumulated {} + block {block_size})",
                blob.len()
            )));
        }

        blob.reserve(block_size);
        for _ in 0..block_size {
            blob.push(c.read_rc()?);
        }
    }
}

/// Read a version-2 ACIS envelope's binary (SAB) file up to and
/// including its terminator (§20.4.41).
fn read_sab_file(c: &mut BitCursor<'_>) -> Result<Vec<u8>> {
    let mut blob: Vec<u8> = Vec::new();
    while blob.len() < SAB_HEADER.len() {
        blob.push(c.read_rc()?);
    }
    if blob != SAB_HEADER {
        return Err(Error::Unsupported {
            feature: "a version-2 ACIS envelope holding a text (SAT) file, which §20.4.41 \
                      says must be parsed to find its end"
                .into(),
        });
    }
    loop {
        if blob.ends_with(SAB_END) {
            return Ok(blob);
        }
        if blob.len() >= MAX_SAT_BLOB_BYTES {
            return Err(Error::SectionMap(format!(
                "SAB stream exceeds the {MAX_SAT_BLOB_BYTES}-byte cap without its terminator"
            )));
        }
        blob.push(c.read_rc()?);
    }
}
