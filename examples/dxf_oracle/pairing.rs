//! Pairing every DWG record with the DXF record of the same handle (#101).

use crate::corpus::{DwgRecord, LoadedPair, record_key};
use dwg::dxf_reader::DxfRecord;
use std::collections::BTreeSet;

/// Types whose records AutoCAD creates afresh while saving, so the DWG and
/// the DXF of one drawing give the same object different handles: their
/// one-sided records are paired with each other in file order instead, with
/// the DXF types each may be exported as (DATATABLE, the tables behind a
/// dynamic block's lookup parameters; the pairing its probe uses,
/// examples/probe_data_table.rs). The R2000 export writes DATATABLE under
/// its older name ACDBDATATABLE, with the same groups.
const PAIRED_IN_ORDER: &[(&str, &[&str])] = &[("DATATABLE", &["DATATABLE", "ACDBDATATABLE"])];

/// How the two sides of one drawing line up by handle.
pub struct Pairing<'a> {
    /// Same handle, and the DXF record is the type the DWG one exports as.
    pub paired: Vec<(&'a DwgRecord, &'a DxfRecord)>,
    /// Same handle, different types.
    pub type_differs: Vec<(&'a DwgRecord, &'a DxfRecord)>,
    /// DWG records whose handle the DXF does not define.
    pub dwg_only: Vec<&'a DwgRecord>,
    /// DXF records with a handle the DWG walk did not reach.
    pub dxf_only: Vec<&'a DxfRecord>,
    /// DXF records with no handle (CLASS, ENDTAB, R12 table entries).
    pub dxf_unkeyed: usize,
}

/// Pairs `pair`'s records by handle.
pub fn pair(pair: &LoadedPair) -> Pairing<'_> {
    let mut out = Pairing {
        paired: Vec::new(),
        type_differs: Vec::new(),
        dwg_only: Vec::new(),
        dxf_only: Vec::new(),
        dxf_unkeyed: 0,
    };
    let mut seen = BTreeSet::new();
    for rec in &pair.records {
        seen.insert(rec.handle);
        match pair.dxf.record_by_handle(rec.handle) {
            Some(d) if rec.dxf_key.as_deref() == Some(record_key(d).as_str()) => {
                out.paired.push((rec, d));
            }
            Some(d) => out.type_differs.push((rec, d)),
            None => out.dwg_only.push(rec),
        }
    }
    for r in &pair.dxf.records {
        match r.handle {
            Some(h) if !seen.contains(&h) => out.dxf_only.push(r),
            Some(_) => {}
            None => out.dxf_unkeyed += 1,
        }
    }
    for (key, exported_as) in PAIRED_IN_ORDER {
        let dwg: Vec<&DwgRecord> = out
            .dwg_only
            .iter()
            .copied()
            .filter(|r| r.dxf_key.as_deref() == Some(*key))
            .collect();
        let dxf: Vec<&DxfRecord> = out
            .dxf_only
            .iter()
            .copied()
            .filter(|r| exported_as.contains(&record_key(r).as_str()))
            .collect();
        let n = dwg.len().min(dxf.len());
        for (a, b) in dwg.iter().zip(&dxf).take(n) {
            out.paired.push((a, b));
        }
        let (dwg, dxf) = (&dwg[..n], &dxf[..n]);
        out.dwg_only
            .retain(|r| !dwg.iter().any(|x| std::ptr::eq(*x, *r)));
        out.dxf_only
            .retain(|r| !dxf.iter().any(|x| std::ptr::eq(*x, *r)));
    }
    out
}

impl Pairing<'_> {
    /// The highest handle the two files share. AutoCAD hands out handles in
    /// ascending order, so a one-sided record above it was created after the
    /// last shared one: by the save that wrote its own file.
    pub fn last_shared_handle(&self) -> u64 {
        self.paired
            .iter()
            .chain(&self.type_differs)
            .map(|(d, _)| d.handle)
            .max()
            .unwrap_or(0)
    }
}
