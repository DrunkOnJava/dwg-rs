//! ACAD_EVALUATION_GRAPH (`AcDbEvalGraph`): the dependency graph AutoCAD keeps for
//! the evaluation of a dynamic block's parameters and actions.
//!
//! ODA spec v5.4.1 prints no layout for it. The field list is derived from
//! AutoCAD's own output under the method in `CLEANROOM.md` (2026-10-26): the data
//! bits of the 21 corpus records (R14 to R2018; three per release) against the DXF
//! object of the same handle. ARCHITECTURE.md §7ab holds the evidence.
//!
//! ```text
//! BL  group 96            (26 and 1)
//! BL  group 97            (26 and 1)
//! BL  node count          (12 and 1; no DXF group, the DXF holds that many nodes)
//! for each node:
//!   BL  group 91          (the node's index)
//!   BL  group 93          (32)
//!   BL  group 95          (an id)
//!   BL  group 92 x4       (-1 or a small integer)
//! BL  edge count          (10 and 0; no DXF group, the DXF holds that many edges)
//! for each edge:
//!   BL  group 92 (the edge's index), BL group 93 (0), BL group 94,
//!   BL  group 91 x2       (small integers),
//!   BL  group 92 x5       (-1 or a small integer)
//! ```
//!
//! Every field is a signed 32-bit `BL`; -1 is the whole four-byte form. The object
//! handle of each node (group 360) lives in the handle stream and is not decoded.
//! The meaning of the four and five link values of a node and of an edge is not
//! read from the corpus: each is tied to its DXF group by position and value only.

use crate::error::{Error, Result};
use crate::objects::small_classes::open_and_finish;
use crate::version::Version;

/// One node of an evaluation graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalGraphNode {
    /// `BL`, DXF group 91.
    pub dxf_91: i32,
    /// `BL`, DXF group 93 (32 in every record).
    pub dxf_93: i32,
    /// `BL`, DXF group 95.
    pub dxf_95: i32,
    /// `BL` x4, DXF groups 92.
    pub dxf_92: [i32; 4],
}

/// One edge of an evaluation graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalGraphEdge {
    /// `BL`, DXF group 92 (the edge's index).
    pub dxf_92: i32,
    /// `BL`, DXF group 93 (0 in every record).
    pub dxf_93: i32,
    /// `BL`, DXF group 94.
    pub dxf_94: i32,
    /// `BL` x2, DXF groups 91.
    pub dxf_91: [i32; 2],
    /// `BL` x5, DXF groups 92.
    pub dxf_92_links: [i32; 5],
}

/// A decoded ACAD_EVALUATION_GRAPH record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalGraph {
    /// `BL`, DXF group 96.
    pub dxf_96: i32,
    /// `BL`, DXF group 97.
    pub dxf_97: i32,
    pub nodes: Vec<EvalGraphNode>,
    pub edges: Vec<EvalGraphEdge>,
}

const MIN_NODE_BITS: usize = 14;
const MIN_EDGE_BITS: usize = 20;

fn check_count(count: i32, min_bits: usize, left: usize, what: &str) -> Result<usize> {
    let n = usize::try_from(count).map_err(|_| {
        Error::SectionMap(format!(
            "ACAD_EVALUATION_GRAPH {what} count {count} is negative"
        ))
    })?;
    if n > left / min_bits {
        return Err(Error::SectionMap(format!(
            "ACAD_EVALUATION_GRAPH {what} count {n} exceeds the {left} bits left in the record"
        )));
    }
    Ok(n)
}

/// Decode an ACAD_EVALUATION_GRAPH record straight from its raw object payload,
/// from the layout of the module docs (derived from AutoCAD's output,
/// ARCHITECTURE.md §7ab), ending exactly on the data-stream boundary.
pub(crate) fn decode_object(
    payload: &[u8],
    body_start: usize,
    inline_data_end: Option<usize>,
    version: Version,
) -> Result<EvalGraph> {
    open_and_finish(
        "ACAD_EVALUATION_GRAPH",
        payload,
        body_start,
        inline_data_end,
        version,
        |c, _| {
            let dxf_96 = c.read_bl()?;
            let dxf_97 = c.read_bl()?;
            let count = c.read_bl()?;
            let n = check_count(count, MIN_NODE_BITS, c.remaining_bits(), "node")?;
            let mut nodes = Vec::with_capacity(n);
            for _ in 0..n {
                nodes.push(EvalGraphNode {
                    dxf_91: c.read_bl()?,
                    dxf_93: c.read_bl()?,
                    dxf_95: c.read_bl()?,
                    dxf_92: [c.read_bl()?, c.read_bl()?, c.read_bl()?, c.read_bl()?],
                });
            }
            let count = c.read_bl()?;
            let m = check_count(count, MIN_EDGE_BITS, c.remaining_bits(), "edge")?;
            let mut edges = Vec::with_capacity(m);
            for _ in 0..m {
                edges.push(EvalGraphEdge {
                    dxf_92: c.read_bl()?,
                    dxf_93: c.read_bl()?,
                    dxf_94: c.read_bl()?,
                    dxf_91: [c.read_bl()?, c.read_bl()?],
                    dxf_92_links: [
                        c.read_bl()?,
                        c.read_bl()?,
                        c.read_bl()?,
                        c.read_bl()?,
                        c.read_bl()?,
                    ],
                });
            }
            Ok(EvalGraph {
                dxf_96,
                dxf_97,
                nodes,
                edges,
            })
        },
    )
}
