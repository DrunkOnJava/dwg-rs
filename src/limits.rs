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
//! | `max_header_bytes`             |      1_048_576 | Largest `AcDb:Header` section decoded         |
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
    /// Largest decompressed `AcDb:Header` section
    /// ([`crate::header_vars::HeaderVars::decode`]) accepted, in bytes.
    pub max_header_bytes: usize,
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
            max_header_bytes: 1 << 20,
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
            max_header_bytes: 1 << 16,
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
            max_header_bytes: 1 << 24,
        }
    }
}

/// Caller-configurable safety limits for graph iteration — the
/// handle-driven walks that resolve owner chains, reactor back-refs,
/// and block expansions.
///
/// Separate from [`ParseLimits`] because graph iteration has its own
/// DoS vectors (cycles, unbounded block nesting, reactor bombs) that
/// aren't covered by per-entity decoder caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkerLimits {
    /// Maximum handles visited in a single graph walk. Bounds the
    /// work done by owner-chain walks, reactor traversals, and
    /// block-flatten operations.
    pub max_handles: usize,
    /// Maximum total bytes scanned across all handles resolved in a
    /// single walk. Prevents an adversarial file whose handle map
    /// points everywhere from re-reading the object stream forever.
    pub max_scan_bytes: usize,
    /// Maximum depth of block-expansion nesting. AutoCAD's own
    /// practical limit is ~32; this cap defaults higher with room for
    /// hand-authored weirdness.
    pub max_block_nesting: usize,
    /// Maximum unanswered references a lossy walk keeps in
    /// [`crate::object::ObjectWalkSummary::unanswered_references`].
    /// Past it the walk still counts them, in
    /// `unanswered_references_suppressed`, but stores none, so a crafted
    /// handle stream (every byte a one-byte relative reference) cannot
    /// grow the summary without bound (#81).
    pub max_retained_references: usize,
}

impl Default for WalkerLimits {
    fn default() -> Self {
        Self::safe()
    }
}

/// `n` bytes as a `usize`, capped at `usize::MAX` where `usize` is 32 bits.
const fn saturating_bytes(n: u64) -> usize {
    if n > usize::MAX as u64 {
        usize::MAX
    } else {
        n as usize
    }
}

impl WalkerLimits {
    /// Conservative default profile. Fits every real-world drawing
    /// encountered during dwg-rs development.
    pub const fn safe() -> Self {
        Self {
            max_handles: 1_000_000,
            max_scan_bytes: 512 * 1024 * 1024,
            max_block_nesting: 128,
            max_retained_references: 100_000,
        }
    }

    /// Tighter profile for untrusted-upload contexts.
    pub const fn paranoid() -> Self {
        Self {
            max_handles: 100_000,
            max_scan_bytes: 64 * 1024 * 1024,
            max_block_nesting: 32,
            max_retained_references: 10_000,
        }
    }

    /// Looser profile for stress-test fixtures.
    pub const fn permissive() -> Self {
        Self {
            max_handles: 10_000_000,
            max_scan_bytes: saturating_bytes(4 << 30),
            max_block_nesting: 512,
            max_retained_references: 1_000_000,
        }
    }
}

/// Top-level safety profile applied at [`crate::DwgFile::open_with_limits`].
///
/// Bundles the three load-bearing caps:
/// - [`max_file_bytes`](Self::max_file_bytes) — refuse oversize input
///   before reading bytes (SEC-08).
/// - [`max_section_bytes`](Self::max_section_bytes) — per-section cap
///   applied after decompression.
/// - [`decompress`](Self::decompress) — the LZ77 caps from
///   [`crate::lz77::DecompressLimits`].
///
/// Plus the per-parse and per-walker profiles ([`ParseLimits`] +
/// [`WalkerLimits`]) so callers can specify a single struct and have
/// every downstream cap honored coherently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenLimits {
    /// Maximum file size (bytes) the open path will read. Compared
    /// against the file's metadata BEFORE allocating buffers, so an
    /// adversarial multi-GB DWG cannot trigger an OOM allocation.
    pub max_file_bytes: u64,
    /// Maximum decompressed bytes for a single section read. Section
    /// reads beyond this cap return an error.
    pub max_section_bytes: usize,
    /// LZ77 decompression caps applied to every section unpack.
    pub decompress: crate::lz77::DecompressLimits,
    /// Parser caps (handle map size, XDATA loop depth, entity count
    /// caps) applied during decode.
    pub parse: ParseLimits,
    /// Graph-iteration caps applied to handle-walking + block expansion.
    pub walker: WalkerLimits,
}

impl Default for OpenLimits {
    fn default() -> Self {
        Self::safe()
    }
}

impl OpenLimits {
    /// Conservative default profile. Fits every real-world drawing
    /// observed during dwg-rs development; fits in a typical 4 GB
    /// container memory budget without thrashing.
    pub fn safe() -> Self {
        Self {
            max_file_bytes: 1024 * 1024 * 1024,   // 1 GiB
            max_section_bytes: 256 * 1024 * 1024, // 256 MiB
            decompress: crate::lz77::DecompressLimits::default(),
            parse: ParseLimits::safe(),
            walker: WalkerLimits::safe(),
        }
    }

    /// Tighter profile for SaaS / web-upload contexts where every
    /// cap should be conservative.
    pub fn paranoid() -> Self {
        Self {
            max_file_bytes: 100 * 1024 * 1024, // 100 MiB
            max_section_bytes: 16 * 1024 * 1024,
            decompress: crate::lz77::DecompressLimits::default(),
            parse: ParseLimits::paranoid(),
            walker: WalkerLimits::paranoid(),
        }
    }

    /// Looser profile for stress-test fixtures and corpus harnesses.
    pub fn permissive() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024 * 1024, // 16 GiB
            max_section_bytes: saturating_bytes(4 << 30),
            decompress: crate::lz77::DecompressLimits::permissive(),
            parse: ParseLimits::permissive(),
            walker: WalkerLimits::permissive(),
        }
    }
}
