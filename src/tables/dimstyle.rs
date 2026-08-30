//! DIMSTYLE table entry (ODA Open Design Specification v5.4.1 §19.5.5,
//! L6-05) — dimension style.
//!
//! DIMSTYLE carries ~75 dimension-variable fields (dimscale, dimasz,
//! dimexo, dimexe, dimtxt, dimcen, ...). Covering 100% of them is
//! mostly mechanical — the full list is mirrored in the AutoCAD DIMVAR
//! table — and balloons the decoder without proportional value for
//! typical use.
//!
//! This decoder implements the 15 most-consulted fields in spec order
//! and surfaces them as a [`DimStyleEntry`]:
//!
//! | Slot | Field     | Type |
//! |------|-----------|------|
//! | 1    | dimscale  | BD   |
//! | 2    | dimasz    | BD   |
//! | 3    | dimexo    | BD   |
//! | 4    | dimexe    | BD   |
//! | 5    | dimtxt    | BD   |
//! | 6    | dimcen    | BD   |
//! | 7    | dimtfac   | BD   |
//! | 8    | dimlfac   | BD   |
//! | 9    | dimtih    | B    |
//! | 10   | dimtoh    | B    |
//! | 11   | dimtad    | RC   |
//! | 12   | dimtolj   | RC   |
//! | 13   | dimaltf   | BD   |
//! | 14   | dimaltrnd | BD   |
//! | 15   | dimupt    | B    |
//!
//! # Cutoff
//!
//! Fields past `dimupt` are left in the stream — callers that need the
//! full record must layer a more specific decoder on top. This is a
//! deliberate scope cut: §19.5.5 has no stable layout change between
//! R2000 and R2018 for these 15 slots, but fields past slot 15 shift
//! position by version and are best read as a version-gated second
//! pass.

use crate::bitcursor::BitCursor;
use crate::error::Result;
use crate::tables::{TableEntryHeader, modern, read_table_entry_header};
use crate::version::Version;

/// Partial DIMSTYLE: 15 rendering-essential dimension variables plus the
/// entry header. See module docstring for the cutoff rationale.
#[derive(Debug, Clone, PartialEq)]
pub struct DimStyleEntry {
    pub header: TableEntryHeader,
    pub dimscale: f64,
    pub dimasz: f64,
    pub dimexo: f64,
    pub dimexe: f64,
    pub dimtxt: f64,
    pub dimcen: f64,
    pub dimtfac: f64,
    pub dimlfac: f64,
    pub dimtih: bool,
    pub dimtoh: bool,
    pub dimtad: u8,
    pub dimtolj: u8,
    pub dimaltf: f64,
    pub dimaltrnd: f64,
    pub dimupt: bool,
}

// Legacy alias retained so callers keep compiling while they migrate to
// [`DimStyleEntry`].
pub type DimStyle = DimStyleEntry;

/// Decodes a `DimStyleEntry` table entry that follows the common object header.
///
/// This is a **partial** reader: it takes the first fifteen dimension
/// variables in whatever order an earlier revision recorded, and stops.
/// It cannot satisfy the record's data-stream boundary, so the
/// dispatcher does not use it on real files — see
/// [`decode_r2000_inline`], which reads the whole §20.4.68 R2000+ body,
/// and `decode_modern_split_stream` for R2007+. It is kept for callers
/// that only want the rendering-essential variables out of a synthetic
/// stream.
pub fn decode(c: &mut BitCursor<'_>, version: Version) -> Result<DimStyleEntry> {
    let header = read_table_entry_header(c, version)?;
    let dimscale = c.read_bd()?;
    let dimasz = c.read_bd()?;
    let dimexo = c.read_bd()?;
    let dimexe = c.read_bd()?;
    let dimtxt = c.read_bd()?;
    let dimcen = c.read_bd()?;
    let dimtfac = c.read_bd()?;
    let dimlfac = c.read_bd()?;
    let dimtih = c.read_b()?;
    let dimtoh = c.read_b()?;
    let dimtad = c.read_rc()?;
    let dimtolj = c.read_rc()?;
    let dimaltf = c.read_bd()?;
    let dimaltrnd = c.read_bd()?;
    let dimupt = c.read_b()?;
    Ok(DimStyleEntry {
        header,
        dimscale,
        dimasz,
        dimexo,
        dimexe,
        dimtxt,
        dimcen,
        dimtfac,
        dimlfac,
        dimtih,
        dimtoh,
        dimtad,
        dimtolj,
        dimaltf,
        dimaltrnd,
        dimupt,
    })
}

/// Read a `CMC` colour field in the release's inline form (§2.11).
///
/// "R15 and earlier: BS color index"; from R2004 the same slot is `BS`
/// index + `BL` true-colour word + `RC` colour byte, with an optional
/// colour name and book name selected by the byte's low two bits.
fn read_cmc_inline(c: &mut BitCursor<'_>, version: Version) -> Result<i16> {
    let index = c.read_bs()?;
    if !version.is_r2004_plus() {
        return Ok(index);
    }
    let _rgb = c.read_bl_u()?;
    let color_byte = c.read_rc()?;
    if color_byte & 0x01 != 0 {
        let _name = crate::tables::read_tv(c, version)?;
    }
    if color_byte & 0x02 != 0 {
        let _book = crate::tables::read_tv(c, version)?;
    }
    Ok(index)
}

/// Decode the whole §20.4.68 "R2000+" DIMSTYLE body inline — the
/// pre-R2007 counterpart of `decode_modern_split_stream`.
///
/// The two field lists are the same list; the only differences are that
/// `DIMPOST` / `DIMAPOST` cost real bits here (they are `TV`s in the
/// data stream rather than slots in a string stream), that the `CMC`
/// colours take the release's inline form, and that the R2007+ and
/// R2010+ blocks are absent.
///
/// # Measured
///
/// Reading the list below lands both DIMSTYLE records of every R2000
/// and R2004 corpus file exactly on their `RL` data-stream boundary
/// (delta 0), where the fifteen-field [`decode`] left 298-440 bits
/// unread. The decoded `ISO-25` values agree field-for-field with what
/// the R2007+ path reads from the same drawing saved as `line_2013.dwg`
/// — dimscale 1.0, dimasz 2.5, dimexo 0.625, dimexe 1.25, dimtxt 2.5,
/// dimcen 2.5.
pub fn decode_r2000_inline(c: &mut BitCursor<'_>, version: Version) -> Result<DimStyleEntry> {
    let header = read_table_entry_header(c, version)?;
    let _dimpost = crate::tables::read_tv(c, version)?;
    let _dimapost = crate::tables::read_tv(c, version)?;
    let dimscale = c.read_bd()?;
    let dimasz = c.read_bd()?;
    let dimexo = c.read_bd()?;
    let _dimdli = c.read_bd()?;
    let dimexe = c.read_bd()?;
    let _dimrnd = c.read_bd()?;
    let _dimdle = c.read_bd()?;
    let _dimtp = c.read_bd()?;
    let _dimtm = c.read_bd()?;
    let _dimtol = c.read_b()?;
    let _dimlim = c.read_b()?;
    let dimtih = c.read_b()?;
    let dimtoh = c.read_b()?;
    let _dimse1 = c.read_b()?;
    let _dimse2 = c.read_b()?;
    let dimtad = c.read_bs()?;
    let _dimzin = c.read_bs()?;
    let _dimazin = c.read_bs()?;
    let dimtxt = c.read_bd()?;
    let dimcen = c.read_bd()?;
    let _dimtsz = c.read_bd()?;
    let dimaltf = c.read_bd()?;
    let dimlfac = c.read_bd()?;
    let _dimtvp = c.read_bd()?;
    let dimtfac = c.read_bd()?;
    let _dimgap = c.read_bd()?;
    let dimaltrnd = c.read_bd()?;
    let _dimalt = c.read_b()?;
    let _dimaltd = c.read_bs()?;
    let _dimtofl = c.read_b()?;
    let _dimsah = c.read_b()?;
    let _dimtix = c.read_b()?;
    let _dimsoxd = c.read_b()?;
    let _dimclrd = read_cmc_inline(c, version)?;
    let _dimclre = read_cmc_inline(c, version)?;
    let _dimclrt = read_cmc_inline(c, version)?;
    let _dimadec = c.read_bs()?;
    let _dimdec = c.read_bs()?;
    let _dimtdec = c.read_bs()?;
    let _dimaltu = c.read_bs()?;
    let _dimalttd = c.read_bs()?;
    let _dimaunit = c.read_bs()?;
    let _dimfrac = c.read_bs()?;
    let _dimlunit = c.read_bs()?;
    let _dimdsep = c.read_bs()?;
    let _dimtmove = c.read_bs()?;
    let _dimjust = c.read_bs()?;
    let _dimsd1 = c.read_b()?;
    let _dimsd2 = c.read_b()?;
    let dimtolj = c.read_bs()?;
    let _dimtzin = c.read_bs()?;
    let _dimaltz = c.read_bs()?;
    let _dimalttz = c.read_bs()?;
    let dimupt = c.read_b()?;
    let _dimfit = c.read_bs()?;
    let _dimlwd = c.read_bs()?;
    let _dimlwe = c.read_bs()?;
    // One further bit that §20.4.68 does not list. The R2007+ path in
    // this module reads the same bit after `DIMLWE`, measured on
    // R2013/R2018; without it all four R2000 and R2004 DIMSTYLE records
    // of the corpus land exactly one bit short of their boundary, and
    // with it all four close on delta 0. Two release bands agreeing on
    // an undocumented trailing bit is the strongest statement available
    // about it, so it is consumed and not named.
    let _unknown = c.read_b()?;
    Ok(DimStyleEntry {
        header,
        dimscale,
        dimasz,
        dimexo,
        dimexe,
        dimtxt,
        dimcen,
        dimtfac,
        dimlfac,
        dimtih,
        dimtoh,
        dimtad: dimtad as u8,
        dimtolj: dimtolj as u8,
        dimaltf,
        dimaltrnd,
        dimupt,
    })
}

/// Decode an R2007+ DIMSTYLE whose `TV` fields live in the object's
/// string stream (ODA v5.4.1 §19.1 split layout, §20.4.5 DIMSTYLE
/// field table).
///
/// Unlike the pre-R2007 [`decode`], this reads the *whole* ~70-field
/// dimension-variable body — not because every variable is surfaced
/// (only the 15 in [`DimStyleEntry`] are) but because
/// [`modern::SplitStream::finish`] can only verify the record when the
/// body is consumed to its last bit. The consumed-but-unsurfaced
/// variables keep their spec names in local bindings so the sequence
/// stays auditable against the field table.
///
/// # Reconstructed from bytes
///
/// The `ISO-25` record of `line_2013.dwg` decodes to the published
/// ISO-25 defaults across the whole body: dimscale 1.0, dimasz 2.5,
/// dimexo 0.625, dimdli 3.75, dimexe 1.25, dimfxl 1.0, dimtad 1,
/// dimzin 8, dimtxt 2.5, dimcen 2.5, dimaltf 1/25.4, dimgap 0.625,
/// dimaltd 3, dimtofl true, dimdec 2, dimtdec 2, dimlunit 2, dimdsep
/// 44 (`.`), dimtzin 8, dimatfit 3, dimaltmzf 100.0, dimmzf 100.0,
/// and the trailing dimlwd / dimlwe both -2 (ByBlock). The
/// text-fill / dimension-line / extension-line / text colours all use
/// the full `BS`/`BL`/`RC` `CMC` form, as in VIEW and VPORT.
pub(crate) fn decode_modern_split_stream(
    payload: &[u8],
    object_body_start: usize,
    version: Version,
) -> Result<DimStyleEntry> {
    let mut split = modern::open_table_entry(payload, object_body_start, version)?;
    let (flag64, xref_index_plus_1, is_xref_dependent) = modern::read_entry_flags(&mut split.data)?;
    let c = &mut split.data;
    let dimscale = c.read_bd()?;
    let dimasz = c.read_bd()?;
    let dimexo = c.read_bd()?;
    let _dimdli = c.read_bd()?;
    let dimexe = c.read_bd()?;
    let _dimrnd = c.read_bd()?;
    let _dimdle = c.read_bd()?;
    let _dimtp = c.read_bd()?;
    let _dimtm = c.read_bd()?;
    let _dimfxl = c.read_bd()?;
    let _dimjogang = c.read_bd()?;
    let _dimtfill = c.read_bs()?;
    let _dimtfillclr = modern::read_cmc_full(c)?;
    let _dimtol = c.read_b()?;
    let _dimlim = c.read_b()?;
    let dimtih = c.read_b()?;
    let dimtoh = c.read_b()?;
    let _dimse1 = c.read_b()?;
    let _dimse2 = c.read_b()?;
    let dimtad = c.read_bs()?;
    let _dimzin = c.read_bs()?;
    let _dimazin = c.read_bs()?;
    let _dimarcsym = c.read_bs()?;
    let dimtxt = c.read_bd()?;
    let dimcen = c.read_bd()?;
    let _dimtsz = c.read_bd()?;
    let dimaltf = c.read_bd()?;
    let dimlfac = c.read_bd()?;
    let _dimtvp = c.read_bd()?;
    let dimtfac = c.read_bd()?;
    let _dimgap = c.read_bd()?;
    let dimaltrnd = c.read_bd()?;
    let _dimalt = c.read_b()?;
    let _dimaltd = c.read_bs()?;
    let _dimtofl = c.read_b()?;
    let _dimsah = c.read_b()?;
    let _dimtix = c.read_b()?;
    let _dimsoxd = c.read_b()?;
    let _dimclrd = modern::read_cmc_full(c)?;
    let _dimclre = modern::read_cmc_full(c)?;
    let _dimclrt = modern::read_cmc_full(c)?;
    let _dimadec = c.read_bs()?;
    let _dimdec = c.read_bs()?;
    let _dimtdec = c.read_bs()?;
    let _dimaltu = c.read_bs()?;
    let _dimalttd = c.read_bs()?;
    let _dimaunit = c.read_bs()?;
    let _dimfrac = c.read_bs()?;
    let _dimlunit = c.read_bs()?;
    let _dimdsep = c.read_bs()?;
    let _dimtmove = c.read_bs()?;
    let _dimjust = c.read_bs()?;
    let _dimsd1 = c.read_b()?;
    let _dimsd2 = c.read_b()?;
    let dimtolj = c.read_bs()?;
    let _dimtzin = c.read_bs()?;
    let _dimaltz = c.read_bs()?;
    let _dimalttz = c.read_bs()?;
    let dimupt = c.read_b()?;
    let _dimatfit = c.read_bs()?;
    let _dimfxlon = c.read_b()?;
    // §20.4.68 gates the next block on R2010+: `DIMTXTDIRECTION B 295`,
    // `DIMALTMZF BD`, `DIMALTMZS T`, `DIMMZF BD`, `DIMMZS T`. R2007 stops
    // at `DIMFXLON` and goes straight to `DIMLWD`. Reading the R2010+
    // block on an R2007 record walks a `BD` onto the reserved `11` code
    // — the failure both DIMSTYLE records of every R2007 corpus file
    // reported.
    if version.is_r2010_plus() {
        let _dimtxtdirection = c.read_b()?;
        let _dimaltmzf = c.read_bd()?;
        let _dimmzf = c.read_bd()?;
    }
    let _dimlwd = c.read_bs()?;
    let _dimlwe = c.read_bs()?;
    let _unknown = c.read_b()?;
    split.finish("DIMSTYLE")?;
    let name = split.strings.read_tv()?;
    let _dimpost = split.strings.read_tv()?;
    let _dimapost = split.strings.read_tv()?;
    if version.is_r2010_plus() {
        let _dimaltmzs = split.strings.read_tv()?;
        let _dimmzs = split.strings.read_tv()?;
    }
    Ok(DimStyleEntry {
        header: TableEntryHeader {
            name,
            is_xref_dependent,
            xref_index_plus_1,
            is_xref_resolved: flag64,
        },
        dimscale,
        dimasz,
        dimexo,
        dimexe,
        dimtxt,
        dimcen,
        dimtfac,
        dimlfac,
        dimtih,
        dimtoh,
        dimtad: dimtad as u8,
        dimtolj: dimtolj as u8,
        dimaltf,
        dimaltrnd,
        dimupt,
    })
}

// Legacy alias — the historical public API called this `decode_partial`.
pub use decode as decode_partial;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    fn write_header(w: &mut BitWriter, name: &[u8]) {
        w.write_bs_u(name.len() as u16);
        for b in name {
            w.write_rc(*b);
        }
        w.write_b(false);
        w.write_bs(0);
        w.write_b(false);
    }

    #[test]
    fn roundtrip_standard_dimstyle() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"Standard");
        w.write_bd(1.0); // dimscale
        w.write_bd(0.18); // dimasz
        w.write_bd(0.0625); // dimexo
        w.write_bd(0.18); // dimexe
        w.write_bd(0.18); // dimtxt
        w.write_bd(0.09); // dimcen
        w.write_bd(1.0); // dimtfac
        w.write_bd(1.0); // dimlfac
        w.write_b(true); // dimtih
        w.write_b(true); // dimtoh
        w.write_rc(0); // dimtad
        w.write_rc(1); // dimtolj
        w.write_bd(25.4); // dimaltf
        w.write_bd(0.0); // dimaltrnd
        w.write_b(false); // dimupt
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.header.name, "Standard");
        assert_eq!(d.dimscale, 1.0);
        assert_eq!(d.dimasz, 0.18);
        assert_eq!(d.dimtxt, 0.18);
        assert_eq!(d.dimcen, 0.09);
        assert_eq!(d.dimtad, 0);
        assert_eq!(d.dimtolj, 1);
        assert!(d.dimtih);
        assert!(d.dimtoh);
        assert!(!d.dimupt);
        assert!((d.dimaltf - 25.4).abs() < 1e-12);
    }

    /// Build the whole R2007+ DIMSTYLE body with ISO-25 defaults and
    /// read it back through the split-stream decoder.
    #[test]
    fn r2007_split_stream_dimstyle_reads_iso25_defaults() {
        let mut b = BitWriter::new();
        b.write_bs_u(0); // no EED
        b.write_b(true); // no xdictionary
        b.write_b(false); // no binary data
        b.write_b(false); // 64-flag
        b.write_b(false); // xref dependent
        b.write_bs(0); // xref index + 1
        b.write_bd(1.0); // dimscale
        b.write_bd(2.5); // dimasz
        b.write_bd(0.625); // dimexo
        b.write_bd(3.75); // dimdli
        b.write_bd(1.25); // dimexe
        b.write_bd(0.0); // dimrnd
        b.write_bd(0.0); // dimdle
        b.write_bd(0.0); // dimtp
        b.write_bd(0.0); // dimtm
        b.write_bd(1.0); // dimfxl
        b.write_bd(std::f64::consts::FRAC_PI_2); // dimjogang
        b.write_bs(0); // dimtfill
        write_cmc(&mut b); // dimtfillclr
        for bit in [false, false, false, false, false, false] {
            b.write_b(bit); // dimtol, dimlim, dimtih, dimtoh, dimse1, dimse2
        }
        b.write_bs(1); // dimtad
        b.write_bs(8); // dimzin
        b.write_bs(0); // dimazin
        b.write_bs(0); // dimarcsym
        b.write_bd(2.5); // dimtxt
        b.write_bd(2.5); // dimcen
        b.write_bd(0.0); // dimtsz
        b.write_bd(1.0 / 25.4); // dimaltf
        b.write_bd(1.0); // dimlfac
        b.write_bd(0.0); // dimtvp
        b.write_bd(1.0); // dimtfac
        b.write_bd(0.625); // dimgap
        b.write_bd(0.0); // dimaltrnd
        b.write_b(false); // dimalt
        b.write_bs(3); // dimaltd
        b.write_b(true); // dimtofl
        b.write_b(false); // dimsah
        b.write_b(false); // dimtix
        b.write_b(false); // dimsoxd
        write_cmc(&mut b); // dimclrd
        write_cmc(&mut b); // dimclre
        write_cmc(&mut b); // dimclrt
        b.write_bs(0); // dimadec
        b.write_bs(2); // dimdec
        b.write_bs(2); // dimtdec
        b.write_bs(2); // dimaltu
        b.write_bs(3); // dimalttd
        b.write_bs(0); // dimaunit
        b.write_bs(0); // dimfrac
        b.write_bs(2); // dimlunit
        b.write_bs(44); // dimdsep — '.'
        b.write_bs(0); // dimtmove
        b.write_bs(0); // dimjust
        b.write_b(false); // dimsd1
        b.write_b(false); // dimsd2
        b.write_bs(0); // dimtolj
        b.write_bs(8); // dimtzin
        b.write_bs(0); // dimaltz
        b.write_bs(0); // dimalttz
        b.write_b(false); // dimupt
        b.write_bs(3); // dimatfit
        b.write_b(false); // dimfxlon
        b.write_b(false); // dimtxtdirection
        b.write_bd(100.0); // dimaltmzf
        b.write_bd(100.0); // dimmzf
        b.write_bs(-2); // dimlwd
        b.write_bs(-2); // dimlwe
        b.write_b(false); // unknown
        let bits = crate::string_stream::tests::bits_of(&b);
        let payload =
            crate::string_stream::tests::build_payload(&bits, &["ISO-25", "", "", "", ""]);
        let d = decode_modern_split_stream(&payload, 8, Version::R2018).unwrap();
        assert_eq!(d.header.name, "ISO-25");
        assert_eq!(d.dimscale, 1.0);
        assert_eq!(d.dimasz, 2.5);
        assert_eq!(d.dimexo, 0.625);
        assert_eq!(d.dimexe, 1.25);
        assert_eq!(d.dimtxt, 2.5);
        assert_eq!(d.dimcen, 2.5);
        assert_eq!(d.dimtfac, 1.0);
        assert_eq!(d.dimlfac, 1.0);
        assert_eq!(d.dimtad, 1);
        assert_eq!(d.dimtolj, 0);
        assert_eq!(d.dimaltrnd, 0.0);
        assert!(!d.dimtih);
        assert!(!d.dimtoh);
        assert!(!d.dimupt);
        assert!((d.dimaltf - 1.0 / 25.4).abs() < 1e-15);
    }

    /// Write a full `CMC` colour: index word, true-colour word, byte.
    fn write_cmc(w: &mut BitWriter) {
        w.write_bs(0);
        w.write_bl(0xC100_0000u32 as i32);
        w.write_rc(0);
    }

    #[test]
    fn roundtrip_metric_dimstyle() {
        let mut w = BitWriter::new();
        write_header(&mut w, b"ISO-25");
        w.write_bd(1.0); // dimscale
        w.write_bd(2.5); // dimasz mm
        w.write_bd(0.625);
        w.write_bd(1.25);
        w.write_bd(2.5); // dimtxt
        w.write_bd(0.0); // dimcen disabled
        w.write_bd(1.0);
        w.write_bd(1.0);
        w.write_b(false);
        w.write_b(false);
        w.write_rc(1); // dimtad = above
        w.write_rc(0);
        w.write_bd(1.0);
        w.write_bd(0.0);
        w.write_b(true); // dimupt = user positioning
        let bytes = w.into_bytes();
        let mut c = BitCursor::new(&bytes);
        let d = decode(&mut c, Version::R2000).unwrap();
        assert_eq!(d.header.name, "ISO-25");
        assert_eq!(d.dimasz, 2.5);
        assert!(!d.dimtih);
        assert_eq!(d.dimtad, 1);
        assert!(d.dimupt);
    }
}
