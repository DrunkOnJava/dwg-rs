//! Non-entity, non-table object decoders (spec §19.5.x).
//!
//! These are the structural objects that hold cross-references
//! between everything else in a DWG: the named-object DICTIONARY
//! (root + nested), XRECORD (opaque key/value storage), and the
//! *_CONTROL objects (LAYER_CONTROL, BLOCK_CONTROL, STYLE_CONTROL,
//! etc.) that own the symbol-table entries.
//!
//! | Object          | Module               | Spec |
//! |-----------------|----------------------|------|
//! | DICTIONARY      | [`dictionary`]       | §19.5.19 |
//! | XRECORD         | [`xrecord`]          | §19.5.67 |
//! | *_CONTROL       | [`control`]          | §19.5.1..§19.5.10 |

pub mod control;
pub mod dictionary;
pub mod xrecord;
