//! Deterministic handle-number allocation (L12-06, task #379).
//!
//! DWG handles are monotonic u64 keys that every object in an
//! `AcDb:AcDbObjects` stream is indexed by. The write pipeline needs a
//! predictable way to mint fresh handles that don't collide with reserved
//! fixed handles (layer 0, block_record 1, etc.) or with any handle the
//! caller plans to write explicitly.
//!
//! [`HandleAllocator`] keeps two pieces of state:
//!
//! - `next` — the monotonic counter returned by [`HandleAllocator::allocate`].
//! - `known` — a set of "taken" handles registered via [`HandleAllocator::reserve`].
//!
//! On every `allocate` the counter advances past any reserved collision
//! before returning, so allocated values are guaranteed not to collide with
//! reserved ones. Reserve then allocate is the typical pattern:
//!
//! ```
//! use dwg::handle_allocator::HandleAllocator;
//! let mut ha = HandleAllocator::starting_at(0x10);
//! ha.reserve(0x10);
//! ha.reserve(0x11);
//! let a = ha.allocate();
//! let b = ha.allocate();
//! assert_eq!(a, 0x12);
//! assert_eq!(b, 0x13);
//! ```
//!
//! This matches the handle assignment convention AutoCAD uses when saving a
//! new drawing: fixed-table handles are reserved up front, then each new
//! object gets the next unused id in ascending order. See ODA Open Design
//! Specification v5.4.1 §19.3 and §20.4.

use std::collections::HashSet;

/// Stateful allocator for DWG object handles.
///
/// See module docs for the reservation / allocation pattern.
#[derive(Debug, Clone, Default)]
pub struct HandleAllocator {
    /// The next candidate handle to hand out. `allocate` returns this
    /// value (after skipping any known collisions) and bumps the counter.
    next: u64,
    /// Handles already spoken for — either reserved by the caller or
    /// previously returned by `allocate`.
    known: HashSet<u64>,
}

impl HandleAllocator {
    /// Construct a fresh allocator. The first handle returned is `1` —
    /// handle `0` is not legal in DWG (used as a null-handle sentinel in
    /// the object stream).
    ///
    /// Use [`Self::allocate`] to mint the next handle and
    /// [`Self::reserve`] to pre-register fixed handles.
    pub fn new() -> Self {
        Self {
            next: 1,
            known: HashSet::new(),
        }
    }

    /// Construct an allocator whose counter starts at `start`. Callers that
    /// know the minimum handle in the source drawing can seed with that
    /// value to match AutoCAD's numbering.
    pub fn starting_at(start: u64) -> Self {
        Self {
            next: start.max(1),
            known: HashSet::new(),
        }
    }

    /// Return a fresh handle guaranteed not to collide with any previously
    /// reserved or allocated value. Skips forward past known collisions.
    pub fn allocate(&mut self) -> u64 {
        // Advance past any handles already in `known`.
        while self.known.contains(&self.next) {
            self.next = self.next.saturating_add(1);
        }
        let h = self.next;
        self.known.insert(h);
        self.next = self.next.saturating_add(1);
        h
    }

    /// Register `h` as taken so a later [`Self::allocate`] call won't
    /// return it. Idempotent — reserving the same value twice is cheap.
    pub fn reserve(&mut self, h: u64) {
        self.known.insert(h);
    }

    /// Total number of handles that have been reserved or allocated.
    pub fn allocated_count(&self) -> usize {
        self.known.len()
    }

    /// True when `h` has been reserved or allocated.
    pub fn contains(&self, h: u64) -> bool {
        self.known.contains(&h)
    }

    /// The handle value the next `allocate` call will *consider* first —
    /// exposed for debugging / deterministic test assertions.
    pub fn next_candidate(&self) -> u64 {
        self.next
    }
}
