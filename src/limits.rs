//! Configurable safety limits for the parser.
//!
//! Every fallible parse path in this crate honors a cap — a
//! maximum count, size, or iteration. Historically those caps were
//! scattered as `const MAX_*` literals in individual modules, which
//! made the threat model enumerated in `THREAT_MODEL.md` hard to
//! reason about and impossible to tighten without a PR touching
//! every site.
//!
//! [`ParseLimits`] groups those caps into a single caller-configurable
//! struct. Callers who need a tighter safety profile for specific
//! input (e.g., processing files from untrusted uploads) choose
//! [`ParseLimits::paranoid`]; callers who need headroom for
//! stress-test fixtures choose [`ParseLimits::permissive`]; the
//! default profile ([`ParseLimits::safe`], returned by
//! [`Default::default`]) matches the crate's built-in `const`
//! values and is appropriate for regular workstation drawings.
//!
//! # Integration
//!
//! As of 0.1.0-alpha.1, [`ParseLimits`] is structurally complete but
//! not yet threaded through every parser. The existing
//! [`crate::lz77::DecompressLimits`] is the load-bearing cap that
//! has been wired through the decompression pipeline. Remaining
//! caps are tracked as work items in `ROADMAP.md`; the planned
//! migration replaces each scattered `const MAX_*` with a field on
//! `ParseLimits` and threads it through the call chain.
//!
//! # Cap registry
//!
//! | Field                           | Default (safe) | Purpose                                       |
//! |--------------------------------|---------------:|-----------------------------------------------|
//! | `max_handle_entries`           |      1_000_000 | `AcDb:Handles` entry cap                      |
//! | `max_retained_dispatch_errors` |          1_000 | `DispatchSummary.errors` cap                  |
//! | `max_xdata_iterations`         |            256 | Per-entity XDATA loop bound                   |
//! | `max_class_entries`            |          4_096 | Class-map (custom-class) entry cap            |
//! | `max_lwpolyline_count`         |      1_000_000 | LWPOLYLINE point / bulge / width / id caps    |
//! | `max_image_clip_verts`         |        100_000 | IMAGE clip-boundary vertices                  |
//! | `max_leader_points`            |        100_000 | LEADER polyline points                        |
//! | `max_spline_count`             |        100_000 | SPLINE knots / control points / fit points    |
//!
//! The `paranoid` profile lowers every count by 10×; the `permissive`
//! profile raises every count by 10×. See the per-field docstrings.

/// Caller-configurable safety limits for the parser.
///
/// All fields are `usize` counts applied at each corresponding
/// parser's count-read site. Instantiate via the three named
/// profile constructors ([`safe`](Self::safe),
/// [`paranoid`](Self::paranoid), [`permissive`](Self::permissive)),
/// or override individual fields on a constructed value:
///
/// ```
/// use dwg::ParseLimits;
/// let mut limits = ParseLimits::safe();
/// limits.max_handle_entries = 50_000;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum entries the handle-map parser
    /// ([`crate::handle_map::HandleMap::parse`]) will accept before
    /// returning an error.
    pub max_handle_entries: usize,
    /// Maximum per-entity error strings retained on
    /// [`crate::entities::DispatchSummary`]. Errors beyond this cap
    /// increment `errors_suppressed` instead.
    pub max_retained_dispatch_errors: usize,
    /// Maximum iterations of the XDATA extraction loop in
    /// [`crate::common_entity`].
    pub max_xdata_iterations: usize,
    /// Maximum class-map entries the custom-class table parser will
    /// accept before bailing.
    pub max_class_entries: usize,
    /// Maximum LWPOLYLINE point / bulge / width / id count, per
    /// [`crate::entities::lwpolyline::decode`].
    pub max_lwpolyline_count: usize,
    /// Maximum IMAGE clip-boundary vertex count.
    pub max_image_clip_verts: usize,
    /// Maximum LEADER polyline points.
    pub max_leader_points: usize,
    /// Maximum SPLINE knots / control points / fit points (per
    /// individual collection, not combined).
    pub max_spline_count: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self::safe()
    }
}

impl ParseLimits {
    /// Conservative default profile that matches the existing
    /// scattered `const MAX_*` values. Appropriate for regular
    /// workstation drawings.
    pub const fn safe() -> Self {
        Self {
            max_handle_entries: 1_000_000,
            max_retained_dispatch_errors: 1_000,
            max_xdata_iterations: 256,
            max_class_entries: 4_096,
            max_lwpolyline_count: 1_000_000,
            max_image_clip_verts: 100_000,
            max_leader_points: 100_000,
            max_spline_count: 100_000,
        }
    }

    /// Every count reduced by 10× relative to [`safe`](Self::safe).
    /// Appropriate for processing files from untrusted uploads in a
    /// web context where the worst-case allocation envelope matters
    /// more than accommodating unusually-complex real drawings.
    pub const fn paranoid() -> Self {
        Self {
            max_handle_entries: 100_000,
            max_retained_dispatch_errors: 100,
            max_xdata_iterations: 64,
            max_class_entries: 512,
            max_lwpolyline_count: 100_000,
            max_image_clip_verts: 10_000,
            max_leader_points: 10_000,
            max_spline_count: 10_000,
        }
    }

    /// Every count raised by 10× relative to [`safe`](Self::safe).
    /// Appropriate for test harnesses and synthetic-fixture
    /// construction; NOT recommended for production input.
    pub const fn permissive() -> Self {
        Self {
            max_handle_entries: 10_000_000,
            max_retained_dispatch_errors: 10_000,
            max_xdata_iterations: 2_560,
            max_class_entries: 40_960,
            max_lwpolyline_count: 10_000_000,
            max_image_clip_verts: 1_000_000,
            max_leader_points: 1_000_000,
            max_spline_count: 1_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paranoid_is_strictly_lower_than_safe() {
        let safe = ParseLimits::safe();
        let paranoid = ParseLimits::paranoid();
        assert!(paranoid.max_handle_entries < safe.max_handle_entries);
        assert!(paranoid.max_retained_dispatch_errors < safe.max_retained_dispatch_errors);
        assert!(paranoid.max_xdata_iterations < safe.max_xdata_iterations);
        assert!(paranoid.max_class_entries < safe.max_class_entries);
        assert!(paranoid.max_lwpolyline_count < safe.max_lwpolyline_count);
        assert!(paranoid.max_image_clip_verts < safe.max_image_clip_verts);
        assert!(paranoid.max_leader_points < safe.max_leader_points);
        assert!(paranoid.max_spline_count < safe.max_spline_count);
    }

    #[test]
    fn permissive_is_strictly_higher_than_safe() {
        let safe = ParseLimits::safe();
        let permissive = ParseLimits::permissive();
        assert!(permissive.max_handle_entries > safe.max_handle_entries);
        assert!(permissive.max_retained_dispatch_errors > safe.max_retained_dispatch_errors);
        assert!(permissive.max_xdata_iterations > safe.max_xdata_iterations);
        assert!(permissive.max_class_entries > safe.max_class_entries);
        assert!(permissive.max_lwpolyline_count > safe.max_lwpolyline_count);
        assert!(permissive.max_image_clip_verts > safe.max_image_clip_verts);
        assert!(permissive.max_leader_points > safe.max_leader_points);
        assert!(permissive.max_spline_count > safe.max_spline_count);
    }

    #[test]
    fn default_matches_safe() {
        assert_eq!(ParseLimits::default(), ParseLimits::safe());
    }
}
