//! The DXF `OBJECTS` section from the decoded drawing objects.
//!
//! [`plan`] decodes every non-entity, non-table object once and names the ones
//! the writer supports and can read the handle stream of; [`build`] writes each
//! as the record AutoCAD's own DXF export writes for it, in the order the export
//! uses: a breadth-first walk that starts from the named object dictionary and,
//! after it, every object owned by something that is not an object (a table, a
//! table record, an entity: their extension dictionaries) in the order of the
//! owners in TABLES, BLOCKS and ENTITIES, and that visits a dictionary's entries
//! and then an object's extension dictionary. An object the walk does not reach
//! follows, in file order.
//!
//! Written: DICTIONARY and ACDBDICTIONARYWDFLT (the key and handle of each
//! entry), DICTIONARYVAR, XRECORD, SCALE and ACDBPLACEHOLDER. A dictionary entry
//! whose object is not written is left out, and [`Objects::skipped`] counts the
//! objects not written by kind.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::dxf::DxfVersion;
use crate::dxf_entities::{EntityRecord, write_record};
use crate::dxf_sections::HeaderGroup;
use crate::dxf_tables::Table;
use crate::entities::DecodedEntity;
use crate::entity_refs::entity_refs;
use crate::header_vars::HeaderVars;
use crate::object::RawObject;
use crate::object_type::ObjectType;
use crate::objects::xrecord::XValue;
use crate::reader::DwgFile;
use crate::tables::handles::{EntryRefs, refs};
use crate::version::Version;

/// The DXF kinds the writer writes of a drawing when the output targets `version`,
/// whatever the drawing holds.
pub fn kinds(_version: DxfVersion) -> Vec<&'static str> {
    vec![
        "DICTIONARY",
        "ACDBDICTIONARYWDFLT",
        "DICTIONARYVAR",
        "XRECORD",
        "SCALE",
        "ACDBPLACEHOLDER",
    ]
}

/// The objects of a drawing as the converter writes them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Objects {
    /// The records, in the order of AutoCAD's export.
    pub records: Vec<EntityRecord>,
    /// Objects left out, by kind.
    pub skipped: BTreeMap<String, usize>,
}

impl Objects {
    /// The handles of every record written.
    pub fn handles(&self) -> HashSet<u64> {
        self.records.iter().map(|r| r.handle).collect()
    }
}

/// One supported object: what it decoded to and its handle references.
#[derive(Debug)]
struct Item {
    handle: u64,
    version: Version,
    decoded: DecodedEntity,
    class: Option<String>,
    refs: EntryRefs,
}

/// One object of the drawing as the order of the output sees it.
#[derive(Debug)]
struct Node {
    handle: u64,
    owner: u64,
    xdictionary: Option<u64>,
    entries: Vec<u64>,
}

/// The objects the writer supports, and the ones it leaves out by kind.
#[derive(Debug)]
pub struct Plan {
    items: Vec<Item>,
    skipped: BTreeMap<String, usize>,
    nodes: Vec<Node>,
    /// Each entity's block record and its place in the object stream.
    entities: HashMap<u64, (u64, usize)>,
    model_space: u64,
    paper_space: u64,
}

impl Plan {
    /// The handles of the objects that will be written.
    pub fn handles(&self) -> HashSet<u64> {
        self.items.iter().map(|i| i.handle).collect()
    }

    /// Close ownership over what the output holds: an object is kept only when its
    /// owner is null, one of `emitted` (the table records, entities and block
    /// markers written) or an object that is kept, so no record names an owner the
    /// output lacks and nothing is left in an orphaned ownership tree (the
    /// extension dictionary of an entity the writer omits, and everything it owns).
    /// Returns the handles of the objects dropped, counted in the skipped objects.
    pub fn close(&mut self, emitted: &HashSet<u64>) -> HashSet<u64> {
        let mut kept: HashSet<u64> = HashSet::new();
        loop {
            let before = kept.len();
            for i in &self.items {
                let owner = i.refs.owner;
                if owner == 0 || emitted.contains(&owner) || kept.contains(&owner) {
                    kept.insert(i.handle);
                }
            }
            if kept.len() == before {
                break;
            }
        }
        let mut gone = HashSet::new();
        let mut labels: Vec<String> = Vec::new();
        self.items.retain(|i| {
            if kept.contains(&i.handle) {
                return true;
            }
            gone.insert(i.handle);
            labels.push(item_label(i));
            false
        });
        for l in labels {
            *self
                .skipped
                .entry(format!("{l} (its owner is not written)"))
                .or_default() += 1;
        }
        gone
    }
}

fn item_label(item: &Item) -> String {
    match &item.decoded {
        DecodedEntity::Dictionary(_) => "DICTIONARY",
        DecodedEntity::DictionaryVar(_) => "DICTIONARYVAR",
        DecodedEntity::XRecord(_) => "XRECORD",
        DecodedEntity::Scale(_) => "SCALE",
        DecodedEntity::Layout(_) => "LAYOUT",
        DecodedEntity::Group(_) => "GROUP",
        DecodedEntity::MLineStyle(_) => "MLINESTYLE",
        _ => "ACDBPLACEHOLDER",
    }
    .to_string()
}

fn class_label(raw: &RawObject, class: &Option<String>) -> String {
    match class {
        Some(c) => c.clone(),
        None => raw.kind.short_label().to_string(),
    }
}

/// Decode the objects of `file` the writer supports.
pub fn plan(file: &DwgFile, header: &HeaderVars) -> Plan {
    let handle = |name: &str| header.get_handle(name).map_or(0, |h| h.value);
    let mut out = Plan {
        items: Vec::new(),
        skipped: BTreeMap::new(),
        nodes: Vec::new(),
        entities: HashMap::new(),
        model_space: handle("BLOCK_RECORD_MODEL_SPACE"),
        paper_space: handle("BLOCK_RECORD_PAPER_SPACE"),
    };
    let Some(Ok(raws)) = file.all_objects() else {
        return out;
    };
    let version = file.version();
    if matches!(version, Version::R14) {
        return out;
    }
    let class_map = file.class_map().and_then(Result::ok);
    let seed = header.get_handle("$HANDSEED").map_or(u64::MAX, |h| h.value);
    // A class-numbered record is an entity when its class says so (item class
    // 0x1F2, an ACAD_TABLE or an IMAGE, against 0x1F3 for an object).
    let entity_class = |raw: &RawObject| match (class_map.as_ref(), raw.kind) {
        (Some(cm), ObjectType::Custom(code)) => cm
            .by_type_code(code)
            .is_some_and(|c| c.item_class_id == 0x1F2),
        _ => false,
    };
    for (index, raw) in raws
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_entity() || entity_class(r))
    {
        if let Ok((common, r)) = entity_refs(raw, version) {
            let block = match (r.owner, common.raw_mode) {
                (Some(o), _) => o,
                (None, 1) => out.paper_space,
                _ => out.model_space,
            };
            out.entities.insert(raw.handle.value, (block, index));
        }
    }
    for raw in raws.iter().filter(|r| {
        !r.is_entity()
            && !entity_class(r)
            && !r.kind.is_table_entry()
            && !r.kind.is_control()
            && r.handle.value < seed
    }) {
        let (decoded, class) = match (class_map.as_ref(), raw.kind) {
            (Some(cm), ObjectType::Custom(code)) => (
                crate::entities::decode_from_raw_with_class_map(raw, version, cm, code),
                cm.by_type_code(code).map(|c| c.dxf_class_name.clone()),
            ),
            _ => (crate::entities::decode_from_raw(raw, version), None),
        };
        let supported = matches!(
            decoded,
            DecodedEntity::Dictionary(_)
                | DecodedEntity::DictionaryVar(_)
                | DecodedEntity::XRecord(_)
                | DecodedEntity::Placeholder(_)
                | DecodedEntity::Scale(_)
        );
        let reads = refs(raw, version).ok();
        if let Some(r) = &reads {
            out.nodes.push(Node {
                handle: raw.handle.value,
                owner: r.owner,
                xdictionary: r.xdictionary,
                entries: match &decoded {
                    DecodedEntity::Dictionary(d) => {
                        r.rest.iter().take(d.keys.len()).copied().collect()
                    }
                    _ => Vec::new(),
                },
            });
        }
        let reads = reads.filter(|_| supported);
        // An object is planned only when its record can be written, so the handles
        // the tables, the entities and the dictionaries are given are all emitted.
        let item = reads.map(|r| Item {
            handle: raw.handle.value,
            version,
            decoded,
            class: class.clone(),
            refs: r,
        });
        match item {
            Some(item) if record(&item, &HashSet::new()).is_some() => out.items.push(item),
            Some(_) => {
                *out.skipped
                    .entry(format!("{} (not written)", class_label(raw, &class)))
                    .or_default() += 1
            }
            None => *out.skipped.entry(class_label(raw, &class)).or_default() += 1,
        }
    }
    out
}

fn is_default_dictionary(class: &Option<String>) -> bool {
    class
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("ACDBDICTIONARYWDFLT"))
}

fn p3(g: &mut Vec<HeaderGroup>, code: u16, p: [f64; 3]) {
    let code = i32::from(code);
    g.push(HeaderGroup::Real(code, p[0]));
    g.push(HeaderGroup::Real(code + 10, p[1]));
    g.push(HeaderGroup::Real(code + 20, p[2]));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// The record of `item`, or `None` when a group does not write.
fn record(item: &Item, defined: &HashSet<u64>) -> Option<EntityRecord> {
    let mut g: Vec<HeaderGroup> = Vec::new();
    let kind: &'static str = match &item.decoded {
        DecodedEntity::Dictionary(d) => {
            let default = is_default_dictionary(&item.class);
            g.push(HeaderGroup::Text(100, "AcDbDictionary".into()));
            if d.hard_owner {
                g.push(HeaderGroup::Int(280, 1));
            }
            g.push(HeaderGroup::Int(281, i64::from(d.cloning_flag)));
            if item.refs.rest.len() < d.keys.len() + usize::from(default) {
                return None;
            }
            let code = if d.hard_owner { 360 } else { 350 };
            for (key, target) in d.keys.iter().zip(&item.refs.rest) {
                if defined.contains(target) {
                    g.push(HeaderGroup::Text(3, key.clone()));
                    g.push(HeaderGroup::Handle(code, *target));
                }
            }
            if default {
                g.push(HeaderGroup::Text(100, "AcDbDictionaryWithDefault".into()));
                g.push(HeaderGroup::Handle(340, item.refs.rest[d.keys.len()]));
                "ACDBDICTIONARYWDFLT"
            } else {
                "DICTIONARY"
            }
        }
        DecodedEntity::DictionaryVar(v) => {
            g.push(HeaderGroup::Text(100, "DictionaryVariables".into()));
            g.push(HeaderGroup::Int(280, i64::from(v.schema)));
            g.push(HeaderGroup::Text(1, v.value.clone()));
            "DICTIONARYVAR"
        }
        DecodedEntity::Scale(s) => {
            if !s.paper_units.is_finite() || !s.drawing_units.is_finite() {
                return None;
            }
            g.push(HeaderGroup::Text(100, "AcDbScale".into()));
            g.push(HeaderGroup::Int(70, i64::from(s.version)));
            g.push(HeaderGroup::Text(300, s.scale_name.clone()));
            g.push(HeaderGroup::Real(140, s.paper_units));
            g.push(HeaderGroup::Real(141, s.drawing_units));
            g.push(HeaderGroup::Int(290, i64::from(s.is_unit_scale)));
            "SCALE"
        }
        DecodedEntity::XRecord(x) => {
            g.push(HeaderGroup::Text(100, "AcDbXrecord".into()));
            g.push(HeaderGroup::Int(280, i64::from(x.cloning_flags)));
            for entry in x.items(item.version).ok()? {
                let code = i32::from(entry.code);
                match entry.value {
                    XValue::Text(t) => g.push(HeaderGroup::Text(code, t)),
                    XValue::Int(n) => g.push(HeaderGroup::Int(code, n)),
                    XValue::Real(r) if r.is_finite() => g.push(HeaderGroup::Real(code, r)),
                    XValue::Point(p) if p.iter().all(|v| v.is_finite()) => {
                        p3(&mut g, entry.code, p)
                    }
                    XValue::Handle(h) => g.push(HeaderGroup::Handle(code, h)),
                    XValue::Binary(b) => g.push(HeaderGroup::Text(code, hex(&b))),
                    _ => return None,
                }
            }
            "XRECORD"
        }
        DecodedEntity::Placeholder(_) => "ACDBPLACEHOLDER",
        _ => return None,
    };
    Some(EntityRecord {
        kind,
        handle: item.handle,
        owner: item.refs.owner,
        block: 0,
        reactors: if item.refs.reactors.iter().all(|h| defined.contains(h)) {
            item.refs.reactors.clone()
        } else {
            Vec::new()
        },
        xdictionary: item.refs.xdictionary.filter(|h| defined.contains(h)),
        groups: g,
    })
}

/// The objects to write when the output targets `version`. `defined` are the
/// handles of everything the output defines: the objects of `plan`, the table
/// records, the entities and the block markers. `tables` are the tables written,
/// whose order and the entities' places order the objects they own. Empty for
/// targets before R2000.
pub fn build(
    version: DxfVersion,
    header: &HeaderVars,
    plan: Plan,
    defined: &HashSet<u64>,
    tables: &[Table],
) -> Objects {
    let mut out = Objects {
        records: Vec::new(),
        skipped: plan.skipped.clone(),
    };
    if !version.supports_subclass_markers() || version == DxfVersion::R14 {
        return out;
    }
    let root = header
        .get_handle("DICTIONARY_NAMED_OBJECTS")
        .map_or(0, |h| h.value);
    let mut by_handle: HashMap<u64, EntityRecord> = HashMap::new();
    for item in &plan.items {
        match record(item, defined) {
            Some(r) => {
                by_handle.insert(item.handle, r);
            }
            None => {
                let label = item_label(item);
                *out.skipped
                    .entry(format!("{label} (not written)"))
                    .or_default() += 1;
            }
        }
    }

    // Where the owners of the objects sit in TABLES, BLOCKS and ENTITIES.
    let mut place: HashMap<u64, (u8, usize, usize, usize)> = HashMap::new();
    let mut block_rank: HashMap<u64, usize> = HashMap::new();
    let mut at = 0usize;
    for t in tables {
        place.insert(t.handle, (0, at, 0, 0));
        at += 1;
        for (i, r) in t.records.iter().enumerate() {
            place.insert(r.handle, (0, at, 0, 0));
            at += 1;
            if t.name == "BLOCK_RECORD" {
                block_rank.insert(r.handle, i);
            }
        }
    }
    // An entity sits where its block puts it, an attribute or a vertex where its
    // parent does, after the parent's earlier children.
    for (entity, (block, index)) in &plan.entities {
        let (mut top, mut top_index) = (*block, *index);
        for _ in 0..8 {
            match plan.entities.get(&top) {
                Some((b, i)) => (top, top_index) = (*b, *i),
                None => break,
            }
        }
        let (class, rank) = if top == plan.model_space {
            (2, 0)
        } else if top == plan.paper_space {
            (2, 1)
        } else {
            (1, block_rank.get(&top).copied().unwrap_or(usize::MAX))
        };
        place.insert(*entity, (class, rank, top_index, *index));
    }

    // The walk: the root dictionary, then the objects not owned by an object,
    // then each one's dictionary entries and extension dictionary, level by level.
    let nodes: HashMap<u64, &Node> = plan.nodes.iter().map(|n| (n.handle, n)).collect();
    let mut seeds: Vec<&Node> = plan
        .nodes
        .iter()
        .filter(|n| n.handle != root && !nodes.contains_key(&n.owner))
        .collect();
    seeds.sort_by_key(|n| {
        (
            place.get(&n.owner).copied().unwrap_or((
                3,
                usize::try_from(n.owner).unwrap_or(usize::MAX),
                0,
                0,
            )),
            n.handle,
        )
    });
    let mut order: Vec<u64> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<u64> = VecDeque::from([root]);
    queue.extend(seeds.iter().map(|n| n.handle));
    while let Some(h) = queue.pop_front() {
        if !seen.insert(h) {
            continue;
        }
        let Some(node) = nodes.get(&h) else {
            continue;
        };
        order.push(h);
        queue.extend(node.entries.iter().copied());
        queue.extend(node.xdictionary);
    }
    for n in &plan.nodes {
        if seen.insert(n.handle) {
            order.push(n.handle);
        }
    }
    for h in order {
        if let Some(r) = by_handle.remove(&h) {
            out.records.push(r);
        }
    }
    out
}

/// Write `objects` as an `OBJECTS` section.
pub fn write(w: &mut crate::dxf::DxfWriter, objects: &Objects) {
    w.begin_section("OBJECTS");
    for (kind, n) in &objects.skipped {
        w.write_comment(&format!("skipped: {kind} x{n}"));
    }
    for r in &objects.records {
        write_record(w, r);
    }
    w.end_section();
}
