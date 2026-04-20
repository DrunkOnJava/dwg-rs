//! Block-reference expansion (L5-05).
//!
//! INSERT entities reference a BLOCK_RECORD by handle; the block's
//! body holds the actual geometry (lines, arcs, polylines, nested
//! INSERTs, etc.). To render a drawing faithfully, each INSERT must
//! be expanded: walk the block's contained entities and apply the
//! INSERT's instance transform (translation + scale + rotation +
//! arbitrary-axis extrusion) to each one.
//!
//! # Honest scope
//!
//! Expansion is a graph-level operation. Today the [`crate::entities::insert::Insert`]
//! struct holds the instance parameters (insertion point, scale,
//! rotation, extrusion) but **not** the block-record handle that
//! identifies which block body to walk — that lives in the
//! trailing-handle stream, which has a documented decode gap in
//! `src/graph.rs`. Callers must therefore supply the block handle
//! out-of-band (e.g. via a future trailing-handle decoder or a
//! manual lookup against [`crate::HandleMap`]).
//!
//! This module provides the walk + transform-composition pipeline
//! ready to consume that handle once the gap closes.
//!
//! # Cycle detection
//!
//! Adversarial files can construct cyclic INSERT chains (A → B, B → A).
//! [`expand_insert`] detects cycles by tracking visited block-record
//! handles through [`ExpansionContext`] and returns
//! [`crate::Error::Unsupported`] at the point the cycle closes —
//! mirroring the [`crate::graph::walk_with_cycle_detection`] discipline.
//!
//! # Depth cap
//!
//! Nested-block recursion is bounded by [`ExpansionContext::max_depth`]
//! (default 16 — AutoCAD's historical upper bound on interactive block
//! nesting). Exceeding it returns the same `Unsupported` variant as a
//! cycle would; callers can tighten via
//! [`ExpansionContext::with_max_depth`] when processing untrusted input.

use std::collections::HashSet;

use crate::entities::DecodedEntity;
use crate::entity_geometry;
use crate::error::{Error, Result};
use crate::geometry::Transform3;

/// Per-walk state for [`expand_insert`].
///
/// Tracks which block handles are currently "on the stack" of the
/// recursion so a nested INSERT that refers back to an ancestor
/// block can be diagnosed as a cycle instead of looping forever.
#[derive(Debug, Clone)]
pub struct ExpansionContext {
    visited: HashSet<u64>,
    depth: usize,
    max_depth: usize,
}

impl Default for ExpansionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpansionContext {
    /// Fresh context with the default `max_depth = 16`.
    pub fn new() -> Self {
        Self {
            visited: HashSet::new(),
            depth: 0,
            max_depth: 16,
        }
    }

    /// Override the nested-block depth cap. Use a tighter value when
    /// processing adversarial input.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Current recursion depth (0 at the top-level INSERT).
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Configured max-depth cap.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Has this block handle already been entered?
    pub fn is_visited(&self, block_handle: u64) -> bool {
        self.visited.contains(&block_handle)
    }
}

/// One entry in the expansion result: an entity that was reached via
/// the block-expansion walk, plus the accumulated world-space
/// transform to apply when rendering it.
#[derive(Debug, Clone)]
pub struct ExpandedEntity {
    /// The decoded entity (LINE, CIRCLE, nested INSERT, ...).
    pub entity: DecodedEntity,
    /// Accumulated transform from drawing-WCS to this entity's
    /// coordinate frame. Composes the ancestor INSERTs' individual
    /// insertion transforms in outer-to-inner order.
    pub accumulated_transform: Transform3,
    /// Recursion depth at which this entity was emitted (0 = direct
    /// member of the top-level INSERT's block).
    pub depth: usize,
}

/// Expand a single INSERT into its constituent entities.
///
/// Callers must supply:
/// - `insert` — the decoded INSERT whose instance transform is applied.
/// - `block_handle` — the block-record handle this INSERT references.
///   Comes from the (not-yet-decoded) trailing-handle stream; callers
///   without a trailing-handle decoder can pass a placeholder once
///   they've resolved it by other means.
/// - `ctx` — per-walk cycle-detection + depth state.
/// - `parent_transform` — transform accumulated by outer ancestor
///   INSERTs (or [`Transform3::identity`] at the top level).
/// - `block_body_lookup` — closure that returns the entities inside a
///   block, keyed by block-record handle.
///
/// Each nested INSERT inside the block recursively expands. Both the
/// INSERT marker itself and the expanded content are emitted — the
/// caller chooses whether to render the instancing layer.
///
/// # Errors
///
/// - [`Error::Unsupported`] on cycle or depth-cap exceeded.
/// - Whatever `block_body_lookup` surfaces (e.g. handle not found).
pub fn expand_insert<F>(
    insert: &crate::entities::insert::Insert,
    block_handle: u64,
    ctx: &mut ExpansionContext,
    parent_transform: &Transform3,
    block_body_lookup: &F,
) -> Result<Vec<ExpandedEntity>>
where
    F: Fn(u64) -> Result<Vec<(DecodedEntity, Option<u64>)>>,
{
    if ctx.depth >= ctx.max_depth {
        return Err(Error::Unsupported {
            feature: format!(
                "block expansion depth {} exceeds cap {}",
                ctx.depth, ctx.max_depth
            ),
        });
    }

    if ctx.visited.contains(&block_handle) {
        return Err(Error::Unsupported {
            feature: format!("block-expansion cycle: handle 0x{block_handle:x} already on stack"),
        });
    }

    // Compose this INSERT's instance transform onto the parent.
    let instance_transform = entity_geometry::insert_to_transform(insert);
    let accumulated = parent_transform.compose(&instance_transform);

    ctx.visited.insert(block_handle);
    ctx.depth += 1;

    let result = (|| -> Result<Vec<ExpandedEntity>> {
        let body = block_body_lookup(block_handle)?;
        let mut out = Vec::with_capacity(body.len());
        for (child, child_block_handle) in body {
            // Emit the entity itself (nested INSERT or otherwise).
            out.push(ExpandedEntity {
                entity: child.clone(),
                accumulated_transform: accumulated,
                depth: ctx.depth,
            });
            // Then recurse if it's a nested INSERT with a known block handle.
            if let DecodedEntity::Insert(nested) = &child {
                if let Some(nested_handle) = child_block_handle {
                    let sub =
                        expand_insert(nested, nested_handle, ctx, &accumulated, block_body_lookup)?;
                    out.extend(sub);
                }
            }
        }
        Ok(out)
    })();

    ctx.depth -= 1;
    ctx.visited.remove(&block_handle);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::insert::Insert;
    use crate::entities::{Point3D, Vec3D};

    fn insert_at(x: f64, y: f64) -> Insert {
        Insert {
            insertion_point: Point3D { x, y, z: 0.0 },
            scale: Point3D {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            rotation: 0.0,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            has_attribs: false,
        }
    }

    #[test]
    fn expansion_context_tracks_depth_and_visited() {
        let ctx = ExpansionContext::new();
        assert_eq!(ctx.depth(), 0);
        assert!(!ctx.is_visited(0x42));
        assert_eq!(ctx.max_depth(), 16);
    }

    #[test]
    fn with_max_depth_overrides_default() {
        let ctx = ExpansionContext::new().with_max_depth(4);
        assert_eq!(ctx.max_depth(), 4);
    }

    #[test]
    fn depth_cap_rejects_overflow() {
        let insert = insert_at(0.0, 0.0);
        let mut ctx = ExpansionContext::new().with_max_depth(0);
        let lookup = |_h: u64| Ok(Vec::new());
        let r = expand_insert(&insert, 0x42, &mut ctx, &Transform3::identity(), &lookup);
        assert!(matches!(r, Err(Error::Unsupported { .. })));
    }

    #[test]
    fn cycle_detection_rejects_reentrant_block() {
        let insert_a = insert_at(0.0, 0.0);
        let insert_b = insert_a.clone();

        let lookup = move |h: u64| -> Result<Vec<(DecodedEntity, Option<u64>)>> {
            if h == 0xA {
                Ok(vec![(
                    DecodedEntity::Insert(insert_b.clone()),
                    Some(0xA), // points back at self → 1-cycle
                )])
            } else {
                Ok(Vec::new())
            }
        };

        let mut ctx = ExpansionContext::new();
        let r = expand_insert(&insert_a, 0xA, &mut ctx, &Transform3::identity(), &lookup);
        assert!(matches!(r, Err(Error::Unsupported { .. })));
    }

    #[test]
    fn empty_block_body_yields_empty_expansion() {
        let insert = insert_at(10.0, 20.0);
        let mut ctx = ExpansionContext::new();
        let lookup = |_h: u64| Ok(Vec::new());
        let expanded =
            expand_insert(&insert, 0x1234, &mut ctx, &Transform3::identity(), &lookup).unwrap();
        assert!(expanded.is_empty());
        assert_eq!(ctx.depth, 0);
        assert!(ctx.visited.is_empty());
    }

    #[test]
    fn single_level_expansion_emits_block_body() {
        // Block 0xB contains one LINE entity. Expanding an INSERT
        // at (10, 20) that references block 0xB yields a single
        // ExpandedEntity at depth 1 with the translated transform.
        use crate::entities::line::Line;

        let line = Line {
            start: Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            end: Point3D {
                x: 5.0,
                y: 0.0,
                z: 0.0,
            },
            thickness: 0.0,
            extrusion: Vec3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            is_2d: true,
        };
        let line_decoded = DecodedEntity::Line(line);

        let lookup = move |h: u64| -> Result<Vec<(DecodedEntity, Option<u64>)>> {
            if h == 0xB {
                Ok(vec![(line_decoded.clone(), None)])
            } else {
                Ok(Vec::new())
            }
        };

        let insert = insert_at(10.0, 20.0);
        let mut ctx = ExpansionContext::new();
        let expanded =
            expand_insert(&insert, 0xB, &mut ctx, &Transform3::identity(), &lookup).unwrap();
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].depth, 1);
        // Depth + visited restored on exit.
        assert_eq!(ctx.depth, 0);
        assert!(ctx.visited.is_empty());
    }
}
