//! ACAD_MATERIAL object — rendering material definition.
//!
//! # What this module does and does not claim
//!
//! It reads a MATERIAL record's **strings** — the material name, its
//! description, and the further string-stream entries R2018 records
//! carry — and reports the **measured bit budget** of the record's data
//! fields. It does not decode those data fields, because their layout
//! is not determined, and it is therefore deliberately **not wired into
//! the object dispatcher**: a MATERIAL record still counts as
//! `Unhandled`, never as decoded.
//!
//! # There is no spec prescription for this object
//!
//! The ODA *Open Design Specification for .dwg files* v5.4.1 lists
//! `MATERIAL` in §20.3's table of non-fixed object types, but §20.4 —
//! the object-prescription chapter — has no entry for it; it runs from
//! `20.4.1 Common Entity Data` to `20.4.104 XRECORD` and stops. (An
//! earlier revision of this module cited "§19.6.9 (L6-16)"; no such
//! section exists.)
//!
//! # Why the previous field list was withdrawn
//!
//! That revision read `BL ambient_color_method, BS ambient_color,
//! BD ambient_color_factor, …` off the front of the record. Measured
//! against `arc_2004.dwg` handle 17 — the `ByLayer` material, whose two
//! `TV`s occupy bits 0..76 of the body — that list decodes
//! `ambient_color_method = 542113793` and `ambient_color = 17728`,
//! followed by eight consecutive `BD = 1.0`. Those are not colour
//! methods and colour indices; the list was wrong from its first field.
//!
//! # What the bytes do say — measured
//!
//! | File | Records | Data-field budget | Strings |
//! |---|---|---|---|
//! | `arc_2004.dwg` | 3 (`ByLayer`, `ByBlock`, `Global`) | 1340 / 1340 / 1332 bits, inclusive of the two inline `TV`s (1264 bits after them, identical in all three) | 2, inline |
//! | `arc_2010.dwg` | 3 | 1284 bits, bit-identical in all three | 2 |
//! | `arc_2013.dwg` | 3 | 1284 bits, bit-identical in all three and to R2010 | 2 |
//! | `sample_AC1032.dwg` | 3 | 516 / 516 / 1028 bits | 7 |
//!
//! Inside the R2010/R2013 body, twelve `BD` slots decode to exactly
//! `1/48` (`0.0208333…`, the imperial default map scale) at data-stream
//! bits 46, 120, 250, 324, 510, 584, 706, 780, 900, 974, 1096 and 1170
//! — six pairs, one per texture map, each pair 74 bits apart. The
//! `sample_AC1032.dwg` `Global` record shows the same shape with four
//! pairs, and each of its pairs is bracketed by `CMC` words
//! `0xC1000000` and `0xC2040300`. Its seven string-stream entries — the
//! name plus six empties — corroborate six map slots.
//!
//! That is enough to say where the per-map blocks are and not enough to
//! say what is inside them, and the gap between the last `1/48` pair and
//! the record's boundary differs from block to block (64, 120, 56, 54,
//! 56 and 48 bits on R2010), so no single repeating block layout has
//! been pinned. Per this crate's honesty rule a record counts as decoded
//! only when its fields end exactly on the string-stream start bit, so
//! MATERIAL stays undecoded until that layout is measured rather than
//! guessed.

use crate::error::Result;
use crate::objects::modern;
use crate::version::Version;

/// The parts of an ACAD_MATERIAL record this crate can prove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcadMaterial {
    /// Material name — `ByLayer`, `ByBlock` and `Global` in every
    /// corpus file. Taken from the R2007+ string stream.
    pub name: String,
    /// Material description; empty in every corpus record.
    pub description: String,
    /// Further string-stream entries. R2018 records carry six empty
    /// slots here, one per texture map.
    pub extra_strings: Vec<String>,
    /// Bits between the end of the common object data and the record's
    /// data-stream boundary — the budget a future field list has to
    /// fill exactly. `None` on R13/R14, which carry no boundary.
    pub data_field_bits: Option<usize>,
}

/// Read the strings and the measured data-field budget of one
/// ACAD_MATERIAL record.
///
/// This does **not** decode the record's data fields and does not
/// satisfy the crate-internal `ObjectStream::finish` boundary check, so
/// it is not dispatched; see the module docs for what is and is not known.
pub fn read_strings(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<AcadMaterial> {
    let mut split = modern::open(payload, body_start, inline_data_end, version)?;
    let name = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let description = modern::read_tv(&mut split.data, &mut split.strings, version)?;
    let data_field_bits = split
        .data_end()
        .map(|end| end.saturating_sub(split.data.position_bits()));
    let mut extra_strings = Vec::new();
    if let Some(strings) = split.strings.as_mut() {
        while !strings.is_exhausted() {
            match strings.read_tv() {
                Ok(text) => extra_strings.push(text),
                Err(_) => break,
            }
        }
    }
    Ok(AcadMaterial {
        name,
        description,
        extra_strings,
        data_field_bits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    #[test]
    fn r2018_split_stream_material_reads_its_names_from_the_string_stream() {
        let mut body = modern::tests::r2018_object_prefix(1);
        // Stand-in for the undecoded data fields.
        for _ in 0..40 {
            body.write_b(false);
        }
        let bits = crate::string_stream::tests::bits_of(&body);
        let payload =
            crate::string_stream::tests::build_payload(&bits, &["Global", "", "", "", "", "", ""]);
        let material = read_strings(&payload, 8, None, Version::R2018).unwrap();
        assert_eq!(material.name, "Global");
        assert_eq!(material.description, "");
        assert_eq!(material.extra_strings.len(), 5);
        assert_eq!(material.data_field_bits, Some(40));
    }

    #[test]
    fn inline_layout_reads_its_names_from_the_data_cursor() {
        let mut w = BitWriter::new();
        w.write_bs_u(0); // EED terminator
        w.write_bl(1); // num_reactors
        w.write_b(true); // no xdictionary (R2004+)
        w.write_bs_u(8);
        for b in b"ByLayer\0" {
            w.write_rc(*b);
        }
        w.write_bs_u(0); // empty description
        let after_names = w.position_bits();
        for _ in 0..24 {
            w.write_b(false);
        }
        let end = w.position_bits();
        let bytes = w.into_bytes();
        let material = read_strings(&bytes, 0, Some(end), Version::R2004).unwrap();
        assert_eq!(material.name, "ByLayer");
        assert_eq!(material.description, "");
        assert!(material.extra_strings.is_empty());
        assert_eq!(material.data_field_bits, Some(end - after_names));
    }
}
