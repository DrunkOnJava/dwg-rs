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
//! This crate is pre-alpha. Coverage below reflects what currently
//! parses end-to-end against measured corpora, not what the spec
//! describes. See `README.md` for the empirical decode-rate table.
//!
//! | Magic    | Release                 | Status                                                                                      |
//! |----------|-------------------------|---------------------------------------------------------------------------------------------|
//! | `AC1014` | R14                     | Identifier / header recognized; object-stream walker for this layout not yet implemented    |
//! | `AC1015` | 2000 / 2000i / 2002     | Identifier / header recognized; object-stream walker not yet implemented                    |
//! | `AC1018` | 2004 / 2005 / 2006      | Container parsing works; end-to-end entity decode currently low on real corpora             |
//! | `AC1021` | 2007 / 2008 / 2009      | **Deferred** — Sec_Mask layer-2 bookkeeping not yet implemented; section payloads error     |
//! | `AC1024` | 2010 / 2011 / 2012      | Container parsing works; partial entity decode (see README for measured coverage)           |
//! | `AC1027` | 2013 → 2017             | Container parsing works; best current entity coverage, still pre-alpha                      |
//! | `AC1032` | 2018 → 2025+            | Object walker works on sample; typed entity decode has known correctness gaps on real files |
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
//! - [`graph`] — Phase 5 handle-driven traversal helpers (owner / reactor
//!   chains, layer / linetype / style / dimstyle resolution)
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
//! "Autodesk", "AutoCAD", and "DWG" are trademarks of Autodesk, Inc.
//! This crate is not affiliated with, authorized by, or endorsed by
//! Autodesk. It is intended as a clean-room interoperability
//! implementation. It does not use Autodesk SDK source, ODA SDK
//! source, or GPL-licensed DWG implementation source. Users with
//! specific legal constraints should evaluate the project with their
//! own counsel; `NOTICE` summarizes the relevant public authority
//! that typically supports independent file-format reverse
//! engineering for interoperability.
//!
//! No Autodesk SDK source, no ODA SDK source, and no LibreDWG (GPL-3) source
//! was consulted at any point. The authoritative reference is the ODA's
//! freely-redistributable *Open Design Specification for .dwg files*
//! (v5.4.1), a document available separately from ODA's SDK license.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod api;
pub mod bitcursor;
pub mod bitwriter;
pub mod cipher;
pub mod classes;
pub mod color;
pub mod common_entity;
pub mod crc;
pub mod curve;
pub mod dxf;
pub mod dxf_sections;
pub mod entities;
pub mod entity_geometry;
pub mod error;
pub mod file_writer;
pub mod geometry;
pub mod graph;
pub mod handle_map;
pub mod header;
pub mod header_vars;
pub mod limits;
pub mod lz77;
pub mod lz77_encode;
pub mod metadata;
pub mod object;
pub mod object_type;
pub mod objects;
pub mod python_stubs;
pub mod r2007;
pub mod reader;
pub mod reed_solomon;
pub mod section;
pub mod section_map;
pub mod section_writer;
pub mod svg;
pub mod tables;
pub mod version;

pub use bitcursor::BitCursor;
pub use classes::{ClassDef, ClassMap};
pub use error::{Error, Result};
pub use handle_map::{HandleEntry, HandleMap};
pub use header_vars::HeaderVars;
pub use limits::{ParseLimits, WalkerLimits};
pub use metadata::{AppInfo, FileDepList, FileDependency, Preview, SummaryInfo};
pub use object::{ObjectWalker, RawObject};
pub use object_type::ObjectType;
pub use reader::{DwgFile, ParseDiagnostics, SectionMapStatus, Summary};
pub use section::{Section, SectionKind};
pub use version::Version;
