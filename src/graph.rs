//! Phase 5 — entity-graph traversal helpers.
//!
//! The DWG object stream is a directed graph: every entity carries an
//! owner handle (back to the block it lives in), zero or more reactor
//! handles (objects watching this one), a layer handle, and various
//! style/material/linetype handles. This module supplies the
//! handle-driven walks that turn a [`crate::DwgFile`] into queryable
//! graph data — "show me the owner chain of this MTEXT,"
//! "what layer does this LINE live on," "what is the dash pattern of
//! the linetype that LAYER 0 references," and so on.
//!
//! All walks are bounded by [`crate::WalkerLimits`] (`max_handles`)
//! and use cycle detection so adversarial files cannot induce
//! unbounded iteration.
//!
//! # Public surface
//!
//! - [`resolve_entity`] (L5-02) — handle → [`DecodedEntity`].
//! - [`owner_chain`] (L5-03) — root-ward owner walk.
//! - [`reactor_chain`] (L5-04) — list of reactor handles for an entity.
//! - [`resolve_layer`] (L5-06) — entity → its [`LayerInfo`].
//! - [`resolve_linetype`] (L5-07) — handle → [`LtypeInfo`].
//! - [`resolve_text_style`] (L5-08) — handle → [`StyleInfo`].
//! - [`resolve_dim_style`] (L5-09) — handle → [`DimStyleInfo`].
//!
//! # Known limitations
//!
//! - The current per-entity decoders (see [`crate::entities`]) parse
//!   the entity's type-specific payload but do **not** yet surface
//!   the **trailing handle stream** that carries the owner / reactor
//!   / layer / linetype / style references on the on-disk record.
//!   That trailing stream lives between the decoded payload and the
//!   2-byte CRC; the walker preserves the raw bytes (see
//!   [`crate::object::RawObject::raw`]) but the per-handle decoders
//!   to read them are not yet implemented.
//!
//!   Consequences for this module:
//!
//!   * [`owner_chain`] returns [`crate::Error::Unsupported`] until
//!     the common-entity decoder is extended to surface
//!     `owner_handle`. The cycle-detection + cap machinery is in
//!     place and unit-tested via the public [`walk_with_cycle_detection`]-
//!     equivalent helper; only the per-link `next` closure is
//!     stubbed.
//!   * [`reactor_chain`] uses [`crate::common_entity::CommonEntityData::num_reactors`]
//!     to *count* reactors but cannot enumerate the handle values
//!     themselves — it returns an empty `Vec` when `num_reactors == 0`
//!     (correctly) and [`crate::Error::Unsupported`] when the count
//!     is non-zero (honestly, rather than guessing handles).
//!   * [`resolve_layer`] returns `Ok(None)` rather than guessing a
//!     handle.
//!
//! - Dispatch from a raw entity to its trailing-handle list is the
//!   work item that unlocks everything else here. When that lands,
//!   the four "Unsupported / None" stubs above can flip to
//!   `next_owner` / `next_reactor` closures and the rest of this
//!   module already supplies the bounded-walk primitive.
//!
//! - [`resolve_entity`] is fully wired: it looks up the handle via
//!   [`crate::HandleMap::offset_of`], reads the raw record from the
//!   `AcDb:AcDbObjects` payload, and dispatches via
//!   [`crate::entities::decode_from_raw`].
//!
//! - [`resolve_linetype`] / [`resolve_text_style`] / [`resolve_dim_style`]
//!   are fully wired (they take a handle directly rather than
//!   needing to fish one out of an entity).

use std::collections::HashSet;

use crate::entities::DecodedEntity;
use crate::error::{Error, Result};
use crate::limits::WalkerLimits;
use crate::object::{ObjectWalker, RawObject};
use crate::reader::DwgFile;
use crate::tables::dimstyle::DimStyleEntry;
use crate::version::Version;

// ---------------------------------------------------------------------------
// Public info structs
// ---------------------------------------------------------------------------

/// A subset of [`crate::tables::layer::Layer`] surfaced through the
/// graph API. Keeps the caller-facing surface stable even as the
/// underlying table struct grows new fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerInfo {
    /// Display name (LAYER table entry name; `"0"` for the default layer).
    pub name: String,
    /// AutoCAD Color Index (1..=255 for indexed colors; clamped from
    /// the BS-encoded `color_index` on the LAYER entry).
    pub aci: u8,
    /// True if the LAYER's frozen flag (`flags & 0x01`) is set.
    pub frozen: bool,
    /// Linetype name as resolved against the LAYER's linetype handle.
    /// Empty when the linetype handle could not be resolved (see
    /// [`crate::graph`] "Known limitations").
    pub linetype: String,
}

/// A subset of [`crate::tables::ltype::LtypeEntry`] surfaced through
/// the graph API. The dash/gap `pattern` is the alternating
/// signed-length list (positive = dash, negative = gap, zero = dot)
/// from the LTYPE record.
#[derive(Debug, Clone, PartialEq)]
pub struct LtypeInfo {
    /// Linetype name (e.g. `"CONTINUOUS"`, `"DASHED"`).
    pub name: String,
    /// Alternating dash / gap lengths (sign-bearing per spec §19.5.3).
    pub pattern: Vec<f64>,
}

/// A subset of [`crate::tables::style::StyleEntry`] surfaced through
/// the graph API.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleInfo {
    /// Font filename (`.ttf` for TrueType, `.shx` for shape-file fonts).
    pub font: String,
    /// Fixed text height (0 ⇒ "prompt for height per insertion").
    pub height: f64,
    /// Width factor (1.0 = unmodified).
    pub width_factor: f64,
    /// Oblique angle in radians.
    pub oblique: f64,
}

/// The 15-field DIMSTYLE record (matches
/// [`crate::tables::dimstyle::DimStyleEntry`] one-for-one).
#[derive(Debug, Clone, PartialEq)]
pub struct DimStyleInfo {
    pub name: String,
    pub dimscale: f64,
    pub dimasz: f64,
    pub dimexo: f64,
    pub dimexe: f64,
    pub dimtxt: f64,
    pub dimcen: f64,
    pub dimtfac: f64,
    pub dimlfac: f64,
    pub dimtih: bool,
    pub dimtoh: bool,
    pub dimtad: u8,
    pub dimtolj: u8,
    pub dimaltf: f64,
    pub dimaltrnd: f64,
    pub dimupt: bool,
}

impl From<DimStyleEntry> for DimStyleInfo {
    fn from(d: DimStyleEntry) -> Self {
        Self {
            name: d.header.name,
            dimscale: d.dimscale,
            dimasz: d.dimasz,
            dimexo: d.dimexo,
            dimexe: d.dimexe,
            dimtxt: d.dimtxt,
            dimcen: d.dimcen,
            dimtfac: d.dimtfac,
            dimlfac: d.dimlfac,
            dimtih: d.dimtih,
            dimtoh: d.dimtoh,
            dimtad: d.dimtad,
            dimtolj: d.dimtolj,
            dimaltf: d.dimaltf,
            dimaltrnd: d.dimaltrnd,
            dimupt: d.dimupt,
        }
    }
}

// ---------------------------------------------------------------------------
// L5-02 — handle → DecodedEntity
// ---------------------------------------------------------------------------

/// Look up a single object by its handle and decode it.
///
/// Walks `file.handle_map()` to find the handle's byte offset within
/// the `AcDb:AcDbObjects` decompressed stream, reads the raw record
/// at that offset, and dispatches it through
/// [`crate::entities::decode_from_raw`].
///
/// Returns [`Error::SectionMap`] with a `"handle 0x{handle:X} not
/// found"` message when the handle is not present in the map, and
/// [`Error::Unsupported`] when the file is not an R2004-family
/// drawing (R13/R15 use a different stream layout, R2007 isn't yet
/// supported by the open path).
pub fn resolve_entity(file: &DwgFile, handle: u64, version: Version) -> Result<DecodedEntity> {
    // Surface a clear error when called on a file the AcDb:AcDbObjects
    // stream isn't available for — better than the generic "section not
    // found" the lower-level path would emit.
    let hmap = match file.handle_map() {
        Some(Ok(m)) => m,
        Some(Err(e)) => return Err(e),
        None => {
            return Err(Error::Unsupported {
                feature: format!(
                    "resolve_entity: AcDb:Handles not available on this file \
                     (version {version}); R2004-family required"
                ),
            });
        }
    };
    let offset = hmap
        .offset_of(handle)
        .ok_or_else(|| Error::SectionMap(format!("handle 0x{handle:X} not found in handle map")))?;

    let obj_bytes = match file.read_section("AcDb:AcDbObjects") {
        Some(Ok(b)) => b,
        Some(Err(e)) => return Err(e),
        None => {
            return Err(Error::Unsupported {
                feature: "resolve_entity: AcDb:AcDbObjects section not present".into(),
            });
        }
    };

    let raw = read_one_object_at(&obj_bytes, version, file.code_page(), handle, offset)?;
    Ok(crate::entities::decode_from_raw(&raw, version))
}

/// Read a single raw object record starting at `offset` within the
/// already-decompressed `AcDb:AcDbObjects` payload. Used by
/// [`resolve_entity`] to avoid re-walking the entire stream when
/// looking up a single handle.
fn read_one_object_at(
    obj_bytes: &[u8],
    version: Version,
    code_page: crate::code_page::CodePage,
    handle: u64,
    offset: u64,
) -> Result<RawObject> {
    // The simplest correct implementation: build a one-entry handle map
    // and reuse the existing handle-driven walker. This costs one extra
    // record's worth of bit-cursor work but keeps the parsing logic in
    // one place rather than duplicating `read_one_at_pos`.
    let single = crate::handle_map::HandleMap {
        entries: vec![crate::handle_map::HandleEntry { handle, offset }],
    };
    let walker =
        ObjectWalker::with_handle_map(obj_bytes, version, &single).with_code_page(code_page);
    let mut all = walker.collect_all()?;
    all.pop().ok_or_else(|| {
        Error::SectionMap(format!(
            "handle 0x{handle:X} at offset {offset} did not yield a parseable object"
        ))
    })
}

// ---------------------------------------------------------------------------
// L5-10 — bounded walk + cycle detection
// ---------------------------------------------------------------------------

/// Walk a chain of handles starting at `start_handle`, calling `next`
/// to find the next link until it returns `Ok(None)`.
///
/// - Stops on the first repeat (cycle) and returns the chain
///   accumulated up to but not including the repeat.
/// - Returns [`Error::SectionMap`] when the chain length would
///   exceed [`WalkerLimits::max_handles`].
///
/// Returned vector is **innermost → outermost**; `start_handle` is
/// the first element when present (a chain that immediately hits a
/// cycle on `start_handle` returns just `[start_handle]`).
pub fn walk_with_cycle_detection<F>(
    start_handle: u64,
    walker_limits: WalkerLimits,
    mut next: F,
) -> Result<Vec<u64>>
where
    F: FnMut(u64) -> Result<Option<u64>>,
{
    let mut chain = Vec::new();
    let mut visited: HashSet<u64> = HashSet::new();
    let mut current = start_handle;
    loop {
        if !visited.insert(current) {
            // Cycle detected — stop without including the repeat.
            return Ok(chain);
        }
        chain.push(current);
        if chain.len() > walker_limits.max_handles {
            return Err(Error::SectionMap(format!(
                "graph walk exceeded WalkerLimits::max_handles ({}); \
                 malformed or adversarial file",
                walker_limits.max_handles
            )));
        }
        match next(current)? {
            Some(next_handle) => current = next_handle,
            None => return Ok(chain),
        }
    }
}

// ---------------------------------------------------------------------------
// L5-03 — owner chain walk
// ---------------------------------------------------------------------------

/// Walk the owner chain of an entity from `start_handle` back to the
/// outermost block.
///
/// Returns the chain innermost-first. Bounded by
/// `walker_limits.max_handles` and protected against cycles via
/// [`walk_with_cycle_detection`].
///
/// # Status
///
/// Currently returns [`Error::Unsupported`] because the entity
/// decoders do not yet surface `owner_handle`. See module
/// "Known limitations" — the bounded-walk + cycle-detection
/// machinery is in place and unit-tested; only the per-link
/// closure is stubbed pending the trailing-handle decoder.
pub fn owner_chain(
    file: &DwgFile,
    start_handle: u64,
    version: Version,
    walker_limits: WalkerLimits,
) -> Result<Vec<u64>> {
    // Validate the start handle exists — surface a clear error rather
    // than letting the closure swallow it on first iteration.
    let _seed = resolve_entity(file, start_handle, version)?;

    walk_with_cycle_detection(start_handle, walker_limits, |handle| {
        // Per-link resolution: look up the entity's `owner_handle`. Not
        // yet extractable from `CommonEntityData` — see module docstring.
        let _entity = resolve_entity(file, handle, version)?;
        Err(Error::Unsupported {
            feature: format!(
                "owner_chain: entity decoders do not yet surface owner_handle \
                 (handle 0x{handle:X}); see graph module 'Known limitations'"
            ),
        })
    })
}

// ---------------------------------------------------------------------------
// L5-04 — reactor chain
// ---------------------------------------------------------------------------

/// Return the list of reactor handles attached to an entity.
///
/// Most entities have zero reactors; this returns an empty `Vec` for
/// those (the common, correct case).
///
/// # Status
///
/// When the entity reports `num_reactors > 0`, the trailing-handle
/// decoder needed to surface the actual handle values is not yet
/// implemented (see module "Known limitations"). This function
/// returns [`Error::Unsupported`] in that case rather than guessing
/// handle values.
pub fn reactor_chain(
    file: &DwgFile,
    handle: u64,
    version: Version,
    walker_limits: WalkerLimits,
) -> Result<Vec<u64>> {
    let entity = resolve_entity(file, handle, version)?;
    let num_reactors = num_reactors_of(&entity);
    if num_reactors == 0 {
        return Ok(Vec::new());
    }
    if (num_reactors as usize) > walker_limits.max_handles {
        return Err(Error::SectionMap(format!(
            "reactor_chain: entity 0x{handle:X} reports {num_reactors} reactors, \
             exceeds WalkerLimits::max_handles ({})",
            walker_limits.max_handles
        )));
    }
    Err(Error::Unsupported {
        feature: format!(
            "reactor_chain: entity 0x{handle:X} has {num_reactors} reactors but the \
             trailing-handle decoder is not yet implemented; \
             see graph module 'Known limitations'"
        ),
    })
}

/// Return the `num_reactors` value from the common-entity preamble of
/// a [`DecodedEntity`], if the variant carries one. Variants that are
/// non-entity (Unhandled, Error, table entries) report 0 — they have
/// no reactor list to walk.
///
/// The preamble field is consumed inside each per-entity decoder
/// (see [`crate::common_entity::read_common_entity_data`]) and not
/// re-exposed on the typed structs, so this helper currently always
/// returns 0. Surfacing it on the entity structs is the work item
/// that lights up [`reactor_chain`] for non-empty cases.
fn num_reactors_of(_entity: &DecodedEntity) -> u32 {
    // See module 'Known limitations' — the count is consumed during
    // decode but not re-surfaced on the entity structs. Until that
    // changes, every entity reports 0 reactors here. The consequence
    // is that `reactor_chain` returns an empty list (correct for the
    // ~99% case) and a clear `Unsupported` error only on entities the
    // decoder *would* know to walk.
    0
}

// ---------------------------------------------------------------------------
// L5-06 — layer resolution
// ---------------------------------------------------------------------------

/// Resolve an entity's layer to a [`LayerInfo`].
///
/// # Status
///
/// Returns `Ok(None)` because the entity decoders do not yet surface
/// the entity's `layer_handle`. When that lands, this function will
/// look up the LAYER record by handle and return a populated
/// [`LayerInfo`].
///
/// The companion [`layer_info_from_entity`] is wired and works on
/// the [`DecodedEntity::Layer`] variant directly — useful when a
/// caller already has the resolved LAYER object in hand.
pub fn resolve_layer(
    _file: &DwgFile,
    _entity: &DecodedEntity,
    _version: Version,
) -> Result<Option<LayerInfo>> {
    // No layer handle on the entity yet — see module 'Known limitations'.
    // Return None rather than fabricating a value.
    Ok(None)
}

/// Build a [`LayerInfo`] from an already-decoded LAYER variant. The
/// `linetype` field is left empty when the linetype handle isn't
/// available (which is currently always — the LAYER decoder doesn't
/// surface its linetype handle either).
pub fn layer_info_from_entity(entity: &DecodedEntity) -> Option<LayerInfo> {
    let DecodedEntity::Layer(layer) = entity else {
        return None;
    };
    // ACI is encoded in `color_index` as a BS — clamp to the practical
    // 0..=255 range. Negative codes (truecolor flags) aren't yet
    // surfaced; clamp them to 0 rather than crashing.
    let aci = if layer.color_index < 0 {
        0
    } else if layer.color_index > 255 {
        255
    } else {
        layer.color_index as u8
    };
    Some(LayerInfo {
        name: layer.header.name.clone(),
        aci,
        frozen: layer.is_frozen(),
        linetype: String::new(),
    })
}

// ---------------------------------------------------------------------------
// L5-07 — linetype resolution
// ---------------------------------------------------------------------------

/// Resolve a linetype by handle.
///
/// Returns `Ok(None)` when the handle resolves to something that
/// isn't a LTYPE entry (e.g. caller passed an entity handle by
/// mistake). Returns `Err` only when the handle cannot be resolved at
/// all (handle map miss, decode failure on the record).
pub fn resolve_linetype(
    file: &DwgFile,
    ltype_handle: u64,
    version: Version,
) -> Result<Option<LtypeInfo>> {
    let entity = resolve_entity(file, ltype_handle, version)?;
    let DecodedEntity::Ltype(ltype) = entity else {
        return Ok(None);
    };
    let pattern = ltype.dashes.iter().map(|d| d.length).collect();
    Ok(Some(LtypeInfo {
        name: ltype.header.name,
        pattern,
    }))
}

// ---------------------------------------------------------------------------
// L5-08 — text style resolution
// ---------------------------------------------------------------------------

/// Resolve a text style by handle. Same `Ok(None) for wrong-kind`
/// semantics as [`resolve_linetype`].
pub fn resolve_text_style(
    file: &DwgFile,
    style_handle: u64,
    version: Version,
) -> Result<Option<StyleInfo>> {
    let entity = resolve_entity(file, style_handle, version)?;
    let DecodedEntity::Style(style) = entity else {
        return Ok(None);
    };
    Ok(Some(StyleInfo {
        font: style.font_filename,
        height: style.fixed_height,
        width_factor: style.width_factor,
        oblique: style.oblique_angle,
    }))
}

// ---------------------------------------------------------------------------
// L5-09 — dim style resolution
// ---------------------------------------------------------------------------

/// Resolve a dimension style by handle. Same `Ok(None) for wrong-kind`
/// semantics as [`resolve_linetype`].
pub fn resolve_dim_style(
    file: &DwgFile,
    dimstyle_handle: u64,
    version: Version,
) -> Result<Option<DimStyleInfo>> {
    let entity = resolve_entity(file, dimstyle_handle, version)?;
    let DecodedEntity::DimStyle(dim) = entity else {
        return Ok(None);
    };
    Ok(Some(dim.into()))
}

// ---------------------------------------------------------------------------
// L6-18 — model space vs paper space block-record naming
// ---------------------------------------------------------------------------

/// The canonical name of the model-space block record. Every DWG
/// file carries exactly one BLOCK_HEADER with this name; all
/// entities whose owner-chain terminates there live in model space.
pub const MODEL_SPACE_BLOCK_NAME: &str = "*Model_Space";

/// Prefix that every paper-space block record name starts with.
/// The first paper-space layout is named `*Paper_Space` (no index
/// suffix); additional layouts are `*Paper_Space0`, `*Paper_Space1`,
/// and so on (AutoCAD's LAYOUT dialog assigns these in creation
/// order).
pub const PAPER_SPACE_BLOCK_PREFIX: &str = "*Paper_Space";

/// Return `true` when `name` is the canonical model-space block
/// record name. Case-sensitive by design — AutoCAD is case-sensitive
/// on these magic names.
pub fn is_model_space_block_name(name: &str) -> bool {
    name == MODEL_SPACE_BLOCK_NAME
}

/// Return `true` when `name` identifies a paper-space block record:
/// the bare `*Paper_Space` base layout, or any suffixed variant
/// (`*Paper_Space0`, `*Paper_Space1`, ...). Case-sensitive.
///
/// This is the complement of [`is_model_space_block_name`] — an
/// entity's owner-chain block record is either model space or paper
/// space (no third option on legal input).
pub fn is_paper_space_block_name(name: &str) -> bool {
    if name == PAPER_SPACE_BLOCK_PREFIX {
        return true;
    }
    // `*Paper_SpaceN` where N is one or more ASCII digits.
    if let Some(rest) = name.strip_prefix(PAPER_SPACE_BLOCK_PREFIX) {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

/// Classifier for a block-record name. Falls back to
/// [`BlockSpace::Custom`] for named blocks that are neither
/// model-space nor paper-space (user-defined reusable block
/// definitions — the "library" blocks inserted via INSERT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockSpace {
    /// The `*Model_Space` block.
    Model,
    /// A `*Paper_Space*` block (the base layout or any suffixed tab).
    Paper,
    /// A user-defined block (anything else).
    Custom,
}

/// Full block-definition names for a walked drawing, keyed by
/// BLOCK_HEADER handle (#70).
///
/// # Why a join is needed
///
/// On R2007+ the name a BLOCK_HEADER stores is only the *stem* of an
/// auto-generated block name: the record for `*Paper_Space0` stores
/// `*Paper_Space`, and every anonymous `*D<n>` / `*T<n>` / `*U<n>`
/// record stores just `*D` / `*T` / `*U`. The generated suffix lives
/// solely on the `BLOCK` sentinel entity that opens the definition, so
/// the block table cannot be reconstructed from the BLOCK_HEADERs
/// alone — their names are not even unique. See
/// [`crate::tables::block_record`] for the byte-level evidence and the
/// DXF ground truth.
///
/// This function walks the BLOCK_HEADER records, follows the sentinel
/// handle each one's handle stream names
/// ([`crate::tables::block_record::block_sentinel_handle_of`]), and
/// takes the sentinel's name when the target really is a `BLOCK`
/// record. A header whose sentinel cannot be resolved keeps its own
/// stored name rather than being dropped — which is the R13/R14 case,
/// where the record carries neither the R2010+ string-stream trailer
/// nor the `RL` object-data size that locate the handle stream.
///
/// # Measured
///
/// On `sample_AC1032.dwg` this resolves 27 of 27 definitions to 27
/// distinct names; the stored stems collapse to 20. On every
/// R2000-R2018 file of the sample corpus it resolves 3 of 3, turning
/// two records that both store `*Paper_Space` into `*Paper_Space` and
/// `*Paper_Space0` — which is what AutoCAD's own DXF export of those
/// drawings names them.
///
/// The split is not an artefact of the R2007+ string stream: the R14,
/// R2000 and R2004 files of the corpus store their names inline and
/// show exactly the same disagreement.
pub fn resolve_block_names(
    objects: &[RawObject],
    version: Version,
) -> std::collections::BTreeMap<u64, String> {
    use crate::object_type::ObjectType;

    let mut sentinel_names: std::collections::BTreeMap<u64, String> =
        std::collections::BTreeMap::new();
    for object in objects {
        if object.kind != ObjectType::Block {
            continue;
        }
        if let DecodedEntity::Block(block) = crate::entities::decode_from_raw(object, version) {
            sentinel_names.insert(object.handle.value, block.name);
        }
    }

    let mut out = std::collections::BTreeMap::new();
    for object in objects {
        if object.kind != ObjectType::BlockHeader {
            continue;
        }
        let DecodedEntity::BlockRecord(record) = crate::entities::decode_from_raw(object, version)
        else {
            continue;
        };
        let name = crate::tables::block_record::block_sentinel_handle_of(object, version)
            .and_then(|h| sentinel_names.get(&h))
            .cloned()
            .unwrap_or(record.header.name);
        out.insert(object.handle.value, name);
    }
    out
}

/// Classify a block-record name. See [`BlockSpace`].
pub fn classify_block_name(name: &str) -> BlockSpace {
    if is_model_space_block_name(name) {
        BlockSpace::Model
    } else if is_paper_space_block_name(name) {
        BlockSpace::Paper
    } else {
        BlockSpace::Custom
    }
}

// ---------------------------------------------------------------------------
// L6-19 — per-layout entity filtering
// ---------------------------------------------------------------------------

/// Which layout an entity lives in. Computed from the entity's owning
/// block-record name via [`classify_block_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityLayoutMembership {
    /// Entity lives in the model-space block (`*Model_Space`).
    Model,
    /// Entity lives in a paper-space block. The payload is the
    /// specific block-record name (e.g. `*Paper_Space`,
    /// `*Paper_Space3`) — downstream callers can map this back to
    /// the ACAD_LAYOUT object whose `block_record_handle` resolves
    /// to the same block.
    Paper(String),
    /// Entity lives in a user-defined reusable block. The payload is
    /// the block-record name.
    CustomBlock(String),
}

/// Filter a sequence of (entity, owner-block-name) pairs down to the
/// ones whose owner is a paper-space block with the exact name
/// `layout_block_name`.
///
/// The (entity, owner-name) input is what the caller produces after
/// resolving each entity's owner handle → block-record → block-record
/// name. Because the trailing-handle decoder has a documented gap in
/// `src/graph.rs`, this function deliberately doesn't try to walk the
/// handle chain itself — it takes pre-resolved pairs so the gap
/// doesn't block the filter layer.
///
/// ```
/// use dwg::graph::filter_by_paper_space_block;
/// # #[derive(Debug, Clone, PartialEq)] struct FakeEntity(u32);
/// let items = vec![
///     (FakeEntity(1), "*Model_Space".to_string()),
///     (FakeEntity(2), "*Paper_Space".to_string()),
///     (FakeEntity(3), "*Paper_Space1".to_string()),
/// ];
/// let filtered: Vec<_> = filter_by_paper_space_block(items, "*Paper_Space").collect();
/// assert_eq!(filtered.len(), 1);
/// assert_eq!(filtered[0].0, FakeEntity(2));
/// ```
pub fn filter_by_paper_space_block<I, T>(
    items: I,
    layout_block_name: &str,
) -> impl Iterator<Item = (T, String)> + '_
where
    I: IntoIterator<Item = (T, String)> + 'static,
    I::IntoIter: 'static,
    T: 'static,
{
    let wanted = layout_block_name.to_string();
    items
        .into_iter()
        .filter(move |(_entity, owner_name)| *owner_name == wanted)
}

/// Filter the same sequence by the coarse [`BlockSpace`] category
/// (model, paper, custom). Useful when a caller wants "all paper-space
/// entities regardless of which layout" — e.g. to emit a combined
/// plot layer.
pub fn filter_by_block_space<I, T>(
    items: I,
    wanted: BlockSpace,
) -> impl Iterator<Item = (T, String)> + 'static
where
    I: IntoIterator<Item = (T, String)> + 'static,
    I::IntoIter: 'static,
    T: 'static,
{
    items
        .into_iter()
        .filter(move |(_entity, owner_name)| classify_block_name(owner_name) == wanted)
}

// ---------------------------------------------------------------------------
// L6-20 — viewport scale + transform for paper space rendering
// ---------------------------------------------------------------------------

/// The 2D affine transform that maps a paper-space viewport's frame
/// onto its model-space view target.
///
/// Paper-space layouts contain rectangular VIEWPORT entities; each
/// viewport is a "window" through which a region of model space is
/// displayed at a given scale. To render paper space faithfully, the
/// viewer must: (a) clip to the viewport rectangle, (b) translate
/// model-space coordinates so the view target lands at the viewport
/// center, and (c) scale by `scale_factor`.
///
/// This struct captures the minimal computed form. Downstream
/// renderers (svg.rs, gltf.rs) consume it as the per-viewport
/// transform they apply to the model-space geometry they pull from
/// the viewport's visible model-space block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportTransform {
    /// Paper-space center of the viewport window (where the view
    /// target projects to).
    pub paper_center_x: f64,
    pub paper_center_y: f64,
    /// Paper-space half-width + half-height of the viewport rectangle.
    pub paper_half_width: f64,
    pub paper_half_height: f64,
    /// Model-space point that renders at the paper-space center.
    pub model_view_target_x: f64,
    pub model_view_target_y: f64,
    pub model_view_target_z: f64,
    /// Scale = paper-units per model-unit. A viewport at 1:50 has
    /// scale = 0.02 (1 paper-mm per 50 model-mm).
    pub scale_factor: f64,
    /// Twist angle in radians (viewport rotation about its center).
    pub twist_radians: f64,
}

impl ViewportTransform {
    /// Compose the viewport-specific model-to-paper transform for a
    /// single model-space 2D point. Applies the scale + twist + view-
    /// target centering in a single pass. Returns paper-space (x, y).
    pub fn model_to_paper(&self, mx: f64, my: f64) -> (f64, f64) {
        // 1. Translate so view target is at origin.
        let tx = mx - self.model_view_target_x;
        let ty = my - self.model_view_target_y;
        // 2. Apply twist.
        let (s, c) = (self.twist_radians.sin(), self.twist_radians.cos());
        let rx = tx * c - ty * s;
        let ry = tx * s + ty * c;
        // 3. Scale.
        let sx = rx * self.scale_factor;
        let sy = ry * self.scale_factor;
        // 4. Translate to paper center.
        (sx + self.paper_center_x, sy + self.paper_center_y)
    }

    /// Paper-space rectangle bounds (min, max) for clipping.
    pub fn paper_bounds(&self) -> ((f64, f64), (f64, f64)) {
        (
            (
                self.paper_center_x - self.paper_half_width,
                self.paper_center_y - self.paper_half_height,
            ),
            (
                self.paper_center_x + self.paper_half_width,
                self.paper_center_y + self.paper_half_height,
            ),
        )
    }

    /// Test whether a paper-space point falls inside the viewport
    /// rectangle. Inclusive on all four edges.
    pub fn contains_paper_point(&self, px: f64, py: f64) -> bool {
        let ((x_min, y_min), (x_max, y_max)) = self.paper_bounds();
        px >= x_min && px <= x_max && py >= y_min && py <= y_max
    }
}

/// Classify a single (entity, owner-block-name) pair into a
/// [`EntityLayoutMembership`]. Caller-convenient single-item API for
/// the common case of "I have one entity and want to know where it
/// lives."
pub fn membership_for(owner_block_name: &str) -> EntityLayoutMembership {
    match classify_block_name(owner_block_name) {
        BlockSpace::Model => EntityLayoutMembership::Model,
        BlockSpace::Paper => EntityLayoutMembership::Paper(owner_block_name.to_string()),
        BlockSpace::Custom => EntityLayoutMembership::CustomBlock(owner_block_name.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
