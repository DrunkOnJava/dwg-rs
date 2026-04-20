//! Public API discipline — strict vs best-effort parsing.
//!
//! The DWG format is a best-effort target. Real-world files often
//! have minor spec deviations that a careful reader can ignore; a
//! strict reader should reject them. This module defines the
//! user-facing types that let callers pick which posture they want.
//!
//! # Usage
//!
//! ```
//! use dwg::api::{ParseMode, Decoded};
//! # fn decode() -> dwg::Result<Decoded<u32>> {
//! #     Ok(Decoded::partial(42, dwg::api::Diagnostics::default()))
//! # }
//!
//! let result = decode()?;
//! if result.complete {
//!     println!("clean decode: {}", result.value);
//! } else {
//!     eprintln!("decoded with {} warnings", result.diagnostics.warnings.len());
//!     // result.value is still usable, just partial
//! }
//! # Ok::<(), dwg::Error>(())
//! ```
//!
//! # Integration with existing APIs
//!
//! [`ParseMode`] is the caller-facing knob; existing `_strict` vs
//! `_lossy` function pairs (HeaderVars, DwgFile) are the underlying
//! dispatch. New APIs take `ParseMode` as a parameter and route to
//! the corresponding pair internally.

use crate::entities::{DecodedEntity, decode_from_raw, decode_from_raw_with_class_map};
use crate::error::{Error, Result};
use crate::object_type::ObjectType;
use crate::reader::DwgFile;
use crate::version::Version;

/// Parsing posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseMode {
    /// Error on any spec deviation — appropriate for SaaS pipelines
    /// where silent partial decodes would corrupt downstream data.
    Strict,
    /// Tolerate recoverable deviations, recording them as warnings
    /// — appropriate for forensic readers, viewers, and any caller
    /// that prefers "something" over "nothing."
    #[default]
    BestEffort,
}

/// A decoded value with optional warnings + completeness flag.
///
/// Returned from lossy/best-effort entry points so callers can
/// inspect whether the result is clean (`complete == true` and
/// `diagnostics.warnings.is_empty()`) or partial.
#[derive(Debug, Clone)]
pub struct Decoded<T> {
    /// The decoded value. Always populated, even when `complete ==
    /// false` — partial values carry whatever fields were read
    /// before the decoder stopped.
    pub value: T,
    /// Accumulated diagnostics (warnings + skipped-record ratio).
    pub diagnostics: Diagnostics,
    /// `true` if every field decoded cleanly and `diagnostics.warnings`
    /// is empty.
    pub complete: bool,
    /// Bit count consumed from the source stream before the decoder
    /// either finished or aborted. `0` when not tracked.
    pub consumed_bits: u64,
    /// Name of the field the decoder stopped at when `complete ==
    /// false`. `None` for clean decodes or for partial decodes that
    /// did not capture a field marker.
    pub stopped_at_field: Option<&'static str>,
}

impl<T> Decoded<T> {
    /// Build a clean-decode result (complete = true, no warnings).
    pub fn complete(value: T) -> Self {
        Decoded {
            value,
            diagnostics: Diagnostics::default(),
            complete: true,
            consumed_bits: 0,
            stopped_at_field: None,
        }
    }

    /// Build a partial-decode result (complete = false; caller
    /// supplies the accumulated diagnostics).
    pub fn partial(value: T, diagnostics: Diagnostics) -> Self {
        Decoded {
            value,
            diagnostics,
            complete: false,
            consumed_bits: 0,
            stopped_at_field: None,
        }
    }

    /// Build a partial-decode result that records both how many bits
    /// were consumed before the decoder stopped and the name of the
    /// field that triggered the stop. Lets callers point a forensic
    /// reader at the exact failure site without reverse-engineering
    /// the bit position from the diagnostic message.
    pub fn partial_at(
        value: T,
        diagnostics: Diagnostics,
        consumed_bits: u64,
        field_name: &'static str,
    ) -> Self {
        Decoded {
            value,
            diagnostics,
            complete: false,
            consumed_bits,
            stopped_at_field: Some(field_name),
        }
    }

    /// Map the inner value without touching diagnostics.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Decoded<U> {
        Decoded {
            value: f(self.value),
            diagnostics: self.diagnostics,
            complete: self.complete,
            consumed_bits: self.consumed_bits,
            stopped_at_field: self.stopped_at_field,
        }
    }
}

/// One issue encountered during a lossy parse.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    /// Short machine-readable code (e.g. `"truncated_field"`).
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Bit offset into the section where the issue was detected.
    pub bit_offset: Option<u64>,
}

/// Accumulated diagnostics from a best-effort parse.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    /// List of recoverable issues, in encounter order.
    pub warnings: Vec<Warning>,
    /// Count of records the walker skipped (handle-map entries that
    /// failed to decode, for example).
    pub skipped_records: usize,
    /// Count of streams that errored out before producing a value.
    pub failed_streams: usize,
    /// Count of fields that produced a value but with lower
    /// confidence (e.g., clamped out-of-range integers).
    pub partial_fields: usize,
}

impl Diagnostics {
    /// Append a warning.
    pub fn warn(&mut self, code: &'static str, message: impl Into<String>) {
        self.warnings.push(Warning {
            code,
            message: message.into(),
            bit_offset: None,
        });
    }

    /// Append a warning carrying a bit offset.
    pub fn warn_at(&mut self, code: &'static str, bit_offset: u64, message: impl Into<String>) {
        self.warnings.push(Warning {
            code,
            message: message.into(),
            bit_offset: Some(bit_offset),
        });
    }

    /// Ratio of "clean" outcomes to total attempts. Returns 1.0 when
    /// no attempts failed + no warnings; 0.0 when every attempt failed.
    /// Useful as a coarse confidence score for a whole-file decode.
    pub fn confidence(&self, total_attempts: usize) -> f64 {
        if total_attempts == 0 {
            return 1.0;
        }
        let failed = self.skipped_records + self.failed_streams;
        let bad = failed + self.warnings.len() + self.partial_fields;
        let clean = total_attempts.saturating_sub(bad);
        clean as f64 / total_attempts as f64
    }

    /// `true` if no issues at all were recorded.
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
            && self.skipped_records == 0
            && self.failed_streams == 0
            && self.partial_fields == 0
    }
}

/// Decode a single object from `file` by handle, hard-failing on any
/// decoder issue (including [`Error::Unsupported`] for object types
/// the dispatcher does not yet handle).
///
/// Use this when downstream code cannot safely consume an
/// [`DecodedEntity::Unhandled`] or [`DecodedEntity::Error`] variant —
/// for example, a SaaS endpoint that contracts on every requested
/// handle being typed.
///
/// Lookup proceeds via `file.all_objects()` and matches the first
/// raw object whose handle equals `handle`. Returns
/// [`Error::Unsupported`] if the handle is not present, or if the
/// dispatcher classifies the object as `Unhandled` or `Error`.
pub fn read_object_strict(file: &DwgFile, handle: u64, version: Version) -> Result<DecodedEntity> {
    let raws = match file.all_objects() {
        Some(Ok(rs)) => rs,
        Some(Err(e)) => return Err(e),
        None => {
            return Err(Error::Unsupported {
                feature: "read_object_strict requires R2004+ object stream (no AcDb:AcDbObjects \
                          section reachable in this file)"
                    .to_string(),
            });
        }
    };
    let raw = raws
        .iter()
        .find(|r| r.handle.value == handle)
        .ok_or_else(|| Error::Unsupported {
            feature: format!("handle 0x{handle:X} not present in object stream"),
        })?;
    let class_map = file.class_map().and_then(Result::ok);
    let decoded = match (class_map.as_ref(), raw.kind) {
        (Some(cm), ObjectType::Custom(code)) => {
            decode_from_raw_with_class_map(raw, version, cm, code)
        }
        _ => decode_from_raw(raw, version),
    };
    match decoded {
        DecodedEntity::Unhandled { type_code, kind } => Err(Error::Unsupported {
            feature: format!(
                "object type 0x{type_code:X} ({kind:?}) at handle 0x{handle:X} \
                 has no decoder implementation"
            ),
        }),
        DecodedEntity::Error {
            type_code,
            kind,
            message,
        } => Err(Error::Unsupported {
            feature: format!(
                "decoder failed for type 0x{type_code:X} ({kind:?}) at handle 0x{handle:X}: \
                 {message}"
            ),
        }),
        ok => Ok(ok),
    }
}

/// Decode a single object from `file` by handle, returning a
/// [`Decoded<Option<DecodedEntity>>`] that distinguishes a clean
/// decode (`Some(entity)`, complete) from a failure
/// (`None`, not complete, with a diagnostic warning attached).
///
/// Never errors at the [`Result`] level — `Err` is reserved for the
/// section-level lookup machinery (decompression, etc.). All
/// per-object decoder issues land in `diagnostics.warnings`.
pub fn read_object_lossy(
    file: &DwgFile,
    handle: u64,
    version: Version,
) -> Decoded<Option<DecodedEntity>> {
    let raws = match file.all_objects() {
        Some(Ok(rs)) => rs,
        Some(Err(e)) => {
            let mut diag = Diagnostics::default();
            diag.warn(
                "all_objects_failed",
                format!("AcDb:AcDbObjects walk failed: {e}"),
            );
            return Decoded::partial(None, diag);
        }
        None => {
            let mut diag = Diagnostics::default();
            diag.warn(
                "object_stream_unavailable",
                "no AcDb:AcDbObjects section reachable (pre-R2004 file?)",
            );
            return Decoded::partial(None, diag);
        }
    };
    let Some(raw) = raws.iter().find(|r| r.handle.value == handle) else {
        let mut diag = Diagnostics::default();
        diag.warn(
            "handle_not_found",
            format!("handle 0x{handle:X} not present in object stream"),
        );
        return Decoded::partial(None, diag);
    };
    let class_map = file.class_map().and_then(Result::ok);
    let decoded = match (class_map.as_ref(), raw.kind) {
        (Some(cm), ObjectType::Custom(code)) => {
            decode_from_raw_with_class_map(raw, version, cm, code)
        }
        _ => decode_from_raw(raw, version),
    };
    match decoded {
        DecodedEntity::Unhandled { type_code, kind } => {
            let mut diag = Diagnostics::default();
            diag.warn(
                "object_unhandled",
                format!(
                    "object type 0x{type_code:X} ({kind:?}) at handle 0x{handle:X} \
                     has no decoder implementation"
                ),
            );
            Decoded::partial(None, diag)
        }
        DecodedEntity::Error {
            type_code,
            kind,
            message,
        } => {
            let mut diag = Diagnostics::default();
            diag.warn(
                "object_decode_error",
                format!(
                    "decoder failed for type 0x{type_code:X} ({kind:?}) at handle 0x{handle:X}: \
                     {message}"
                ),
            );
            Decoded::partial(None, diag)
        }
        ok => Decoded::complete(Some(ok)),
    }
}

/// Walk every object in `file` and return the first
/// [`Error::Unsupported`] encountered for an object type the
/// dispatcher does not know how to decode (`DecodedEntity::Unhandled`).
/// `Ok(())` only when every object successfully classified to a typed
/// variant.
///
/// SaaS pipelines that need an explicit gate ("refuse files that
/// contain anything we can't fully model") call this after opening a
/// [`DwgFile`]; lossy callers do not need to invoke it.
///
/// `Error` variants in the dispatch output are NOT treated as
/// `Unknown` here — they represent decoders that ran and failed,
/// not missing decoders. Use [`read_object_strict`] per handle to
/// surface those.
pub fn assert_no_unknown_objects(file: &DwgFile, version: Version) -> Result<()> {
    let raws = match file.all_objects() {
        Some(Ok(rs)) => rs,
        Some(Err(e)) => return Err(e),
        None => {
            return Err(Error::Unsupported {
                feature: "assert_no_unknown_objects requires R2004+ object stream (no \
                          AcDb:AcDbObjects section reachable in this file)"
                    .to_string(),
            });
        }
    };
    let class_map = file.class_map().and_then(Result::ok);
    for raw in &raws {
        let decoded = match (class_map.as_ref(), raw.kind) {
            (Some(cm), ObjectType::Custom(code)) => {
                decode_from_raw_with_class_map(raw, version, cm, code)
            }
            _ => decode_from_raw(raw, version),
        };
        if let DecodedEntity::Unhandled { type_code, .. } = decoded {
            return Err(Error::Unsupported {
                feature: format!(
                    "Unknown object type 0x{type_code:X} at handle 0x{:X}",
                    raw.handle.value
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsemode_default_is_best_effort() {
        assert_eq!(ParseMode::default(), ParseMode::BestEffort);
    }

    #[test]
    fn decoded_complete_has_no_warnings() {
        let d = Decoded::complete(42);
        assert!(d.complete);
        assert!(d.diagnostics.warnings.is_empty());
    }

    #[test]
    fn decoded_partial_preserves_diagnostics() {
        let mut diag = Diagnostics::default();
        diag.warn("test", "something went off");
        let d = Decoded::partial("value", diag);
        assert!(!d.complete);
        assert_eq!(d.diagnostics.warnings.len(), 1);
    }

    #[test]
    fn decoded_map_preserves_diagnostics() {
        let mut diag = Diagnostics::default();
        diag.warn("a", "b");
        let d = Decoded::partial(10_i32, diag);
        let mapped = d.map(|n| n * 2);
        assert_eq!(mapped.value, 20);
        assert_eq!(mapped.diagnostics.warnings.len(), 1);
        assert!(!mapped.complete);
    }

    #[test]
    fn diagnostics_is_clean_on_default() {
        let d = Diagnostics::default();
        assert!(d.is_clean());
    }

    #[test]
    fn diagnostics_warn_mutates() {
        let mut d = Diagnostics::default();
        d.warn("x", "y");
        assert!(!d.is_clean());
        assert_eq!(d.warnings[0].code, "x");
        assert_eq!(d.warnings[0].message, "y");
        assert_eq!(d.warnings[0].bit_offset, None);
    }

    #[test]
    fn diagnostics_warn_at_carries_offset() {
        let mut d = Diagnostics::default();
        d.warn_at("truncated", 1234, "section ran short");
        assert_eq!(d.warnings[0].bit_offset, Some(1234));
    }

    #[test]
    fn confidence_zero_attempts_is_one() {
        let d = Diagnostics::default();
        assert_eq!(d.confidence(0), 1.0);
    }

    #[test]
    fn confidence_all_clean_is_one() {
        let d = Diagnostics::default();
        assert_eq!(d.confidence(100), 1.0);
    }

    #[test]
    fn confidence_counts_all_failure_modes() {
        let mut d = Diagnostics {
            skipped_records: 10,
            failed_streams: 5,
            partial_fields: 3,
            ..Default::default()
        };
        d.warn("a", "b");
        d.warn("c", "d");
        // 20 bad out of 100 → confidence 0.8
        assert_eq!(d.confidence(100), 0.8);
    }

    #[test]
    fn decoded_complete_initializes_new_fields() {
        let d = Decoded::complete(7_u8);
        assert_eq!(d.consumed_bits, 0);
        assert_eq!(d.stopped_at_field, None);
    }

    #[test]
    fn decoded_partial_initializes_new_fields() {
        let d = Decoded::partial(7_u8, Diagnostics::default());
        assert_eq!(d.consumed_bits, 0);
        assert_eq!(d.stopped_at_field, None);
    }

    #[test]
    fn decoded_partial_at_captures_marker() {
        let mut diag = Diagnostics::default();
        diag.warn("truncated_field", "ran out of bits in flags");
        let d = Decoded::partial_at(0_u32, diag, 1_234, "flags");
        assert!(!d.complete);
        assert_eq!(d.consumed_bits, 1_234);
        assert_eq!(d.stopped_at_field, Some("flags"));
        assert_eq!(d.diagnostics.warnings[0].code, "truncated_field");
    }

    #[test]
    fn decoded_map_preserves_partial_at_markers() {
        let mut diag = Diagnostics::default();
        diag.warn("x", "y");
        let d = Decoded::partial_at(10_i32, diag, 64, "thickness");
        let mapped = d.map(|n| n.to_string());
        assert_eq!(mapped.value, "10");
        assert_eq!(mapped.consumed_bits, 64);
        assert_eq!(mapped.stopped_at_field, Some("thickness"));
        assert!(!mapped.complete);
    }
}
