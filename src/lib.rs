//! dwg-rs · Apache-2 Rust reader for Autodesk DWG files (R13 → R2018 / AC1032).
//!
//! Clean-room implementation against the Open Design Alliance's freely-
//! redistributable *Open Design Specification for .dwg files* (v5.4.1). No
//! Autodesk SDK, no ODA SDK, no GPL-3 dependency.
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
//! # Version coverage
//!
//! | Magic    | Release          | Year | Coverage             |
//! |----------|------------------|------|----------------------|
//! | `AC1014` | R14              | 1997 | Full                 |
//! | `AC1015` | 2000 / 2000i / 2002 | 1999 | Full               |
//! | `AC1018` | 2004 / 2005 / 2006  | 2003 | Full               |
//! | `AC1021` | 2007 / 2008 / 2009  | 2006 | Partial (Sec_Mask)  |
//! | `AC1024` | 2010 / 2011 / 2012  | 2009 | Full               |
//! | `AC1027` | 2013 → 2017         | 2012 | Full               |
//! | `AC1032` | 2018 → 2025+        | 2017 | Full               |
//!
//! # Module map
//!
//! - [`bitcursor`] / [`bitwriter`] — bit-level primitives (spec §2)
//! - [`cipher`] — R2004+ XOR magic-sequence + Sec_Mask page masking
//! - [`crc`] — CRC-8 (spec §2.14.1) and CRC-32 (IEEE)
//! - [`lz77`] / [`lz77_encode`] — LZ77 de/compression (spec §4.7)
//! - [`reed_solomon`] — (255,239) FEC over GF(256), defensive-only
//! - [`header`] / [`header_vars`] — file-open header + `AcDb:Header` parse
//! - [`section`] / [`section_map`] / [`section_writer`] — page + section layer
//! - [`handle_map`] / [`classes`] — object-stream cross-ref tables
//! - [`metadata`] — SummaryInfo / AppInfo / Preview / FileDepList
//! - [`object`] / [`object_type`] / [`common_entity`] — object-stream walker
//! - [`entities`] — per-entity decoders (LINE, CIRCLE, TEXT, MTEXT, INSERT,
//!   DIMENSION family, HATCH, MLEADER, VIEWPORT, ...)
//! - [`tables`] — symbol-table entries (LAYER, LTYPE, STYLE, VIEW, UCS,
//!   VPORT, APPID, DIMSTYLE, BLOCK_RECORD)
//! - [`objects`] — DICTIONARY / XRECORD / `*_CONTROL`
//! - [`r2007`] — R2007-specific Sec_Mask two-layer obfuscation (layer 1 done)
//! - [`file_writer`] — scaffolded inverse of [`reader::DwgFile`]
//!
//! See [`ARCHITECTURE.md`](https://github.com/DrunkOnJava/dwg-rs/blob/main/ARCHITECTURE.md)
//! for the full design document.
//!
//! # Safety
//!
//! The entire crate is `#![deny(unsafe_code)]`. All parsing returns
//! `Result<T, Error>` — no panics on malformed input. Defensive caps
//! bound runaway allocations from adversarial files. See
//! [`SECURITY.md`](https://github.com/DrunkOnJava/dwg-rs/blob/main/SECURITY.md)
//! for the threat model and private vulnerability reporting.
//!
//! # Legal posture
//!
//! DWG is a trademark of Autodesk, Inc. This crate is not affiliated with,
//! authorized by, or endorsed by Autodesk. It is a clean-room third-party
//! implementation created for interoperability purposes, protected by
//! 17 U.S.C. § 1201(f) (DMCA interoperability exception) and the 2006
//! *Autodesk, Inc. v. Open Design Alliance* settlement, which explicitly
//! permits third-party DWG implementations.
//!
//! No Autodesk SDK source, no ODA SDK source, and no LibreDWG (GPL-3) source
//! was consulted at any point. The authoritative reference is the ODA's
//! freely-redistributable *Open Design Specification for .dwg files*
//! (v5.4.1), a document available separately from ODA's SDK license.

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
pub mod file_writer;
pub mod handle_map;
pub mod header;
pub mod header_vars;
pub mod lz77;
pub mod lz77_encode;
pub mod metadata;
pub mod object;
pub mod object_type;
pub mod objects;
pub mod r2007;
pub mod reader;
pub mod reed_solomon;
pub mod section;
pub mod section_map;
pub mod section_writer;
pub mod tables;
pub mod version;

pub use bitcursor::BitCursor;
pub use classes::{ClassDef, ClassMap};
pub use error::{Error, Result};
pub use handle_map::{HandleEntry, HandleMap};
pub use header_vars::HeaderVars;
pub use metadata::{AppInfo, FileDepList, FileDependency, Preview, SummaryInfo};
pub use object::{ObjectWalker, RawObject};
pub use object_type::ObjectType;
pub use reader::DwgFile;
pub use section::{Section, SectionKind};
pub use version::Version;
