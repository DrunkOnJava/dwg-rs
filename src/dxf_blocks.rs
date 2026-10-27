//! The DXF `BLOCKS` section from the decoded block records.
//!
//! One block per `BLOCK_RECORD` of the table, in its order: the `BLOCK`, the
//! entities of the block (built by [`crate::dxf_entities::build`], which writes
//! model space and the active paper space to `ENTITIES` instead, so those two
//! blocks are empty here as in AutoCAD's own export) and the `ENDBLK`. An entity
//! of a kind nothing writes yet is counted in a `skipped` comment.

use crate::dxf_entities::{Entities, write_record};

/// Write `entities`' blocks as a `BLOCKS` section.
pub fn write(w: &mut crate::dxf::DxfWriter, entities: &Entities) {
    w.begin_section("BLOCKS");
    for (kind, n) in &entities.block_skipped {
        w.write_comment(&format!("skipped: {kind} x{n}"));
    }
    for b in &entities.blocks {
        write_record(w, &b.begin);
        if b.block != entities.model_space && b.block != entities.paper_space {
            for r in entities.records.iter().filter(|r| r.block == b.block) {
                write_record(w, r);
            }
        }
        write_record(w, &b.end);
    }
    w.end_section();
}
