//! The decoded `AcDb:Header` as a DXF `HEADER` section.
//!
//! [`header_entries`] turns [`crate::header_vars::HeaderVars`] into the
//! `$VARIABLE` groups AutoCAD's own DXF export writes for the same drawing,
//! and writes nothing else: a variable AutoCAD's export omits (`$PELLIPSE`,
//! `$FACETRES`, the unknown slots, the control-object handles) is omitted
//! here too. Three variables DXF puts in `HEADER` live elsewhere in the DWG and
//! are read from there: `$MEASUREMENT` (`AcDb:Template`, §22), `$DWGCODEPAGE` (the
//! file header's code page, 29 and 30 measured) and `$LASTSAVEDBY`
//! (`AcDb:SummaryInfo`). `$ACADMAINTVER` and the local `$TDCREATE` and
//! `$TDUPDATE` are not derivable from the DWG and are omitted.
//!
//! # Where the table comes from
//!
//! [`VARS`] is measured, not transcribed from a reference: for each variable
//! the group codes and the first release whose AutoCAD export writes it are
//! those of the 50 AutoCAD DXF exports (AC1015 to AC1032) of the public
//! corpus, which agree with each other (`$XCLIPFRAME` is 290 before R2010 and
//! 280 after; every other variable keeps its codes). `examples/probe_dxf_header.rs`
//! reads `dwg-to-dxf` output back and compares every variable with those
//! exports.
//!
//! # What the DXF form needs that the DWG form does not hold
//!
//! - A handle whose DXF form is a name (`$CLAYER`, `$DIMBLK`, ...) is
//!   resolved through the decoded symbol-table record it points at. A name
//!   that cannot be resolved omits the variable rather than guessing.
//! - If a record's fields do not all decode (an R2007 `DIMSTYLE` with a custom
//!   dimension variable), its name is still the first string of its string stream.
//! - A name is kept only if the output defines the record
//!   ([`crate::dxf_tables`] writes the symbol tables): `$CLAYER` for a written
//!   layer, `$TEXTSTYLE` and `$DIMTXSTY` for a written style, `$CELTYPE` and the
//!   `$DIMLTYPE` family for a written linetype, `$DIMSTYLE` for a written dimension
//!   style. An empty name (a null handle) is always kept. An arrowhead block
//!   (`$DIMBLK` and its family) is kept only for the two blocks BLOCKS writes
//!   ([`crate::dxf_tables::BLOCKS_EXPORTED`]), and every other name (`$CMLSTYLE`,
//!   the UCS names) is omitted until those records are written.
//! - An angle is degrees in DXF group 50 (`$ANGBASE`), radians in the database.
//!   Before R2007 text that is not ASCII is written as `\U+XXXX` escapes, since
//!   those releases read bytes in `$DWGCODEPAGE`.
//! - `$CMLSTYLE` is written under the key the style has in the
//!   `ACAD_MLINESTYLE` dictionary (`Standard`), the record holds it in capitals.
//! - A standard arrowhead block `_BoxBlank` is `$DIMBLK` `BoxBlank`; a block with any
//!   other name keeps its name verbatim.
//! - A handle variable that names an object (`$CMATERIAL`, `$INTERFEREOBJVS`,
//!   `$INTERFEREVPVS`, `$CPSNID`) is omitted: the converter exports those objects
//!   as untyped placeholders, so the reference would point at the wrong thing.
//!   `$HANDSEED`, the next free handle, is written. (A null handle there is
//!   where AutoCAD's export names the default visual style, which the DWG does
//!   not hold.)
//! - Control characters and the caret are written in DXF's caret notation, so a
//!   value with a line feed stays on its own line.
//! - A colour is its AutoCAD index ([`crate::header_vars::Color::aci`]); a
//!   true colour has none and is omitted.
//! - Before R2010 `$XCLIPFRAME` is the flag 290, not 280: the value 2 does not
//!   exist there and AutoCAD writes it false.
//!
//! Only targets from R2000 on are written: no AutoCAD export of R12 or R14 is in
//! the corpus to measure the set against, so those targets keep `$ACADVER`
//! alone.

use std::collections::BTreeMap;

use crate::dxf::DxfVersion;
use crate::dxf_sections::{HeaderEntry, HeaderGroup};
use crate::dxf_tables::Defined;
use crate::entities::DecodedEntity;
use crate::header_table::{R2000, R2004, R2007, R2010, R2013, R2018, Rel};
use crate::header_vars::Value;
use crate::object_type::ObjectType;
use crate::reader::DwgFile;

/// `$NAME`, its DXF group codes in order, the first release whose export
/// writes it.
pub static VARS: &[(&str, &[u16], Rel)] = &[
    ("$REQUIREDVERSIONS", &[160], R2013),
    ("$INSBASE", &[10, 20, 30], R2000),
    ("$EXTMIN", &[10, 20, 30], R2000),
    ("$EXTMAX", &[10, 20, 30], R2000),
    ("$LIMMIN", &[10, 20], R2000),
    ("$LIMMAX", &[10, 20], R2000),
    ("$ORTHOMODE", &[70], R2000),
    ("$REGENMODE", &[70], R2000),
    ("$FILLMODE", &[70], R2000),
    ("$QTEXTMODE", &[70], R2000),
    ("$MIRRTEXT", &[70], R2000),
    ("$LTSCALE", &[40], R2000),
    ("$ATTMODE", &[70], R2000),
    ("$TEXTSIZE", &[40], R2000),
    ("$TRACEWID", &[40], R2000),
    ("$TEXTSTYLE", &[7], R2000),
    ("$CLAYER", &[8], R2000),
    ("$CELTYPE", &[6], R2000),
    ("$CECOLOR", &[62], R2000),
    ("$CELTSCALE", &[40], R2000),
    ("$DISPSILH", &[70], R2000),
    ("$DIMSCALE", &[40], R2000),
    ("$DIMASZ", &[40], R2000),
    ("$DIMEXO", &[40], R2000),
    ("$DIMDLI", &[40], R2000),
    ("$DIMRND", &[40], R2000),
    ("$DIMDLE", &[40], R2000),
    ("$DIMEXE", &[40], R2000),
    ("$DIMTP", &[40], R2000),
    ("$DIMTM", &[40], R2000),
    ("$DIMTXT", &[40], R2000),
    ("$DIMCEN", &[40], R2000),
    ("$DIMTSZ", &[40], R2000),
    ("$DIMTOL", &[70], R2000),
    ("$DIMLIM", &[70], R2000),
    ("$DIMTIH", &[70], R2000),
    ("$DIMTOH", &[70], R2000),
    ("$DIMSE1", &[70], R2000),
    ("$DIMSE2", &[70], R2000),
    ("$DIMTAD", &[70], R2000),
    ("$DIMZIN", &[70], R2000),
    ("$DIMBLK", &[1], R2000),
    ("$DIMASO", &[70], R2000),
    ("$DIMSHO", &[70], R2000),
    ("$DIMPOST", &[1], R2000),
    ("$DIMAPOST", &[1], R2000),
    ("$DIMALT", &[70], R2000),
    ("$DIMALTD", &[70], R2000),
    ("$DIMALTF", &[40], R2000),
    ("$DIMLFAC", &[40], R2000),
    ("$DIMTOFL", &[70], R2000),
    ("$DIMTVP", &[40], R2000),
    ("$DIMTIX", &[70], R2000),
    ("$DIMSOXD", &[70], R2000),
    ("$DIMSAH", &[70], R2000),
    ("$DIMBLK1", &[1], R2000),
    ("$DIMBLK2", &[1], R2000),
    ("$DIMSTYLE", &[2], R2000),
    ("$DIMCLRD", &[70], R2000),
    ("$DIMCLRE", &[70], R2000),
    ("$DIMCLRT", &[70], R2000),
    ("$DIMTFAC", &[40], R2000),
    ("$DIMGAP", &[40], R2000),
    ("$DIMJUST", &[70], R2000),
    ("$DIMSD1", &[70], R2000),
    ("$DIMSD2", &[70], R2000),
    ("$DIMTOLJ", &[70], R2000),
    ("$DIMTZIN", &[70], R2000),
    ("$DIMALTZ", &[70], R2000),
    ("$DIMALTTZ", &[70], R2000),
    ("$DIMUPT", &[70], R2000),
    ("$DIMDEC", &[70], R2000),
    ("$DIMTDEC", &[70], R2000),
    ("$DIMALTU", &[70], R2000),
    ("$DIMALTTD", &[70], R2000),
    ("$DIMTXSTY", &[7], R2000),
    ("$DIMAUNIT", &[70], R2000),
    ("$DIMADEC", &[70], R2000),
    ("$DIMALTRND", &[40], R2000),
    ("$DIMAZIN", &[70], R2000),
    ("$DIMDSEP", &[70], R2000),
    ("$DIMATFIT", &[70], R2000),
    ("$DIMFRAC", &[70], R2000),
    ("$DIMLDRBLK", &[1], R2000),
    ("$DIMLUNIT", &[70], R2000),
    ("$DIMLWD", &[70], R2000),
    ("$DIMLWE", &[70], R2000),
    ("$DIMTMOVE", &[70], R2000),
    ("$DIMFXL", &[40], R2007),
    ("$DIMFXLON", &[70], R2007),
    ("$DIMJOGANG", &[40], R2007),
    ("$DIMTFILL", &[70], R2007),
    ("$DIMTFILLCLR", &[70], R2007),
    ("$DIMARCSYM", &[70], R2007),
    ("$DIMLTYPE", &[6], R2007),
    ("$DIMLTEX1", &[6], R2007),
    ("$DIMLTEX2", &[6], R2007),
    ("$DIMTXTDIRECTION", &[70], R2010),
    ("$LUNITS", &[70], R2000),
    ("$LUPREC", &[70], R2000),
    ("$SKETCHINC", &[40], R2000),
    ("$FILLETRAD", &[40], R2000),
    ("$AUNITS", &[70], R2000),
    ("$AUPREC", &[70], R2000),
    ("$MENU", &[1], R2000),
    ("$ELEVATION", &[40], R2000),
    ("$PELEVATION", &[40], R2000),
    ("$THICKNESS", &[40], R2000),
    ("$LIMCHECK", &[70], R2000),
    ("$CHAMFERA", &[40], R2000),
    ("$CHAMFERB", &[40], R2000),
    ("$CHAMFERC", &[40], R2000),
    ("$CHAMFERD", &[40], R2000),
    ("$SKPOLY", &[70], R2000),
    ("$TDCREATE", &[40], R2000),
    ("$TDUCREATE", &[40], R2000),
    ("$TDUPDATE", &[40], R2000),
    ("$TDUUPDATE", &[40], R2000),
    ("$TDINDWG", &[40], R2000),
    ("$TDUSRTIMER", &[40], R2000),
    ("$USRTIMER", &[70], R2000),
    ("$ANGBASE", &[50], R2000),
    ("$ANGDIR", &[70], R2000),
    ("$PDMODE", &[70], R2000),
    ("$PDSIZE", &[40], R2000),
    ("$PLINEWID", &[40], R2000),
    ("$SPLFRAME", &[70], R2000),
    ("$SPLINETYPE", &[70], R2000),
    ("$SPLINESEGS", &[70], R2000),
    ("$HANDSEED", &[5], R2000),
    ("$SURFTAB1", &[70], R2000),
    ("$SURFTAB2", &[70], R2000),
    ("$SURFTYPE", &[70], R2000),
    ("$SURFU", &[70], R2000),
    ("$SURFV", &[70], R2000),
    ("$UCSBASE", &[2], R2000),
    ("$UCSNAME", &[2], R2000),
    ("$UCSORG", &[10, 20, 30], R2000),
    ("$UCSXDIR", &[10, 20, 30], R2000),
    ("$UCSYDIR", &[10, 20, 30], R2000),
    ("$UCSORTHOREF", &[2], R2000),
    ("$UCSORTHOVIEW", &[70], R2000),
    ("$UCSORGTOP", &[10, 20, 30], R2000),
    ("$UCSORGBOTTOM", &[10, 20, 30], R2000),
    ("$UCSORGLEFT", &[10, 20, 30], R2000),
    ("$UCSORGRIGHT", &[10, 20, 30], R2000),
    ("$UCSORGFRONT", &[10, 20, 30], R2000),
    ("$UCSORGBACK", &[10, 20, 30], R2000),
    ("$PUCSBASE", &[2], R2000),
    ("$PUCSNAME", &[2], R2000),
    ("$PUCSORG", &[10, 20, 30], R2000),
    ("$PUCSXDIR", &[10, 20, 30], R2000),
    ("$PUCSYDIR", &[10, 20, 30], R2000),
    ("$PUCSORTHOREF", &[2], R2000),
    ("$PUCSORTHOVIEW", &[70], R2000),
    ("$PUCSORGTOP", &[10, 20, 30], R2000),
    ("$PUCSORGBOTTOM", &[10, 20, 30], R2000),
    ("$PUCSORGLEFT", &[10, 20, 30], R2000),
    ("$PUCSORGRIGHT", &[10, 20, 30], R2000),
    ("$PUCSORGFRONT", &[10, 20, 30], R2000),
    ("$PUCSORGBACK", &[10, 20, 30], R2000),
    ("$USERI1", &[70], R2000),
    ("$USERI2", &[70], R2000),
    ("$USERI3", &[70], R2000),
    ("$USERI4", &[70], R2000),
    ("$USERI5", &[70], R2000),
    ("$USERR1", &[40], R2000),
    ("$USERR2", &[40], R2000),
    ("$USERR3", &[40], R2000),
    ("$USERR4", &[40], R2000),
    ("$USERR5", &[40], R2000),
    ("$WORLDVIEW", &[70], R2000),
    ("$SHADEDGE", &[70], R2000),
    ("$SHADEDIF", &[70], R2000),
    ("$TILEMODE", &[70], R2000),
    ("$MAXACTVP", &[70], R2000),
    ("$PINSBASE", &[10, 20, 30], R2000),
    ("$PLIMCHECK", &[70], R2000),
    ("$PEXTMIN", &[10, 20, 30], R2000),
    ("$PEXTMAX", &[10, 20, 30], R2000),
    ("$PLIMMIN", &[10, 20], R2000),
    ("$PLIMMAX", &[10, 20], R2000),
    ("$UNITMODE", &[70], R2000),
    ("$VISRETAIN", &[70], R2000),
    ("$PLINEGEN", &[70], R2000),
    ("$PSLTSCALE", &[70], R2000),
    ("$TREEDEPTH", &[70], R2000),
    ("$CMLSTYLE", &[2], R2000),
    ("$CMLJUST", &[70], R2000),
    ("$CMLSCALE", &[40], R2000),
    ("$PROXYGRAPHICS", &[70], R2000),
    ("$CELWEIGHT", &[370], R2000),
    ("$ENDCAPS", &[280], R2000),
    ("$JOINSTYLE", &[280], R2000),
    ("$LWDISPLAY", &[290], R2000),
    ("$INSUNITS", &[70], R2000),
    ("$HYPERLINKBASE", &[1], R2000),
    ("$STYLESHEET", &[1], R2000),
    ("$XEDIT", &[290], R2000),
    ("$CEPSNTYPE", &[380], R2000),
    ("$PSTYLEMODE", &[290], R2000),
    ("$FINGERPRINTGUID", &[2], R2000),
    ("$VERSIONGUID", &[2], R2000),
    ("$EXTNAMES", &[290], R2000),
    ("$PSVPSCALE", &[40], R2000),
    ("$OLESTARTUP", &[290], R2000),
    ("$SORTENTS", &[280], R2004),
    ("$INDEXCTL", &[280], R2004),
    ("$HIDETEXT", &[280], R2004),
    ("$XCLIPFRAME", &[280], R2004),
    ("$HALOGAP", &[280], R2004),
    ("$OBSCOLOR", &[70], R2004),
    ("$OBSLTYPE", &[280], R2004),
    ("$INTERSECTIONDISPLAY", &[280], R2004),
    ("$INTERSECTIONCOLOR", &[70], R2004),
    ("$DIMASSOC", &[280], R2004),
    ("$PROJECTNAME", &[1], R2004),
    ("$CAMERADISPLAY", &[290], R2007),
    ("$LENSLENGTH", &[40], R2007),
    ("$CAMERAHEIGHT", &[40], R2007),
    ("$STEPSPERSEC", &[40], R2007),
    ("$STEPSIZE", &[40], R2007),
    ("$3DDWFPREC", &[40], R2007),
    ("$PSOLWIDTH", &[40], R2007),
    ("$PSOLHEIGHT", &[40], R2007),
    ("$LOFTANG1", &[40], R2007),
    ("$LOFTANG2", &[40], R2007),
    ("$LOFTMAG1", &[40], R2007),
    ("$LOFTMAG2", &[40], R2007),
    ("$LOFTPARAM", &[70], R2007),
    ("$LOFTNORMALS", &[280], R2007),
    ("$LATITUDE", &[40], R2007),
    ("$LONGITUDE", &[40], R2007),
    ("$NORTHDIRECTION", &[40], R2007),
    ("$TIMEZONE", &[70], R2007),
    ("$LIGHTGLYPHDISPLAY", &[280], R2007),
    ("$TILEMODELIGHTSYNCH", &[280], R2007),
    ("$CMATERIAL", &[347], R2007),
    ("$SOLIDHIST", &[280], R2007),
    ("$SHOWHIST", &[280], R2007),
    ("$DWFFRAME", &[280], R2007),
    ("$DGNFRAME", &[280], R2007),
    ("$REALWORLDSCALE", &[290], R2007),
    ("$INTERFERECOLOR", &[62], R2007),
    ("$INTERFEREOBJVS", &[345], R2007),
    ("$INTERFEREVPVS", &[346], R2007),
    ("$CSHADOW", &[280], R2007),
    ("$SHADOWPLANELOCATION", &[40], R2007),
];

/// The `$DWGCODEPAGE` name for the code page in the file header (§3.2.5,
/// §4.1: the short at 0x13). Only the two values the corpus measures against
/// AutoCAD's export are named ([`crate::code_page::CodePage`]); any other code
/// page omits the variable.
fn code_page_name(file: &DwgFile) -> Option<&'static str> {
    file.code_page().dxf_name()
}

/// `$MEASUREMENT`, the one variable of `AcDb:Template` (§22): an `Int16`
/// description length, the description, then the `UInt16`.
///
/// §22 counts the description in bytes. AutoCAD writes it empty (0) up to
/// R2004, but from R2007 it writes a length of 1 and two bytes, so the length
/// counts UTF-16 units there; every R2007 to R2018 file of the corpus agrees.
fn measurement(file: &DwgFile) -> Option<i64> {
    let section = file.read_section("AcDb:Template")?.ok()?;
    let len = usize::from(u16::from_le_bytes([*section.first()?, *section.get(1)?]));
    let unit = if file.version().is_r2007_plus() { 2 } else { 1 };
    let at = len.checked_mul(unit)?.checked_add(2)?;
    let word = section.get(at..at.checked_add(2)?)?;
    Some(i64::from(u16::from_le_bytes([word[0], word[1]])))
}

/// `s` as text for the DXF release `version` (see `text_for`): control
/// characters in caret notation, and before R2007 what is not ASCII as
/// `\U+XXXX` escapes. R12 and R14 targets are returned unchanged.
pub(crate) fn escape_text(version: DxfVersion, s: &str) -> String {
    match target_release(version) {
        Some(rel) => text_for(rel, s),
        None => s.to_string(),
    }
}

fn target_release(version: DxfVersion) -> Option<Rel> {
    match version {
        DxfVersion::R12 | DxfVersion::R14 => None,
        DxfVersion::R2000 => Some(R2000),
        DxfVersion::R2004 => Some(R2004),
        DxfVersion::R2007 => Some(R2007),
        DxfVersion::R2010 => Some(R2010),
        DxfVersion::R2013 => Some(R2013),
        DxfVersion::R2018 => Some(R2018),
    }
}

/// The first string of an R2007+ record's string stream, which for a
/// symbol-table entry is its name.
fn first_string(
    raw: &crate::object::RawObject,
    version: crate::version::Version,
) -> Option<String> {
    if !version.is_r2007_plus() {
        return None;
    }
    let stream = crate::string_stream::locate(&raw.raw, version)?;
    crate::string_stream::StringReader::new(&raw.raw, stream)
        .ok()?
        .read_tv()
        .ok()
        .filter(|n| !n.is_empty())
}

#[derive(Default)]
struct Names {
    layers: BTreeMap<u64, String>,
    styles: BTreeMap<u64, String>,
    ltypes: BTreeMap<u64, String>,
    dimstyles: BTreeMap<u64, String>,
    ucs: BTreeMap<u64, String>,
    blocks: BTreeMap<u64, String>,
    mlines: BTreeMap<u64, String>,
    mline_keys: Vec<String>,
    /// The symbol records the output defines.
    defined: Defined,
}

impl Names {
    fn build(file: &DwgFile, mline_dictionary: Option<u64>, defined: &Defined) -> Self {
        let mut names = Names {
            defined: defined.clone(),
            ..Names::default()
        };
        let Some(Ok(objects)) = file.all_objects() else {
            return names;
        };
        let version = file.version();
        names.blocks = crate::graph::resolve_block_names(&objects, version);
        for raw in &objects {
            let h = raw.handle.value;
            match raw.kind {
                ObjectType::Layer
                | ObjectType::Style
                | ObjectType::Ltype
                | ObjectType::DimStyle
                | ObjectType::Ucs
                | ObjectType::MLineStyle
                | ObjectType::Dictionary => {}
                _ => continue,
            }
            let decoded = crate::entities::decode_from_raw(raw, version);
            if matches!(decoded, DecodedEntity::Error { .. }) {
                // The record's fields do not all decode, but from R2007 its
                // name is the first string of its string stream.
                if let Some(name) = first_string(raw, version) {
                    match raw.kind {
                        ObjectType::Layer => names.layers.insert(h, name),
                        ObjectType::Style => names.styles.insert(h, name),
                        ObjectType::Ltype => names.ltypes.insert(h, name),
                        ObjectType::DimStyle => names.dimstyles.insert(h, name),
                        ObjectType::Ucs => names.ucs.insert(h, name),
                        _ => None,
                    };
                }
                continue;
            }
            match decoded {
                DecodedEntity::Layer(e) => {
                    names.layers.insert(h, e.header.name);
                }
                DecodedEntity::Style(e) => {
                    names.styles.insert(h, e.header.name);
                }
                DecodedEntity::Ltype(e) => {
                    names.ltypes.insert(h, e.header.name);
                }
                DecodedEntity::DimStyle(e) => {
                    names.dimstyles.insert(h, e.header.name);
                }
                DecodedEntity::Ucs(e) => {
                    names.ucs.insert(h, e.header.name);
                }
                DecodedEntity::MLineStyle(e) => {
                    names.mlines.insert(h, e.name);
                }
                DecodedEntity::Dictionary(d) if Some(h) == mline_dictionary => {
                    names.mline_keys = d.keys;
                }
                _ => {}
            }
        }
        names
    }

    /// The name a variable holds, if the output defines the record it names.
    ///
    /// A name that is not empty is kept only for a layer, text style,
    /// linetype or block the written tables define: a DXF whose `$DIMSTYLE` names a
    /// style it never defines is not one.
    fn resolve(&self, var: &str, handle: u64) -> Option<String> {
        let name = self.record_name(var, handle)?;
        let set = match var {
            "$CLAYER" => &self.defined.layers,
            "$DIMSTYLE" => &self.defined.dimstyles,
            "$TEXTSTYLE" | "$DIMTXSTY" => &self.defined.styles,
            "$CELTYPE" | "$DIMLTYPE" | "$DIMLTEX1" | "$DIMLTEX2" => &self.defined.ltypes,
            // Only the blocks BLOCKS writes are in `defined.blocks`
            // (`dxf_tables::BLOCKS_EXPORTED`): an arrowhead block it does not write
            // is left unnamed until it does.
            "$DIMBLK" | "$DIMBLK1" | "$DIMBLK2" | "$DIMLDRBLK" => {
                let defined = self.defined.blocks.iter().any(|n| n == &name);
                return (name.is_empty() || defined).then_some(name);
            }
            _ => return name.is_empty().then_some(name),
        };
        (name.is_empty() || set.iter().any(|n| n == &name)).then_some(name)
    }

    fn record_name(&self, var: &str, handle: u64) -> Option<String> {
        if handle == 0 {
            return Some(String::new());
        }
        let table = match var {
            "$CLAYER" => &self.layers,
            "$TEXTSTYLE" | "$DIMTXSTY" => &self.styles,
            "$CELTYPE" | "$DIMLTYPE" | "$DIMLTEX1" | "$DIMLTEX2" => &self.ltypes,
            "$DIMSTYLE" => &self.dimstyles,
            "$UCSNAME" | "$PUCSNAME" | "$UCSBASE" | "$PUCSBASE" | "$UCSORTHOREF"
            | "$PUCSORTHOREF" => &self.ucs,
            "$CMLSTYLE" => {
                let name = self.mlines.get(&handle)?;
                return Some(
                    self.mline_keys
                        .iter()
                        .find(|k| k.eq_ignore_ascii_case(name))
                        .cloned()
                        .unwrap_or_else(|| name.clone()),
                );
            }
            "$DIMBLK" | "$DIMBLK1" | "$DIMBLK2" | "$DIMLDRBLK" => {
                let name = self.blocks.get(&handle)?;
                let builtin = BUILTIN_ARROWHEADS
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case(name));
                return Some(if builtin {
                    name.strip_prefix('_').unwrap_or(name).to_string()
                } else {
                    name.clone()
                });
            }
            _ => return None,
        };
        table.get(&handle).cloned()
    }
}

/// AutoCAD's standard arrowhead blocks, which `$DIMBLK` names without the
/// leading underscore. Only `_BoxBlank` is measured on the corpus; a block with
/// any other name keeps it, underscore and all, because a user's block may
/// start with one.
const BUILTIN_ARROWHEADS: &[&str] = &[
    "_ARCHTICK",
    "_BOXBLANK",
    "_BOXFILLED",
    "_CLOSED",
    "_CLOSEDBLANK",
    "_DATUMBLANK",
    "_DATUMFILLED",
    "_DOT",
    "_DOTBLANK",
    "_DOTSMALL",
    "_INTEGRAL",
    "_NONE",
    "_OBLIQUE",
    "_OPEN",
    "_OPEN30",
    "_OPEN90",
    "_ORIGIN",
    "_ORIGIN2",
    "_SMALL",
];

/// Text for a target release. Control characters and the caret are written in
/// DXF's caret notation on every release. DXF before R2007 reads bytes in
/// `$DWGCODEPAGE`, not UTF-8, so what that code page cannot be assumed to hold
/// is written as `\U+XXXX` escapes, which every release reads; from R2007 it is
/// UTF-8.
fn text_for(target: Rel, s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut units = [0u16; 2];
    for c in s.chars() {
        if c == '^' {
            // DXF writes a caret as a caret and a space.
            out.push_str("^ ");
        } else if c.is_ascii_control() {
            // A control character, a line feed above all, is `^` and the
            // character 64 above it (`^J`), so a value stays on its own line.
            out.push('^');
            out.push(char::from(c as u8 ^ 0x40));
        } else if c.is_ascii() || target >= R2007 {
            out.push(c);
        } else {
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\U+{u:04X}"));
            }
        }
    }
    out
}

fn is_handle_group(code: u16) -> bool {
    code == 5 || (320..=369).contains(&code) || (390..=399).contains(&code)
}

fn groups(
    name: &str,
    codes: &[u16],
    value: &Value,
    names: &Names,
    target: Rel,
) -> Option<Vec<HeaderGroup>> {
    let code = |i: usize| codes.get(i).map(|c| i32::from(*c));
    let real = |i: usize, v: f64| match code(i) {
        // Angles are radians in the database and degrees in DXF groups 50 to 58.
        Some(c) if v.is_finite() && (50..=58).contains(&c) => {
            Some(HeaderGroup::Real(c, v.to_degrees()))
        }
        Some(c) if v.is_finite() => Some(HeaderGroup::Real(c, v)),
        _ => None,
    };
    Some(match value {
        Value::Bool(_) | Value::Byte(_) | Value::Short(_) | Value::Long(_) | Value::LongLong(_) => {
            let v = value.as_i64()?;
            if name == "$XCLIPFRAME" && target < R2010 {
                vec![HeaderGroup::Int(290, i64::from(v == 1))]
            } else if name == "$XCLIPFRAME" {
                vec![HeaderGroup::Int(280, v)]
            } else {
                vec![HeaderGroup::Int(code(0)?, v)]
            }
        }
        Value::Double(v) => vec![real(0, *v)?],
        Value::Point3(p) => vec![real(0, p[0])?, real(1, p[1])?, real(2, p[2])?],
        Value::Point2(p) => vec![real(0, p[0])?, real(1, p[1])?],
        Value::Text(t) => vec![HeaderGroup::Text(code(0)?, text_for(target, t))],
        // Text in the drawing's code page, which the crate does not apply.
        Value::RawText(_) => return None,
        Value::Color(c) => vec![HeaderGroup::Int(code(0)?, i64::from(c.aci()?))],
        Value::Date { .. } => vec![real(0, value.as_julian()?)?],
        Value::Handle(h) => {
            if is_handle_group(*codes.first()?) {
                // `$HANDSEED` is the next free handle, not a reference. Every other
                // handle variable names an object (a MATERIAL, a VISUALSTYLE, a
                // plot style) which the converter exports as a placeholder, not as
                // that type, so the reference is omitted until it does.
                if h.value == 0 || name != "$HANDSEED" {
                    return None;
                }
                vec![HeaderGroup::Handle(code(0)?, h.value)]
            } else {
                vec![HeaderGroup::Text(
                    code(0)?,
                    text_for(target, &names.resolve(name, h.value)?),
                )]
            }
        }
    })
}

/// The `HEADER` variables to write for `file` when the output targets
/// `version`: every variable the DWG header holds that AutoCAD's export of
/// that release writes, with its DXF groups.
///
/// Returns nothing for [`DxfVersion::R12`] and [`DxfVersion::R14`] (no
/// export of either is in the corpus to measure the set against), and when the
/// file has no readable header.
///
/// `defined` names the layers, text styles and linetypes the written tables
/// define. A variable that names any other symbol record is omitted, since the
/// converter writes none (a `$DIMSTYLE` pointing at nothing is not a valid
/// reference); an empty name (a null handle) is always kept.
pub fn header_entries(file: &DwgFile, version: DxfVersion, defined: &Defined) -> Vec<HeaderEntry> {
    let Some(target) = target_release(version) else {
        return Vec::new();
    };
    let Some(Ok(header)) = file.header_vars() else {
        return Vec::new();
    };
    if !header.closed {
        return Vec::new();
    }
    let names = Names::build(
        file,
        header
            .get_handle("DICTIONARY_ACAD_MLINESTYLE")
            .map(|h| h.value),
        defined,
    );
    let mut out = Vec::new();
    if let Some(page) = code_page_name(file) {
        out.push(HeaderEntry::groups(
            "$DWGCODEPAGE",
            vec![HeaderGroup::Text(3, page.to_string())],
        ));
    }
    if target >= R2004 {
        if let Some(Ok(info)) = file.summary_info() {
            out.push(HeaderEntry::groups(
                "$LASTSAVEDBY",
                vec![HeaderGroup::Text(1, text_for(target, &info.last_saved_by))],
            ));
        }
    }
    if let Some(m) = measurement(file) {
        out.push(HeaderEntry::groups(
            "$MEASUREMENT",
            vec![HeaderGroup::Int(70, m)],
        ));
    }
    for (name, codes, first) in VARS {
        if *first > target {
            continue;
        }
        let Some(value) = header.get(name) else {
            continue;
        };
        if let Some(g) = groups(name, codes, value, &names, target) {
            out.push(HeaderEntry::groups(*name, g));
        }
    }
    out
}
