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
}

impl<T> Decoded<T> {
    /// Build a clean-decode result (complete = true, no warnings).
    pub fn complete(value: T) -> Self {
        Decoded {
            value,
            diagnostics: Diagnostics::default(),
            complete: true,
        }
    }

    /// Build a partial-decode result (complete = false; caller
    /// supplies the accumulated diagnostics).
    pub fn partial(value: T, diagnostics: Diagnostics) -> Self {
        Decoded {
            value,
            diagnostics,
            complete: false,
        }
    }

    /// Map the inner value without touching diagnostics.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Decoded<U> {
        Decoded {
            value: f(self.value),
            diagnostics: self.diagnostics,
            complete: self.complete,
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
}
