//! The `AcDb:Header` variable table (ODA spec v5.4.1 §9).
//!
//! One row per slot of the header's bit stream, in stream order, each
//! with the bit code that reads it and the first and last release that
//! carry it. [`crate::header_vars`] walks this table for the file's
//! release and nothing else: there is no per-release decoder to keep in
//! step with the spec.
//!
//! # Reading the table
//!
//! - **Order is the stream.** §9 lists the slots in the order they are
//!   written, grouped under release headings (`R13-R14 Only`,
//!   `R2000+ Only`, `Common`, ...). A heading holds until the next one, so
//!   `DIMPOST` and `DIMAPOST` sit under the `R2000+ Only` that opens
//!   before `UCSORTHOREF`, and the `R13-R14` forms of the same names
//!   come later as `T`.
//! - **Releases.** [`R13`] to [`R2018`] are an ordinal, one per
//!   [`crate::Version`]. The R13 rows (which §9 always groups with R14) close
//!   on `Cabin.dwg`, 195 slots to the stated end. The 21 Sovol R13 drawings
//!   also write §9's four "R14+" shorts (each -1), which §9 says "do not seem
//!   to be required", and size the section one byte past the byte holding the
//!   last slot, so they are read with the R14 rows (ARCHITECTURE.md §7ad).
//!   `Pre-2004 Only` is R13 to R2000, `R13-R15` is R13 to R2000, and
//!   `R13-R18` (`MENUNAME`) is R13 to R2004: §9's R15 and R18 are the
//!   file-format generations AC1015 and AC1018.
//! - **Names.** A name that starts with `$` is the AutoCAD variable
//!   (`$INSBASE`, `$DIMTAD`, ...), spelled as the DXF `HEADER` section
//!   spells it. The spec's `(PSPACE)` group is AutoCAD's `$P` family
//!   (`$PINSBASE`, `$PEXTMIN`, ...). A name without `$` is a slot the
//!   header carries that is not a variable (control-object and
//!   dictionary handles, a viewport header), and a name that starts with
//!   `UNKNOWN_` is a slot §9 itself calls unknown or undocumented, named
//!   by its bit code and its order among the slots of that code.
//! - **Derived rows.** [`Kind::Bits`] rows read nothing: they take a
//!   field of the `BL` flags word just before them (§9's `CELWEIGHT`,
//!   `ENDCAPS`, ...), so every variable has a name of its own.

/// A release as an ordinal, oldest first.
pub type Rel = u8;

/// AC1012.
pub const R13: Rel = 0;
/// AC1014.
pub const R14: Rel = 1;
/// AC1015.
pub const R2000: Rel = 2;
/// AC1018.
pub const R2004: Rel = 3;
/// AC1021.
pub const R2007: Rel = 4;
/// AC1024.
pub const R2010: Rel = 5;
/// AC1027.
pub const R2013: Rel = 6;
/// AC1032.
pub const R2018: Rel = 7;

/// How a slot is read (spec §2 bit codes, plus the header's own forms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `B`: one bit.
    B,
    /// `BS`: bitshort.
    Bs,
    /// `BL`: bitlong.
    Bl,
    /// `BLL`: bitlonglong.
    Bll,
    /// `BD`: bitdouble.
    Bd,
    /// `RC`: raw byte.
    Rc,
    /// `3BD`: three bitdoubles.
    Bd3,
    /// `2RD`: two raw doubles.
    Rd2,
    /// `TV` (and R13-R14's `T`): text, from the string stream from R2007.
    Tv,
    /// `H`: handle reference, from the handle stream from R2007.
    H,
    /// `H` kept in the data stream on every release (`HANDSEED`).
    HData,
    /// `B` that ends the section: from R2007 it is in the handle stream,
    /// after the last handle, not in the data stream.
    TailB,
    /// `BS` that ends the section: from R2007 it is in the handle stream,
    /// after the last handle, not in the data stream.
    TailBs,
    /// `H` present only when the earlier slot named here equals the
    /// value (`CPSNID`, present when `CEPSNTYPE` is 3).
    HWhen(&'static str, i16),
    /// `CMC`: `BS` index to R2000; from R2004 `BS`, `BL` and `RC`, with
    /// the name and book-name text the `RC` announces.
    Cmc,
    /// Two `BL`: a Julian day and the milliseconds into the day
    /// (`TDUCREATE`, `TDUUPDATE`, `TDINDWG`, `TDUSRTIMER`).
    Date,
    /// Reads nothing: the 5-bit lineweight index of the flags word (mask
    /// `0x001F`), as the DXF value in hundredths of a millimetre.
    Lineweight,
    /// Reads nothing: the bits `mask` of the `BL` read by the closest
    /// earlier row, negated when `invert` is set.
    Bits {
        /// Bits of the flags word this variable occupies.
        mask: u32,
        /// §9 writes `!(Flags & mask)`.
        invert: bool,
    },
}

/// One slot of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Var {
    /// AutoCAD name (`$DIMTAD`), or a name without `$` for a slot that is
    /// not a variable, or `UNKNOWN_<code>_<n>`.
    pub name: &'static str,
    /// How the slot is read.
    pub kind: Kind,
    /// First release that carries it.
    pub first: Rel,
    /// Last release that carries it.
    pub last: Rel,
}

impl Var {
    /// Whether a release carries this slot.
    pub const fn in_release(&self, rel: Rel) -> bool {
        self.first <= rel && rel <= self.last
    }

    /// Whether the name is an AutoCAD variable (`$...`).
    pub fn is_autocad_variable(&self) -> bool {
        self.name.starts_with('$')
    }

    /// Whether §9 itself calls the slot unknown.
    pub fn is_unknown(&self) -> bool {
        self.name.starts_with("UNKNOWN_")
    }
}

const fn v(name: &'static str, kind: Kind, first: Rel, last: Rel) -> Var {
    Var {
        name,
        kind,
        first,
        last,
    }
}

use Kind::*;

/// Every slot of §9, in stream order.
pub static TABLE: &[Var] = &[
    v("$REQUIREDVERSIONS", Bll, R2013, R2018),
    v("UNKNOWN_BD_1", Bd, R13, R2018),
    v("UNKNOWN_BD_2", Bd, R13, R2018),
    v("UNKNOWN_BD_3", Bd, R13, R2018),
    v("UNKNOWN_BD_4", Bd, R13, R2018),
    v("UNKNOWN_TV_1", Tv, R13, R2018),
    v("UNKNOWN_TV_2", Tv, R13, R2018),
    v("UNKNOWN_TV_3", Tv, R13, R2018),
    v("UNKNOWN_TV_4", Tv, R13, R2018),
    v("UNKNOWN_BL_1", Bl, R13, R2018),
    v("UNKNOWN_BL_2", Bl, R13, R2018),
    v("UNKNOWN_BS_1", Bs, R13, R14),
    v("CURRENT_VIEWPORT_ENTITY_HEADER", H, R13, R2000),
    v("$DIMASO", B, R13, R2018),
    v("$DIMSHO", B, R13, R2018),
    v("$DIMSAV", B, R13, R14),
    v("$PLINEGEN", B, R13, R2018),
    v("$ORTHOMODE", B, R13, R2018),
    v("$REGENMODE", B, R13, R2018),
    v("$FILLMODE", B, R13, R2018),
    v("$QTEXTMODE", B, R13, R2018),
    v("$PSLTSCALE", B, R13, R2018),
    v("$LIMCHECK", B, R13, R2018),
    v("$BLIPMODE", B, R13, R14),
    v("UNKNOWN_B_1", B, R2004, R2018),
    v("$USRTIMER", B, R13, R2018),
    v("$SKPOLY", B, R13, R2018),
    v("$ANGDIR", B, R13, R2018),
    v("$SPLFRAME", B, R13, R2018),
    v("$ATTREQ", B, R13, R14),
    v("$ATTDIA", B, R13, R14),
    v("$MIRRTEXT", B, R13, R2018),
    v("$WORLDVIEW", B, R13, R2018),
    v("$WIREFRAME", B, R13, R14),
    v("$TILEMODE", B, R13, R2018),
    v("$PLIMCHECK", B, R13, R2018),
    v("$VISRETAIN", B, R13, R2018),
    v("$DELOBJ", B, R13, R14),
    v("$DISPSILH", B, R13, R2018),
    v("$PELLIPSE", B, R13, R2018),
    v("$PROXYGRAPHICS", Bs, R13, R2018),
    v("$DRAGMODE", Bs, R13, R14),
    v("$TREEDEPTH", Bs, R13, R2018),
    v("$LUNITS", Bs, R13, R2018),
    v("$LUPREC", Bs, R13, R2018),
    v("$AUNITS", Bs, R13, R2018),
    v("$AUPREC", Bs, R13, R2018),
    v("$OSMODE", Bs, R13, R14),
    v("$ATTMODE", Bs, R13, R2018),
    v("$COORDS", Bs, R13, R14),
    v("$PDMODE", Bs, R13, R2018),
    v("$PICKSTYLE", Bs, R13, R14),
    v("UNKNOWN_BL_3", Bl, R2004, R2018),
    v("UNKNOWN_BL_4", Bl, R2004, R2018),
    v("UNKNOWN_BL_5", Bl, R2004, R2018),
    v("$USERI1", Bs, R13, R2018),
    v("$USERI2", Bs, R13, R2018),
    v("$USERI3", Bs, R13, R2018),
    v("$USERI4", Bs, R13, R2018),
    v("$USERI5", Bs, R13, R2018),
    v("$SPLINESEGS", Bs, R13, R2018),
    v("$SURFU", Bs, R13, R2018),
    v("$SURFV", Bs, R13, R2018),
    v("$SURFTYPE", Bs, R13, R2018),
    v("$SURFTAB1", Bs, R13, R2018),
    v("$SURFTAB2", Bs, R13, R2018),
    v("$SPLINETYPE", Bs, R13, R2018),
    v("$SHADEDGE", Bs, R13, R2018),
    v("$SHADEDIF", Bs, R13, R2018),
    v("$UNITMODE", Bs, R13, R2018),
    v("$MAXACTVP", Bs, R13, R2018),
    v("$ISOLINES", Bs, R13, R2018),
    v("$CMLJUST", Bs, R13, R2018),
    v("$TEXTQLTY", Bs, R13, R2018),
    v("$LTSCALE", Bd, R13, R2018),
    v("$TEXTSIZE", Bd, R13, R2018),
    v("$TRACEWID", Bd, R13, R2018),
    v("$SKETCHINC", Bd, R13, R2018),
    v("$FILLETRAD", Bd, R13, R2018),
    v("$THICKNESS", Bd, R13, R2018),
    v("$ANGBASE", Bd, R13, R2018),
    v("$PDSIZE", Bd, R13, R2018),
    v("$PLINEWID", Bd, R13, R2018),
    v("$USERR1", Bd, R13, R2018),
    v("$USERR2", Bd, R13, R2018),
    v("$USERR3", Bd, R13, R2018),
    v("$USERR4", Bd, R13, R2018),
    v("$USERR5", Bd, R13, R2018),
    v("$CHAMFERA", Bd, R13, R2018),
    v("$CHAMFERB", Bd, R13, R2018),
    v("$CHAMFERC", Bd, R13, R2018),
    v("$CHAMFERD", Bd, R13, R2018),
    v("$FACETRES", Bd, R13, R2018),
    v("$CMLSCALE", Bd, R13, R2018),
    v("$CELTSCALE", Bd, R13, R2018),
    v("$MENU", Tv, R13, R2018),
    v("$TDCREATE", Date, R13, R14),
    v("$TDUPDATE", Date, R13, R14),
    v("$TDUCREATE", Date, R2000, R2018),
    v("$TDUUPDATE", Date, R2000, R2018),
    v("UNKNOWN_BL_6", Bl, R2004, R2018),
    v("UNKNOWN_BL_7", Bl, R2004, R2018),
    v("UNKNOWN_BL_8", Bl, R2004, R2018),
    v("$TDINDWG", Date, R13, R2018),
    v("$TDUSRTIMER", Date, R13, R2018),
    v("$CECOLOR", Cmc, R13, R2018),
    v("$HANDSEED", HData, R13, R2018),
    v("$CLAYER", H, R13, R2018),
    v("$TEXTSTYLE", H, R13, R2018),
    v("$CELTYPE", H, R13, R2018),
    v("$CMATERIAL", H, R2007, R2018),
    v("$DIMSTYLE", H, R13, R2018),
    v("$CMLSTYLE", H, R13, R2018),
    v("$PSVPSCALE", Bd, R2000, R2018),
    v("$PINSBASE", Bd3, R13, R2018),
    v("$PEXTMIN", Bd3, R13, R2018),
    v("$PEXTMAX", Bd3, R13, R2018),
    v("$PLIMMIN", Rd2, R13, R2018),
    v("$PLIMMAX", Rd2, R13, R2018),
    v("$PELEVATION", Bd, R13, R2018),
    v("$PUCSORG", Bd3, R13, R2018),
    v("$PUCSXDIR", Bd3, R13, R2018),
    v("$PUCSYDIR", Bd3, R13, R2018),
    v("$PUCSNAME", H, R13, R2018),
    v("$PUCSORTHOREF", H, R2000, R2018),
    v("$PUCSORTHOVIEW", Bs, R2000, R2018),
    v("$PUCSBASE", H, R2000, R2018),
    v("$PUCSORGTOP", Bd3, R2000, R2018),
    v("$PUCSORGBOTTOM", Bd3, R2000, R2018),
    v("$PUCSORGLEFT", Bd3, R2000, R2018),
    v("$PUCSORGRIGHT", Bd3, R2000, R2018),
    v("$PUCSORGFRONT", Bd3, R2000, R2018),
    v("$PUCSORGBACK", Bd3, R2000, R2018),
    v("$INSBASE", Bd3, R13, R2018),
    v("$EXTMIN", Bd3, R13, R2018),
    v("$EXTMAX", Bd3, R13, R2018),
    v("$LIMMIN", Rd2, R13, R2018),
    v("$LIMMAX", Rd2, R13, R2018),
    v("$ELEVATION", Bd, R13, R2018),
    v("$UCSORG", Bd3, R13, R2018),
    v("$UCSXDIR", Bd3, R13, R2018),
    v("$UCSYDIR", Bd3, R13, R2018),
    v("$UCSNAME", H, R13, R2018),
    v("$UCSORTHOREF", H, R2000, R2018),
    v("$UCSORTHOVIEW", Bs, R2000, R2018),
    v("$UCSBASE", H, R2000, R2018),
    v("$UCSORGTOP", Bd3, R2000, R2018),
    v("$UCSORGBOTTOM", Bd3, R2000, R2018),
    v("$UCSORGLEFT", Bd3, R2000, R2018),
    v("$UCSORGRIGHT", Bd3, R2000, R2018),
    v("$UCSORGFRONT", Bd3, R2000, R2018),
    v("$UCSORGBACK", Bd3, R2000, R2018),
    v("$DIMPOST", Tv, R2000, R2018),
    v("$DIMAPOST", Tv, R2000, R2018),
    v("$DIMTOL", B, R13, R14),
    v("$DIMLIM", B, R13, R14),
    v("$DIMTIH", B, R13, R14),
    v("$DIMTOH", B, R13, R14),
    v("$DIMSE1", B, R13, R14),
    v("$DIMSE2", B, R13, R14),
    v("$DIMALT", B, R13, R14),
    v("$DIMTOFL", B, R13, R14),
    v("$DIMSAH", B, R13, R14),
    v("$DIMTIX", B, R13, R14),
    v("$DIMSOXD", B, R13, R14),
    v("$DIMALTD", Rc, R13, R14),
    v("$DIMZIN", Rc, R13, R14),
    v("$DIMSD1", B, R13, R14),
    v("$DIMSD2", B, R13, R14),
    v("$DIMTOLJ", Rc, R13, R14),
    v("$DIMJUST", Rc, R13, R14),
    v("$DIMFIT", Rc, R13, R14),
    v("$DIMUPT", B, R13, R14),
    v("$DIMTZIN", Rc, R13, R14),
    v("$DIMALTZ", Rc, R13, R14),
    v("$DIMALTTZ", Rc, R13, R14),
    v("$DIMTAD", Rc, R13, R14),
    v("$DIMUNIT", Bs, R13, R14),
    v("$DIMAUNIT", Bs, R13, R14),
    v("$DIMDEC", Bs, R13, R14),
    v("$DIMTDEC", Bs, R13, R14),
    v("$DIMALTU", Bs, R13, R14),
    v("$DIMALTTD", Bs, R13, R14),
    v("$DIMTXSTY", H, R13, R14),
    v("$DIMSCALE", Bd, R13, R2018),
    v("$DIMASZ", Bd, R13, R2018),
    v("$DIMEXO", Bd, R13, R2018),
    v("$DIMDLI", Bd, R13, R2018),
    v("$DIMEXE", Bd, R13, R2018),
    v("$DIMRND", Bd, R13, R2018),
    v("$DIMDLE", Bd, R13, R2018),
    v("$DIMTP", Bd, R13, R2018),
    v("$DIMTM", Bd, R13, R2018),
    v("$DIMFXL", Bd, R2007, R2018),
    v("$DIMJOGANG", Bd, R2007, R2018),
    v("$DIMTFILL", Bs, R2007, R2018),
    v("$DIMTFILLCLR", Cmc, R2007, R2018),
    v("$DIMTOL", B, R2000, R2018),
    v("$DIMLIM", B, R2000, R2018),
    v("$DIMTIH", B, R2000, R2018),
    v("$DIMTOH", B, R2000, R2018),
    v("$DIMSE1", B, R2000, R2018),
    v("$DIMSE2", B, R2000, R2018),
    v("$DIMTAD", Bs, R2000, R2018),
    v("$DIMZIN", Bs, R2000, R2018),
    v("$DIMAZIN", Bs, R2000, R2018),
    v("$DIMARCSYM", Bs, R2007, R2018),
    v("$DIMTXT", Bd, R13, R2018),
    v("$DIMCEN", Bd, R13, R2018),
    v("$DIMTSZ", Bd, R13, R2018),
    v("$DIMALTF", Bd, R13, R2018),
    v("$DIMLFAC", Bd, R13, R2018),
    v("$DIMTVP", Bd, R13, R2018),
    v("$DIMTFAC", Bd, R13, R2018),
    v("$DIMGAP", Bd, R13, R2018),
    v("$DIMPOST", Tv, R13, R14),
    v("$DIMAPOST", Tv, R13, R14),
    v("$DIMBLK", Tv, R13, R14),
    v("$DIMBLK1", Tv, R13, R14),
    v("$DIMBLK2", Tv, R13, R14),
    v("$DIMALTRND", Bd, R2000, R2018),
    v("$DIMALT", B, R2000, R2018),
    v("$DIMALTD", Bs, R2000, R2018),
    v("$DIMTOFL", B, R2000, R2018),
    v("$DIMSAH", B, R2000, R2018),
    v("$DIMTIX", B, R2000, R2018),
    v("$DIMSOXD", B, R2000, R2018),
    v("$DIMCLRD", Cmc, R13, R2018),
    v("$DIMCLRE", Cmc, R13, R2018),
    v("$DIMCLRT", Cmc, R13, R2018),
    v("$DIMADEC", Bs, R2000, R2018),
    v("$DIMDEC", Bs, R2000, R2018),
    v("$DIMTDEC", Bs, R2000, R2018),
    v("$DIMALTU", Bs, R2000, R2018),
    v("$DIMALTTD", Bs, R2000, R2018),
    v("$DIMAUNIT", Bs, R2000, R2018),
    v("$DIMFRAC", Bs, R2000, R2018),
    v("$DIMLUNIT", Bs, R2000, R2018),
    v("$DIMDSEP", Bs, R2000, R2018),
    v("$DIMTMOVE", Bs, R2000, R2018),
    v("$DIMJUST", Bs, R2000, R2018),
    v("$DIMSD1", B, R2000, R2018),
    v("$DIMSD2", B, R2000, R2018),
    v("$DIMTOLJ", Bs, R2000, R2018),
    v("$DIMTZIN", Bs, R2000, R2018),
    v("$DIMALTZ", Bs, R2000, R2018),
    v("$DIMALTTZ", Bs, R2000, R2018),
    v("$DIMUPT", B, R2000, R2018),
    v("$DIMATFIT", Bs, R2000, R2018),
    v("$DIMFXLON", B, R2007, R2018),
    v("$DIMTXTDIRECTION", B, R2010, R2018),
    v("$DIMALTMZF", Bd, R2010, R2018),
    v("$DIMALTMZS", Tv, R2010, R2018),
    v("$DIMMZF", Bd, R2010, R2018),
    v("$DIMMZS", Tv, R2010, R2018),
    v("$DIMTXSTY", H, R2000, R2018),
    v("$DIMLDRBLK", H, R2000, R2018),
    v("$DIMBLK", H, R2000, R2018),
    v("$DIMBLK1", H, R2000, R2018),
    v("$DIMBLK2", H, R2000, R2018),
    v("$DIMLTYPE", H, R2007, R2018),
    v("$DIMLTEX1", H, R2007, R2018),
    v("$DIMLTEX2", H, R2007, R2018),
    v("$DIMLWD", Bs, R2000, R2018),
    v("$DIMLWE", Bs, R2000, R2018),
    v("BLOCK_CONTROL_OBJECT", H, R13, R2018),
    v("LAYER_CONTROL_OBJECT", H, R13, R2018),
    v("STYLE_CONTROL_OBJECT", H, R13, R2018),
    v("LINETYPE_CONTROL_OBJECT", H, R13, R2018),
    v("VIEW_CONTROL_OBJECT", H, R13, R2018),
    v("UCS_CONTROL_OBJECT", H, R13, R2018),
    v("VPORT_CONTROL_OBJECT", H, R13, R2018),
    v("APPID_CONTROL_OBJECT", H, R13, R2018),
    v("DIMSTYLE_CONTROL_OBJECT", H, R13, R2018),
    v("VIEWPORT_ENTITY_HEADER_CONTROL_OBJECT", H, R13, R2000),
    v("DICTIONARY_ACAD_GROUP", H, R13, R2018),
    v("DICTIONARY_ACAD_MLINESTYLE", H, R13, R2018),
    v("DICTIONARY_NAMED_OBJECTS", H, R13, R2018),
    v("$TSTACKALIGN", Bs, R2000, R2018),
    v("$TSTACKSIZE", Bs, R2000, R2018),
    v("$HYPERLINKBASE", Tv, R2000, R2018),
    v("$STYLESHEET", Tv, R2000, R2018),
    v("DICTIONARY_LAYOUTS", H, R2000, R2018),
    v("DICTIONARY_PLOTSETTINGS", H, R2000, R2018),
    v("DICTIONARY_PLOTSTYLES", H, R2000, R2018),
    v("DICTIONARY_MATERIALS", H, R2004, R2018),
    v("DICTIONARY_COLORS", H, R2004, R2018),
    v("DICTIONARY_VISUALSTYLE", H, R2007, R2018),
    v("UNKNOWN_H_1", H, R2013, R2018),
    v("FLAGS", Bl, R2000, R2018),
    v("$CELWEIGHT", Lineweight, R2000, R2018),
    v(
        "$ENDCAPS",
        Bits {
            mask: 0x0060,
            invert: false,
        },
        R2000,
        R2018,
    ),
    v(
        "$JOINSTYLE",
        Bits {
            mask: 0x0180,
            invert: false,
        },
        R2000,
        R2018,
    ),
    v(
        "$LWDISPLAY",
        Bits {
            mask: 0x0200,
            invert: true,
        },
        R2000,
        R2018,
    ),
    v(
        "$XEDIT",
        Bits {
            mask: 0x0400,
            invert: true,
        },
        R2000,
        R2018,
    ),
    v(
        "$EXTNAMES",
        Bits {
            mask: 0x0800,
            invert: false,
        },
        R2000,
        R2018,
    ),
    v(
        "$PSTYLEMODE",
        Bits {
            mask: 0x2000,
            invert: false,
        },
        R2000,
        R2018,
    ),
    v(
        "$OLESTARTUP",
        Bits {
            mask: 0x4000,
            invert: false,
        },
        R2000,
        R2018,
    ),
    v("$INSUNITS", Bs, R2000, R2018),
    v("$CEPSNTYPE", Bs, R2000, R2018),
    v("$CPSNID", HWhen("$CEPSNTYPE", 3), R2000, R2018),
    v("$FINGERPRINTGUID", Tv, R2000, R2018),
    v("$VERSIONGUID", Tv, R2000, R2018),
    v("$SORTENTS", Rc, R2004, R2018),
    v("$INDEXCTL", Rc, R2004, R2018),
    v("$HIDETEXT", Rc, R2004, R2018),
    v("$XCLIPFRAME", Rc, R2004, R2018),
    v("$DIMASSOC", Rc, R2004, R2018),
    v("$HALOGAP", Rc, R2004, R2018),
    v("$OBSCOLOR", Bs, R2004, R2018),
    v("$INTERSECTIONCOLOR", Bs, R2004, R2018),
    v("$OBSLTYPE", Rc, R2004, R2018),
    v("$INTERSECTIONDISPLAY", Rc, R2004, R2018),
    v("$PROJECTNAME", Tv, R2004, R2018),
    v("BLOCK_RECORD_PAPER_SPACE", H, R13, R2018),
    v("BLOCK_RECORD_MODEL_SPACE", H, R13, R2018),
    v("LTYPE_BYLAYER", H, R13, R2018),
    v("LTYPE_BYBLOCK", H, R13, R2018),
    v("LTYPE_CONTINUOUS", H, R13, R2018),
    v("$CAMERADISPLAY", B, R2007, R2018),
    v("UNKNOWN_BL_9", Bl, R2007, R2018),
    v("UNKNOWN_BL_10", Bl, R2007, R2018),
    v("UNKNOWN_BD_5", Bd, R2007, R2018),
    v("$STEPSPERSEC", Bd, R2007, R2018),
    v("$STEPSIZE", Bd, R2007, R2018),
    v("$3DDWFPREC", Bd, R2007, R2018),
    v("$LENSLENGTH", Bd, R2007, R2018),
    v("$CAMERAHEIGHT", Bd, R2007, R2018),
    v("$SOLIDHIST", Rc, R2007, R2018),
    v("$SHOWHIST", Rc, R2007, R2018),
    v("$PSOLWIDTH", Bd, R2007, R2018),
    v("$PSOLHEIGHT", Bd, R2007, R2018),
    v("$LOFTANG1", Bd, R2007, R2018),
    v("$LOFTANG2", Bd, R2007, R2018),
    v("$LOFTMAG1", Bd, R2007, R2018),
    v("$LOFTMAG2", Bd, R2007, R2018),
    v("$LOFTPARAM", Bs, R2007, R2018),
    v("$LOFTNORMALS", Rc, R2007, R2018),
    v("$LATITUDE", Bd, R2007, R2018),
    v("$LONGITUDE", Bd, R2007, R2018),
    v("$NORTHDIRECTION", Bd, R2007, R2018),
    v("$TIMEZONE", Bl, R2007, R2018),
    v("$LIGHTGLYPHDISPLAY", Rc, R2007, R2018),
    v("$TILEMODELIGHTSYNCH", Rc, R2007, R2018),
    v("$DWFFRAME", Rc, R2007, R2018),
    v("$DGNFRAME", Rc, R2007, R2018),
    v("$REALWORLDSCALE", B, R2007, R2018),
    v("$INTERFERECOLOR", Cmc, R2007, R2018),
    v("$INTERFEREOBJVS", H, R2007, R2018),
    v("$INTERFEREVPVS", H, R2007, R2018),
    v("$DRAGVS", H, R2007, R2018),
    v("$CSHADOW", Rc, R2007, R2018),
    v("$SHADOWPLANELOCATION", Bd, R2007, R2018),
    v("UNKNOWN_BS_2", TailBs, R14, R2018),
    v("UNKNOWN_BS_3", TailBs, R14, R2018),
    v("UNKNOWN_BS_4", TailBs, R14, R2018),
    v("UNKNOWN_BS_5", TailBs, R14, R2018),
    v("UNKNOWN_B_2", TailB, R2004, R2018),
    v("UNKNOWN_B_3", TailB, R2004, R2018),
    v("UNKNOWN_B_4", TailB, R2004, R2018),
    v("UNKNOWN_B_5", TailB, R2004, R2018),
    v("UNKNOWN_B_6", TailB, R2004, R2018),
];

/// The slots a release carries, in stream order.
pub fn rows_for(rel: Rel) -> impl Iterator<Item = &'static Var> {
    TABLE.iter().filter(move |r| r.in_release(rel))
}
