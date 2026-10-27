//! The DXF `CLASSES` section from the drawing's class table (`AcDb:Classes`).
//!
//! One `CLASS` record per class a drawing defines, in the order of the class
//! table: the DXF record name (1), the C++ class name (2), the application name
//! (3), the proxy capability flags (90), the number of instances (91, written for
//! a target of R2004 or later), whether the class was a proxy (280) and whether
//! it is an entity class (281, item class id 0x1F2 against 0x1F3 for an object).
//!
//! The instance count comes from the class table when the drawing is R2004 or
//! later, which is the first release whose table records it, and from the object
//! stream otherwise: the number of objects whose type code is the class number.
//! Group 91 follows the target release and the count follows the source.

use std::collections::HashMap;

use crate::classes::ClassMap;
use crate::dxf::{DxfVersion, DxfWriter};
use crate::object_type::ObjectType;
use crate::reader::DwgFile;

/// The objects of the stream of `file` whose type code is each class number.
pub fn stream_counts(file: &DwgFile) -> HashMap<u16, u32> {
    let mut counts: HashMap<u16, u32> = HashMap::new();
    if let Some(Ok(raws)) = file.all_objects() {
        for raw in &raws {
            if let ObjectType::Custom(code) = raw.kind {
                *counts.entry(code).or_default() += 1;
            }
        }
    }
    counts
}

/// The instances of each class of `file`: the table's count from R2004, else
/// [`stream_counts`].
pub fn instance_counts(file: &DwgFile, classes: &ClassMap) -> HashMap<u16, u32> {
    if file.version().is_r2004_plus() {
        return classes
            .classes
            .iter()
            .map(|c| (c.class_number, c.num_objects))
            .collect();
    }
    stream_counts(file)
}

/// Write `classes` as a `CLASSES` section. AutoCAD's export differs from the table
/// in two places the corpus shows: from R2004 it does not list LAYOUT and
/// ACDBPLACEHOLDER, which its newer releases hold as native types, and an R2000
/// export names `AcDbDataTable`'s record `ACDBDATATABLE` where later releases
/// (and the DWG's table) say `DATATABLE`. `counts` are the instance counts of
/// [`instance_counts`] for a target that writes group 91, `None` for one that
/// does not (before R2004).
pub fn write(w: &mut DxfWriter, classes: &ClassMap, counts: Option<&HashMap<u16, u32>>) {
    w.begin_section("CLASSES");
    let target = w.version();
    for c in classes.classes.iter().filter(|c| {
        target == DxfVersion::R2000
            || !matches!(c.dxf_class_name.as_str(), "LAYOUT" | "ACDBPLACEHOLDER")
    }) {
        let dxf_name = match c.dxf_class_name.as_str() {
            "DATATABLE" if target == DxfVersion::R2000 => "ACDBDATATABLE",
            other => other,
        };
        w.write_string(0, "CLASS");
        w.write_string(1, &crate::dxf_header::escape_text(target, dxf_name));
        w.write_string(
            2,
            &crate::dxf_header::escape_text(target, &c.cpp_class_name),
        );
        w.write_string(3, &crate::dxf_header::escape_text(target, &c.app_name));
        w.write_int(90, i64::from(c.version as u16));
        if let Some(counts) = counts {
            let n = counts.get(&c.class_number).copied().unwrap_or(0);
            w.write_int(91, i64::from(n));
        }
        w.write_int(280, i64::from(c.was_a_proxy));
        w.write_int(281, i64::from(c.item_class_id == 0x1F2));
    }
    w.end_section();
}
