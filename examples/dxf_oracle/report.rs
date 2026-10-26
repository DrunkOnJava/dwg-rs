//! Aggregating field scores into the agreement tables, the TSV and the ratchet.

use crate::corpus::{self, LoadedPair};
use crate::pairing;
use crate::scorers::{self, FieldScore, Status};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Counts of one field of one type on one release.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub equal: usize,
    pub default: usize,
    pub different: usize,
    pub dwg_only: usize,
}

impl Counts {
    fn add(&mut self, s: Status) {
        match s {
            Status::Equal => self.equal += 1,
            Status::Default => self.default += 1,
            Status::Different => self.different += 1,
            Status::DwgOnly => self.dwg_only += 1,
            Status::NotDecoded => {}
        }
    }

    fn total(&self) -> usize {
        self.equal + self.default + self.different + self.dwg_only
    }

    /// One matrix cell: `=` every record agrees (`=d` some by default), `≠k`
    /// k disagree, `-` the DXF has no group for it.
    fn cell(&self) -> String {
        if self.different > 0 {
            format!("≠{}/{}", self.different, self.total())
        } else if self.dwg_only == self.total() {
            "-".into()
        } else if self.dwg_only > 0 {
            format!("-{}/{}", self.dwg_only, self.total())
        } else if self.default > 0 {
            "=d".into()
        } else {
            "=".into()
        }
    }
}

/// (type, field, code) -> release -> counts.
type FieldTable = BTreeMap<(String, String, String), BTreeMap<String, Counts>>;

pub struct Scored {
    pub fields: FieldTable,
    /// (type, code) -> release -> groups the decoder does not surface.
    pub not_decoded: BTreeMap<(String, String), BTreeMap<String, usize>>,
    /// (type, reason) -> release -> paired records no scorer covered.
    pub unscored: BTreeMap<(String, String), BTreeMap<String, usize>>,
    /// (type, release) -> paired records scored.
    pub records: BTreeMap<(String, String), usize>,
    /// Every disagreement: file, handle, type, field, code, DWG, DXF.
    pub differences: Vec<[String; 7]>,
    pub releases: BTreeSet<String>,
}

/// Scores every paired record of every loaded drawing.
pub fn score_all(loaded: &[LoadedPair]) -> Scored {
    let mut out = Scored {
        fields: BTreeMap::new(),
        not_decoded: BTreeMap::new(),
        unscored: BTreeMap::new(),
        records: BTreeMap::new(),
        differences: Vec::new(),
        releases: BTreeSet::new(),
    };
    for l in loaded {
        let rel = corpus::release(l.version);
        out.releases.insert(rel.clone());
        let p = pairing::pair(l);
        let index: scorers::DwgIndex<'_> = l.records.iter().map(|r| (r.handle, r)).collect();
        for (d, x) in &p.paired {
            match scorers::score(d, &index, &l.dxf, x) {
                Ok(scores) => {
                    *out.records
                        .entry((d.label.clone(), rel.clone()))
                        .or_default() += 1;
                    add(&mut out, &rel, l, d.handle, &d.label, scores);
                }
                Err(reason) => {
                    // The decoder's own message is coverage_report's to show;
                    // here only its kind.
                    let reason = reason.split(':').next().unwrap_or("").to_string();
                    *out.unscored
                        .entry((d.label.clone(), reason))
                        .or_default()
                        .entry(rel.clone())
                        .or_default() += 1;
                }
            }
        }
    }
    out
}

fn add(
    out: &mut Scored,
    rel: &str,
    l: &LoadedPair,
    handle: u64,
    label: &str,
    scores: Vec<FieldScore>,
) {
    for s in scores {
        if s.status == Status::NotDecoded {
            *out.not_decoded
                .entry((label.to_string(), s.code))
                .or_default()
                .entry(rel.to_string())
                .or_default() += 1;
            continue;
        }
        if s.status == Status::Different {
            out.differences.push([
                l.name.clone(),
                format!("{handle:X}"),
                label.to_string(),
                s.field.clone(),
                s.code.clone(),
                s.dwg.clone(),
                s.dxf.clone(),
            ]);
        }
        out.fields
            .entry((label.to_string(), s.field, s.code))
            .or_default()
            .entry(rel.to_string())
            .or_default()
            .add(s.status);
    }
}

fn release_order(releases: &BTreeSet<String>) -> Vec<String> {
    const ORDER: [&str; 7] = ["R14", "R2000", "R2004", "R2007", "R2010", "R2013", "R2018"];
    let mut v: Vec<String> = ORDER
        .iter()
        .filter(|r| releases.contains(**r))
        .map(|r| r.to_string())
        .collect();
    v.extend(
        releases
            .iter()
            .filter(|r| !ORDER.contains(&r.as_str()))
            .cloned(),
    );
    v
}

/// The per-type matrices: one row per field, one column per release.
pub fn print_fields(s: &Scored) {
    let rels = release_order(&s.releases);
    println!("\n== Field agreement by type and release (#102)\n");
    println!("Cells: = every record agrees, =d some through the DXF default, ≠k/n k of n");
    println!("disagree, - the DXF has no group for the field. Totals: equal / default /");
    println!("different / dwg-only over every release.\n");
    type Row<'a> = (&'a (String, String, String), &'a BTreeMap<String, Counts>);
    let mut by_type: BTreeMap<&str, Vec<Row<'_>>> = BTreeMap::new();
    for (k, v) in &s.fields {
        by_type.entry(k.0.as_str()).or_default().push((k, v));
    }
    for (ty, rows) in by_type {
        let recs: Vec<String> = rels
            .iter()
            .map(|r| {
                s.records
                    .get(&(ty.to_string(), r.clone()))
                    .copied()
                    .unwrap_or(0)
                    .to_string()
            })
            .collect();
        println!(
            "{ty} ({}) records per release: {}",
            scorers::section_of(ty),
            recs.join(" ")
        );
        let mut header = format!("  {:<40} {:<12}", "field", "group");
        for r in &rels {
            let _ = write!(header, " {r:>7}");
        }
        println!("{header}   total");
        for ((_, field, code), per) in rows {
            let mut line = format!("  {field:<40} {code:<12}");
            let mut t = Counts::default();
            for r in &rels {
                let cell = per.get(r).map_or(String::new(), |c| {
                    t.equal += c.equal;
                    t.default += c.default;
                    t.different += c.different;
                    t.dwg_only += c.dwg_only;
                    c.cell()
                });
                let _ = write!(line, " {cell:>7}");
            }
            println!(
                "{line}   {}/{}/{}/{}",
                t.equal, t.default, t.different, t.dwg_only
            );
        }
        println!();
    }
}

/// Every disagreement, one line each.
pub fn print_differences(s: &Scored, limit: usize) {
    println!("== Disagreements ({} values)\n", s.differences.len());
    let mut shown: BTreeMap<(String, String), usize> = BTreeMap::new();
    for d in &s.differences {
        let n = shown.entry((d[2].clone(), d[3].clone())).or_default();
        *n += 1;
        if *n > limit {
            continue;
        }
        println!(
            "  {:<24} {:>5} {:<14} {:<34} {:<10} DWG {}  DXF {}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6]
        );
    }
    for ((ty, field), n) in &shown {
        if *n > limit {
            println!("  ... {ty} {field}: {} more", n - limit);
        }
    }
}

/// The DXF groups no decoded field maps to, and the records no scorer covers.
pub fn print_gaps(s: &Scored) {
    let rels = release_order(&s.releases);
    println!("\n== DXF groups the decoders do not surface (not-decoded), by type\n");
    let mut by_type: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for ((ty, code), per) in &s.not_decoded {
        let n: usize = per.values().sum();
        by_type
            .entry(ty.as_str())
            .or_default()
            .push(format!("{code}×{n}"));
    }
    for (ty, codes) in by_type {
        println!("  {ty:<22} {}", codes.join("  "));
    }
    println!("\n== Paired records no scorer covers\n");
    for ((ty, reason), per) in &s.unscored {
        let cells: Vec<String> = rels
            .iter()
            .map(|r| format!("{r}:{}", per.get(r).copied().unwrap_or(0)))
            .collect();
        println!("  {ty:<24} {reason:<16} {}", cells.join(" "));
    }
}

/// One line per ODA spec section: records scored, field values by status,
/// DXF groups not surfaced.
pub fn print_sections(s: &Scored) {
    println!("\n== Summary by spec section\n");
    println!(
        "{:<10} {:<22} {:>7} {:>7} {:>7} {:>9} {:>8} {:>11} {:>8}",
        "section",
        "type",
        "records",
        "equal",
        "default",
        "different",
        "dwg-only",
        "not-decoded",
        "unscored"
    );
    for (sec, ty, records, t, nd, un) in section_rows(s) {
        println!(
            "{sec:<10} {ty:<22} {records:>7} {:>7} {:>7} {:>9} {:>8} {nd:>11} {un:>8}",
            t.equal, t.default, t.different, t.dwg_only
        );
    }
}

/// The section summary as a Markdown table, for a CI job summary (#103).
pub fn sections_markdown(s: &Scored) -> String {
    let mut out = String::from(
        "| Section | Type | Records | Equal | Default | Different | DWG only | Not decoded | Unscored |\n|---|---|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for r in section_rows(s) {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.0, r.1, r.2, r.3.equal, r.3.default, r.3.different, r.3.dwg_only, r.4, r.5
        );
    }
    out
}

/// (section, type, records, field counts, not decoded, unscored), in
/// section order.
fn section_rows(s: &Scored) -> Vec<(&'static str, String, usize, Counts, usize, usize)> {
    let mut types: BTreeSet<&str> = BTreeSet::new();
    types.extend(s.fields.keys().map(|k| k.0.as_str()));
    types.extend(s.unscored.keys().map(|k| k.0.as_str()));
    // Paired records with nothing to compare (ENDBLK, SEQEND) still count.
    types.extend(s.records.keys().map(|k| k.0.as_str()));
    let mut rows: Vec<(&'static str, String, usize, Counts, usize, usize)> = types
        .into_iter()
        .map(|ty| {
            let records: usize = s
                .records
                .iter()
                .filter(|(k, _)| k.0 == ty)
                .map(|(_, v)| v)
                .sum();
            let mut t = Counts::default();
            for (_, per) in s.fields.iter().filter(|(k, _)| k.0 == ty) {
                for c in per.values() {
                    t.equal += c.equal;
                    t.default += c.default;
                    t.different += c.different;
                    t.dwg_only += c.dwg_only;
                }
            }
            let nd = s
                .not_decoded
                .iter()
                .filter(|(k, _)| k.0 == ty)
                .map(|(_, v)| v.values().sum::<usize>())
                .sum();
            let un = s
                .unscored
                .iter()
                .filter(|(k, _)| k.0 == ty)
                .map(|(_, v)| v.values().sum::<usize>())
                .sum();
            (scorers::section_of(ty), ty.to_string(), records, t, nd, un)
        })
        .collect();
    rows.sort_by(|a, b| section_key(a.0).cmp(&section_key(b.0)).then(a.1.cmp(&b.1)));
    rows
}

fn section_key(sec: &str) -> Vec<u32> {
    sec.trim_start_matches('§')
        .split('.')
        .map(|p| p.parse().unwrap_or(u32::MAX))
        .collect()
}

/// The deterministic TSV the ratchet compares: one row per release, type,
/// field and group.
/// The header line of [`tsv`] and of every baseline [`ratchet`] reads.
const TSV_HEADER: &str =
    "release\tsection\ttype\tfield\tgroup\tequal\tdefault\tdifferent\tdwg_only";

pub fn tsv(s: &Scored) -> String {
    let mut out = format!("{TSV_HEADER}\n");
    let mut rows = Vec::new();
    for ((ty, field, code), per) in &s.fields {
        for (rel, c) in per {
            rows.push(format!(
                "{rel}\t{}\t{ty}\t{field}\t{code}\t{}\t{}\t{}\t{}",
                scorers::section_of(ty),
                c.equal,
                c.default,
                c.different,
                c.dwg_only
            ));
        }
    }
    rows.sort();
    for r in rows {
        out.push_str(&r);
        out.push('\n');
    }
    out
}

/// Compares this run with a baseline TSV: every row's agreeing count
/// (equal + default) must be at least the baseline's. Returns the drops,
/// or an error when the baseline is not such a TSV (wrong header, a row
/// without nine cells, a count that is not a number, or no rows at all),
/// so a damaged baseline fails the run instead of passing it.
pub fn ratchet(s: &Scored, baseline: &str) -> Result<Vec<String>, String> {
    let mut now: BTreeMap<(String, String, String, String), usize> = BTreeMap::new();
    for ((ty, field, code), per) in &s.fields {
        for (rel, c) in per {
            now.insert(
                (rel.clone(), ty.clone(), field.clone(), code.clone()),
                c.equal + c.default,
            );
        }
    }
    let mut lines = baseline.lines().enumerate();
    match lines.next() {
        Some((_, h)) if h == TSV_HEADER => {}
        _ => {
            return Err(format!(
                "the first line is not the oracle TSV header ({TSV_HEADER:?})"
            ));
        }
    }
    let mut drops = Vec::new();
    let mut rows = 0;
    for (n, line) in lines {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 9 {
            return Err(format!("line {}: {} cells, not 9", n + 1, f.len()));
        }
        let count = |i: usize| {
            f[i].parse::<usize>()
                .map_err(|_| format!("line {}: count {:?} is not a number", n + 1, f[i]))
        };
        for i in 7..9 {
            count(i)?;
        }
        rows += 1;
        let key = (
            f[0].to_string(),
            f[2].to_string(),
            f[3].to_string(),
            f[4].to_string(),
        );
        let base = count(5)? + count(6)?;
        match now.get(&key).copied() {
            Some(got) if got < base => drops.push(format!(
                "{} {} {} {}: {base} agreeing in the baseline, {got} now",
                f[0], f[2], f[3], f[4]
            )),
            Some(_) => {}
            // A renamed field or a scorer that no longer runs: not a
            // regression of a value, but the baseline must say so.
            None if base > 0 => drops.push(format!(
                "{} {} {} {}: row absent from this run ({base} agreeing in the baseline); \
                 a renamed or removed field needs the regenerated TSV committed with it",
                f[0], f[2], f[3], f[4]
            )),
            None => {}
        }
    }
    if rows == 0 {
        return Err("no rows".to_string());
    }
    Ok(drops)
}
