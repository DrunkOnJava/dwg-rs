//! dwg-rs · Open reader for Autodesk DWG files (R13 → AC1032 / 2018+)
//!
//! Clean-room Rust implementation against the Open Design Alliance specification
//! v5.4.1 (`OpenDesign_Specification_for_.dwg_files.pdf`), which is a publicly
//! redistributable document. No ODA SDK, no Autodesk SDK, no GPL code — safe for
//! Apache-2 consumers.
//!
//! # What this crate reads today (Phase A)
//!
//! - **Version identification** across AC1014 (R14) → AC1032 (2018/2024/2025+),
//!   seven production formats spanning ~28 years of AutoCAD releases.
//! - **File header** for R13-R15 (AC1014, AC1015): raw bytes, image-data offset,
//!   codepage, section-locator count and records.
//! - **R2004+ header** (AC1018 → AC1032): XOR-decrypted 0x6C-byte payload,
//!   decompressed section-map + page-map structure.
//! - **Section enumeration**: named sections (AcDb:Header, AcDb:Classes,
//!   AcDb:Handles, AcDb:Objects, AcDb:SummaryInfo, AcDb:Preview, ...).
//! - **Bit-cursor primitives** per spec §2: B, BB, 3B, BS, BL, BLL, BD, MC, MS,
//!   RC, RS, RL, RD, H, TV.
//! - **CRC-8** (16-bit output, spec §2.14.1) and **CRC-32** (R2004+ system
//!   sections).
//!
//! # What is explicitly deferred
//!
//! - **Entity & object decoding** (Phase B) — requires per-class field layouts
//!   across R2000 / R2004 / R2007 / R2010 / R2013 / R2018 deltas, plus the
//!   R2007 `Sec_Mask` 2-layer bitstream.
//! - **Reed-Solomon(255,239) decode** — needed to *verify* R2004+ system
//!   sections. The reader currently trusts CRC-8 on intra-section chunks,
//!   which is sufficient for extraction.
//! - **Write support** — encoding is Phase C.
//! - **LZ77-style decompression** for section data — Phase B.
//!
//! # Version code → AutoCAD release
//!
//! | Magic    | Release          | Year | Notes                   |
//! |----------|------------------|------|-------------------------|
//! | `AC1014` | R14              | 1997 | Simple header           |
//! | `AC1015` | 2000/2000i/2002  | 1999 | R13-R15 header family   |
//! | `AC1018` | 2004/2005/2006   | 2003 | XOR-encrypted + RS FEC  |
//! | `AC1021` | 2007/2008/2009   | 2006 | Sec_Mask bitstream      |
//! | `AC1024` | 2010/2011/2012   | 2009 | Additive entity types   |
//! | `AC1027` | 2013 → 2017      | 2012 | Five-release run        |
//! | `AC1032` | 2018 → 2025+     | 2017 | Nine-year-and-counting  |
//!
//! # Quick start
//!
//! ```no_run
//! use dwg::DwgFile;
//!
//! let f = DwgFile::open("drawing.dwg")?;
//! println!("version: {}", f.version());
//! for s in f.sections() {
//!     println!("{} ({} bytes at 0x{:x})", s.name, s.size, s.offset);
//! }
//! # Ok::<(), dwg::Error>(())
//! ```
//!
//! # Legal posture
//!
//! DWG is a trademark of Autodesk, Inc. This crate is a clean-room
//! implementation under the interoperability exception of 17 U.S.C.
//! § 1201(f) and the 2006 *Autodesk v. ODA* settlement, which explicitly
//! permits third-party DWG interop. No ODA SDK or LibreDWG source was
//! consulted; the authoritative reference is the ODA's own freely-published
//! specification PDF, which is provided separately from ODA's SDK license.

#![deny(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod bitcursor;
pub mod bitwriter;
pub mod cipher;
pub mod classes;
pub mod common_entity;
pub mod crc;
pub mod entities;
pub mod error;
pub mod handle_map;
pub mod header;
pub mod lz77;
pub mod lz77_encode;
pub mod metadata;
pub mod object;
pub mod object_type;
pub mod objects;
pub mod reader;
pub mod reed_solomon;
pub mod section;
pub mod section_map;
pub mod tables;
pub mod version;

pub use bitcursor::BitCursor;
pub use classes::{ClassDef, ClassMap};
pub use error::{Error, Result};
pub use handle_map::{HandleEntry, HandleMap};
pub use metadata::{AppInfo, FileDepList, FileDependency, Preview, SummaryInfo};
pub use object::{ObjectWalker, RawObject};
pub use object_type::ObjectType;
pub use reader::DwgFile;
pub use section::{Section, SectionKind};
pub use version::Version;
