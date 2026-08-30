//! Shared helpers for ACIS-backed modeler entities (3DSOLID, BODY,
//! REGION, and the SURFACE family) — ODA Open Design Specification
//! v5.4.1 §20.4.41, which prescribes REGION (37), 3DSOLID (38) and
//! BODY (39) as one record shape.
//!
//! Autodesk stores parametric solids and surfaces as an opaque ACIS
//! SAT/SAB byte stream wrapped in a thin DWG-level envelope. The
//! low-level envelope decoder lives in
//! [`crate::entities::three_d_solid`]; this module owns
//! the rest of §20.4.41 — the wireframe / isoline / silhouette block,
//! the second ACIS-empty bit, the R2007+ trailing `BL`, and the R2013+
//! data-store block measured below.
//!
//! # Stream shape (§20.4.41, after the common entity data)
//!
//! ```text
//! B    ACIS empty                   -- 1 = no SAT/SAB data follows
//! (if !ACIS empty:)
//!   B    unknown
//!   BS   version                    -- 1 = length-prefixed blocks, 2 = raw ACIS file
//!   (version 1:) loop { BL block size; RC × block size }  until size == 0
//! B    wireframe data present
//! (if wireframe data present:)
//!   B    point present
//!   (if point present:) 3BD point
//!   BL   num isolines
//!   B    isolines present
//!   (if isolines present:)
//!     BL   num wires;        wire × num wires
//!     BL   num silhouettes;  silhouette × num silhouettes
//! B    ACIS empty (2)                -- "Normally 1"
//! (if !ACIS empty (2):) the envelope again, with no wireframe block
//! R2007+:
//! BL   unknown
//! ```
//!
//! A wire is `RC type, BL selection marker, BS colour, BL ACIS index,
//! BL point count, 3BD × point count, B transform present` and, when
//! the transform is present, `3BD × 4, BD scale, B × 3`. A silhouette
//! is `BL viewport id, 3BD × 3, B perspective, BL wire count` then that
//! many wires.
//!
//! # Measured: the R2013+ data-store block
//!
//! §24 of the same specification says the SAB stream of every ACIS
//! entity is moved into the data-storage section
//! (`AcDb:AcDsPrototype_1b`) as its own data record, and the record's
//! `has AcDs binary data` bit in the common entity data (§20.4.1) is
//! what flags it. All three ACIS records of the corpus —
//! `sample_AC1032.dwg` (R2018) 3DSOLID `0xD65` / `0xD6A` and REGION
//! `0xD69` — set that bit, and on all three the record carries **no
//! inline ACIS envelope at all** and **137 further bits** after the
//! `BL` §20.4.41 ends on:
//!
//! ```text
//! B     unknown_a                    -- 1 on all three
//! BB    unknown_b                    -- 0 on all three
//! BB    unknown_c
//! BB    unknown_d
//! RC×16 revision GUID
//! BL    unknown_e                    -- 0 on all three
//! ```
//!
//! Evidence, per record (bit offsets are from the start of the
//! payload; the common entity preamble ends at bit 82 on all three):
//!
//! | record | point | isolines | GUID | delta |
//! |---|---|---|---|---|
//! | 3DSOLID `0xD65` | `(17.7767…, -220.8501…, 2.5)` | 4 | `833111a1-b7ac-4dd4-824d-78b33668f9e7` | 0 |
//! | REGION `0xD69` | `(24.9857…, -220.4000…, 0)` | 4 | `2b80e3b3-b594-475e-8593-6a36b15e7945` | 0 |
//! | 3DSOLID `0xD6A` | `(31.6857…, -220.1073…, 2.1902…)` | 4 | `f21ff2a0-ff9c-4ed1-9b2e-7a1e6518d595` | 0 |
//!
//! Four independent corroborations, not just an arithmetic fit:
//!
//! 1. The three points are real drawing coordinates — three solids in
//!    a row at `y ≈ -220`, `x` stepping `17.8 → 25.0 → 31.7`.
//! 2. `num isolines` decodes `4` on every one — AutoCAD's default
//!    `ISOLINES` system-variable value.
//! 3. The 16 bytes are a valid RFC-4122 **version-4** UUID on all
//!    three (version nibble `4`, variant bits `10`). Scanning every
//!    128-bit window of all three records, bit offset `data_end - 130`
//!    is the only alignment where all three satisfy both. The chance
//!    of three independent windows doing so is 1 in 2^18.
//! 4. The rest of the list is §20.4.41's own grammar, decoding
//!    `wireframe present = 1`, `point present = 1`, `num wires = 0`,
//!    `num silhouettes = 0`, `ACIS empty (2) = 1` ("Normally 1" per the
//!    spec) and the R2007+ `BL` as `0`.
//!
//! What the corpus cannot say: whether the missing envelope and the
//! trailing block are conditional on the `has AcDs binary data` bit or
//! are unconditional R2013+ changes. No corpus file holds an ACIS
//! record with the bit clear. This module ties both to the bit, which
//! is the reading §24 supports; a flag-clear R2013+ record would fail
//! the boundary check rather than decode wrongly.
//!
//! The seven bits before the GUID are read here as `B, BB, BB, BB`.
//! Only their **total width** is measured — the three records agree on
//! `1, 0` for the first two slots and differ on the last two, and
//! nothing in them separates that tokenisation from `3B, BB, BB` or
//! from seven plain `B`s.
//!
//! `examples/probe_acis_census.rs` prints the census and the two
//! candidate readings of the AcDs marker side by side;
//! `examples/probe_acis_tail.rs` walks this grammar from every
//! candidate start bit and reports which ones land on delta 0.

use crate::bitcursor::BitCursor;
use crate::entities::{Point3D, read_bd3, three_d_solid};
use crate::error::{Error, Result};
use crate::version::Version;

/// Re-export of [`three_d_solid::MAX_SAT_BLOB_BYTES`] so SURFACE
/// decoders can reference a stable symbol without reaching into the
/// 3DSOLID module.
pub const MAX_SAT_BYTES: usize = three_d_solid::MAX_SAT_BLOB_BYTES;

/// Defensive cap on the `num wires` / `num silhouettes` / wire point
/// counts of the §20.4.41 wireframe block.
///
/// Every count in that block is a `BL`, so an adversarial record could
/// declare four billion wires. Real isoline sets on a solid are a
/// handful; 65 536 is orders of magnitude past anything a modeller
/// writes and still bounds the walk.
pub const MAX_MODELER_ELEMENTS: i32 = 65_536;

/// Width in bytes of the R2013+ revision GUID (§ measured — see module docs).
pub const REVISION_GUID_BYTES: usize = 16;

/// ACIS envelope data decoded from the stream.
///
/// `bytes` is the concatenation of every non-terminator chunk. If
/// `empty` is true, the drawing stored no SAT payload for this
/// entity (a legitimate state — some procedural surfaces retain only
/// their parametric definition, not a cached ACIS body, and from
/// R2013 the payload moves to the data-storage section entirely).
#[derive(Debug, Clone, PartialEq)]
pub struct SatBlob {
    /// Was the `acis_empty` flag set? If so, no payload followed.
    pub empty: bool,
    /// ACIS format version (e.g. `70` for ACIS 7.0). Undefined when
    /// `empty == true`.
    pub version: u16,
    /// Raw concatenated SAT bytes. May be XOR-masked per the ACIS
    /// format rules; this helper does not demask.
    pub bytes: Vec<u8>,
}

/// The §20.4.41 tail every ACIS entity writes after its envelope.
///
/// Per-wire and per-silhouette payloads are *stepped* rather than
/// surfaced: no corpus record carries any (`num_wires` and
/// `num_silhouettes` are `0` on all three), so structs for them would
/// be API that no byte has ever exercised. The counts are surfaced so
/// a caller can tell an empty block from an absent one.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelerTail {
    /// `wireframe data present` — true when the point/isoline block
    /// that follows was written.
    pub wireframe_present: bool,
    /// The wireframe block's point. `None` when the block is absent or
    /// its `point present` bit is clear (§20.4.41: "otherwise assume
    /// 0,0,0 for point").
    pub point: Option<Point3D>,
    /// `Num IsoLines` — AutoCAD's `ISOLINES` system variable, `4` by
    /// default. `0` when no wireframe block was written.
    pub num_isolines: i32,
    /// `IsoLines present` — whether the wire/silhouette lists follow.
    pub isolines_present: bool,
    /// Number of wires in the isoline list.
    pub num_wires: i32,
    /// Number of silhouettes in the silhouette list.
    pub num_silhouettes: i32,
    /// The second `ACIS empty` bit — §20.4.41 notes it is "normally 1".
    pub acis_empty_2: bool,
    /// R2013+ data-store records only: the 16-byte revision GUID.
    /// `None` when the record wrote no data-store block.
    pub revision_guid: Option<[u8; REVISION_GUID_BYTES]>,
}

/// One whole ACIS record: the envelope plus the §20.4.41 tail.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelerRecord {
    /// The record's inline ACIS envelope. On an R2013+ record whose
    /// `has AcDs binary data` bit is set there is none, and this is
    /// `SatBlob { empty: true, .. }`.
    pub blob: SatBlob,
    /// `true` when the SAB stream lives in the data-storage section
    /// (§24) rather than inline.
    pub in_data_store: bool,
    /// The §20.4.41 tail.
    pub tail: ModelerTail,
}

/// Decode one ACIS envelope by delegating to
/// `three_d_solid::read_sat_blob` and adapting the tuple result
/// into the [`SatBlob`] struct.
pub fn decode_sat_blob(c: &mut BitCursor<'_>) -> Result<SatBlob> {
    let (empty, version, bytes) = three_d_solid::read_sat_blob(c)?;
    Ok(SatBlob {
        empty,
        version: version.unwrap_or(0),
        bytes: bytes.unwrap_or_default(),
    })
}

/// Step one §20.4.41 wire struct, consuming its bits without
/// surfacing them.
fn step_wire(c: &mut BitCursor<'_>) -> Result<()> {
    let _wire_type = c.read_rc()?;
    let _selection_marker = c.read_bl()?;
    let _colour = c.read_bs()?;
    let _acis_index = c.read_bl()?;
    let point_count = bounded_count(c.read_bl()?, "wire point count")?;
    for _ in 0..point_count {
        let _ = read_bd3(c)?;
    }
    if c.read_b()? {
        // X / Y / Z axis, translation.
        for _ in 0..4 {
            let _ = read_bd3(c)?;
        }
        let _scale = c.read_bd()?;
        let _has_rotation = c.read_b()?;
        let _has_reflection = c.read_b()?;
        let _has_shear = c.read_b()?;
    }
    Ok(())
}

/// Reject a `BL` count that cannot describe a real wireframe block.
fn bounded_count(value: i32, what: &str) -> Result<i32> {
    if !(0..=MAX_MODELER_ELEMENTS).contains(&value) {
        return Err(Error::SectionMap(format!(
            "ACIS {what} is {value}, outside 0..={MAX_MODELER_ELEMENTS}"
        )));
    }
    Ok(value)
}

/// Read the §20.4.41 record body: the envelope, the wireframe block,
/// the second ACIS-empty bit, the R2007+ `BL`, and — when
/// `in_data_store` — the measured R2013+ data-store block.
///
/// `in_data_store` is the `has AcDs binary data` bit of the common
/// entity data ([`crate::common_entity::CommonEntityData::binary_chain`]).
/// See the module docs for what each branch is measured against.
pub fn decode_record(
    c: &mut BitCursor<'_>,
    version: Version,
    in_data_store: bool,
) -> Result<ModelerRecord> {
    let blob = if in_data_store {
        // §24.2.2.3: the SAB stream is a data-store record, and the
        // three corpus records write no inline envelope at all.
        SatBlob {
            empty: true,
            version: 0,
            bytes: Vec::new(),
        }
    } else {
        decode_sat_blob(c)?
    };

    let wireframe_present = c.read_b()?;
    let mut point = None;
    let mut num_isolines = 0;
    let mut isolines_present = false;
    let mut num_wires = 0;
    let mut num_silhouettes = 0;
    if wireframe_present {
        if c.read_b()? {
            point = Some(read_bd3(c)?);
        }
        num_isolines = bounded_count(c.read_bl()?, "isoline count")?;
        isolines_present = c.read_b()?;
        if isolines_present {
            num_wires = bounded_count(c.read_bl()?, "wire count")?;
            for _ in 0..num_wires {
                step_wire(c)?;
            }
            num_silhouettes = bounded_count(c.read_bl()?, "silhouette count")?;
            for _ in 0..num_silhouettes {
                let _viewport_id = c.read_bl()?;
                // Target, direction from target, up direction.
                for _ in 0..3 {
                    let _ = read_bd3(c)?;
                }
                let _perspective = c.read_b()?;
                let wires = bounded_count(c.read_bl()?, "silhouette wire count")?;
                for _ in 0..wires {
                    step_wire(c)?;
                }
            }
        }
    }

    let acis_empty_2 = c.read_b()?;
    if !acis_empty_2 {
        // §20.4.41: "acis data follows in the same format as described
        // above, except no wireframe or silhouette data will be
        // present". The envelope's own empty bit is not repeated here.
        let _unknown = c.read_b()?;
        let version_2 = c.read_bs_u()?;
        let _blob = three_d_solid::read_sat_blocks(c, version_2)?;
    }
    if version.is_r2007_plus() {
        let _unknown = c.read_bl()?;
    }

    let revision_guid = if in_data_store {
        let _unknown_a = c.read_b()?;
        let _unknown_b = c.read_bb()?;
        let _unknown_c = c.read_bb()?;
        let _unknown_d = c.read_bb()?;
        let mut guid = [0u8; REVISION_GUID_BYTES];
        for byte in guid.iter_mut() {
            *byte = c.read_rc()?;
        }
        let _unknown_e = c.read_bl()?;
        Some(guid)
    } else {
        None
    };

    Ok(ModelerRecord {
        blob,
        in_data_store,
        tail: ModelerTail {
            wireframe_present,
            point,
            num_isolines,
            isolines_present,
            num_wires,
            num_silhouettes,
            acis_empty_2,
            revision_guid,
        },
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Encode a SAT blob in the same wire shape the underlying
    /// `three_d_solid` decoder expects. Keeps the per-entity tests
    /// focused on their own fields without re-stating the chunk loop
    /// each time.
    pub(crate) fn write_sat_blob(w: &mut BitWriter, blob: &SatBlob) {
        if blob.empty {
            w.write_b(true);
            return;
        }
        w.write_b(false);
        w.write_b(false); // §20.4.41 unknown bit
        w.write_bs_u(blob.version);
        if !blob.bytes.is_empty() {
            w.write_bl(blob.bytes.len() as i32);
            for b in &blob.bytes {
                w.write_rc(*b);
            }
        }
        w.write_bl(0); // terminating block size
    }

    /// Write the §20.4.41 tail of a record that carries no wireframe
    /// block and no data-store block.
    pub(crate) fn write_minimal_tail(w: &mut BitWriter) {
        w.write_b(false); // wireframe data present
        w.write_b(true); // ACIS empty (2)
        w.write_bl(0); // R2007+ unknown
    }

    #[test]
    fn roundtrip_empty_blob() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: true,
                version: 0,
                bytes: Vec::new(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(b.empty);
        assert!(b.bytes.is_empty());
    }

    #[test]
    fn roundtrip_one_chunk_blob() {
        let payload = b"ACIS DUMMY BODY DATA".to_vec();
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 1,
                bytes: payload.clone(),
            },
        );
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let b = decode_sat_blob(&mut c).unwrap();
        assert!(!b.empty);
        assert_eq!(b.version, 1);
        assert_eq!(b.bytes, payload);
    }

    /// The data-store shape measured on `sample_AC1032.dwg`: no inline
    /// envelope, a wireframe block whose point is real geometry and
    /// whose isoline count is AutoCAD's default `4`, then the seven
    /// unknown bits, the revision GUID and the trailing `BL`.
    #[test]
    fn roundtrip_r2018_data_store_record() {
        let guid: [u8; 16] = [
            0x83, 0x31, 0x11, 0xA1, 0xB7, 0xAC, 0x4D, 0xD4, 0x82, 0x4D, 0x78, 0xB3, 0x36, 0x68,
            0xF9, 0xE7,
        ];
        let mut w = BitWriter::new();
        w.write_b(true); // wireframe data present
        w.write_b(true); // point present
        w.write_bd(17.776_725_469_823_76);
        w.write_bd(-220.850_122_660_715_93);
        w.write_bd(2.5);
        w.write_bl(4); // num isolines
        w.write_b(true); // isolines present
        w.write_bl(0); // num wires
        w.write_bl(0); // num silhouettes
        w.write_b(true); // ACIS empty (2)
        w.write_bl(0); // R2007+ unknown
        w.write_b(true); // unknown_a
        w.write_bb(0); // unknown_b
        w.write_bb(0); // unknown_c
        w.write_bb(0b10); // unknown_d
        for b in guid {
            w.write_rc(b);
        }
        w.write_bl(0); // unknown_e
        let end = w.position_bits();

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let rec = decode_record(&mut c, Version::R2018, true).unwrap();
        assert!(rec.in_data_store);
        assert!(rec.blob.empty);
        assert!(rec.tail.wireframe_present);
        assert_eq!(rec.tail.num_isolines, 4);
        assert!(rec.tail.isolines_present);
        assert_eq!(rec.tail.num_wires, 0);
        assert_eq!(rec.tail.num_silhouettes, 0);
        assert!(rec.tail.acis_empty_2);
        assert_eq!(rec.tail.revision_guid, Some(guid));
        let p = rec.tail.point.unwrap();
        assert_eq!(p.z, 2.5);
        assert_eq!(c.position_bits(), end);
    }

    /// A record with an inline envelope and no wireframe block closes
    /// on the same grammar with the data-store branch off.
    #[test]
    fn roundtrip_inline_envelope_record() {
        let mut w = BitWriter::new();
        write_sat_blob(
            &mut w,
            &SatBlob {
                empty: false,
                version: 1,
                bytes: b"400 0 1 0".to_vec(),
            },
        );
        write_minimal_tail(&mut w);
        let end = w.position_bits();

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let rec = decode_record(&mut c, Version::R2018, false).unwrap();
        assert!(!rec.blob.empty);
        assert_eq!(rec.blob.bytes, b"400 0 1 0".to_vec());
        assert!(!rec.tail.wireframe_present);
        assert!(rec.tail.acis_empty_2);
        assert_eq!(rec.tail.revision_guid, None);
        assert_eq!(c.position_bits(), end);
    }

    /// A wire list is stepped, not surfaced — but it must still be
    /// stepped exactly, or the record misses its boundary.
    #[test]
    fn wire_and_silhouette_lists_are_stepped_exactly() {
        let mut w = BitWriter::new();
        w.write_b(true); // ACIS empty — no inline envelope
        w.write_b(true); // wireframe data present
        w.write_b(false); // point present
        w.write_bl(4);
        w.write_b(true); // isolines present
        w.write_bl(1); // one wire
        w.write_rc(3); // wire type
        w.write_bl(7); // selection marker
        w.write_bs(256); // colour
        w.write_bl(2); // acis index
        w.write_bl(2); // point count
        for _ in 0..2 {
            w.write_bd(1.0);
            w.write_bd(2.0);
            w.write_bd(3.0);
        }
        w.write_b(true); // transform present
        for _ in 0..4 {
            w.write_bd(1.0);
            w.write_bd(0.0);
            w.write_bd(0.0);
        }
        w.write_bd(1.0); // scale
        w.write_b(false);
        w.write_b(false);
        w.write_b(false);
        w.write_bl(1); // one silhouette
        w.write_bl(11); // viewport id
        for _ in 0..3 {
            w.write_bd(0.0);
            w.write_bd(0.0);
            w.write_bd(1.0);
        }
        w.write_b(false); // perspective
        w.write_bl(0); // silhouette wire count
        w.write_b(true); // ACIS empty (2)
        w.write_bl(0); // R2007+ unknown
        let end = w.position_bits();

        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let rec = decode_record(&mut c, Version::R2018, false).unwrap();
        assert_eq!(rec.tail.num_wires, 1);
        assert_eq!(rec.tail.num_silhouettes, 1);
        assert_eq!(rec.tail.point, None);
        assert_eq!(c.position_bits(), end);
    }

    #[test]
    fn absurd_wire_count_is_rejected() {
        let mut w = BitWriter::new();
        w.write_b(true); // wireframe present
        w.write_b(false); // point present
        w.write_bl(4);
        w.write_b(true); // isolines present
        w.write_bl(1_000_000); // wire count
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let err = decode_record(&mut c, Version::R2018, true).unwrap_err();
        match err {
            Error::SectionMap(msg) => assert!(msg.contains("wire count"), "msg: {msg}"),
            other => panic!("expected SectionMap, got {other:?}"),
        }
    }
}
