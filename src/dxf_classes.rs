//! The DXF `CLASSES` section from the drawing's class table (`AcDb:Classes`).
//!
//! One `CLASS` record per class a drawing defines, in the order of the class
//! table: the DXF record name (1), the C++ class name (2), the application name
//! (3), the proxy capability flags (90), the number of instances (91, from
//! R2004, which is the first release whose table records it), whether the class
//! was a proxy (280) and whether it is an entity class (281, item class id
//! 0x1F2 against 0x1F3 for an object).

use crate::classes::ClassMap;
use crate::dxf::{DxfVersion, DxfWriter};

/// Write `classes` as a `CLASSES` section. AutoCAD's export differs from the table
/// in two places the corpus shows: from R2004 it does not list LAYOUT and
/// ACDBPLACEHOLDER, which its newer releases hold as native types, and an R2000
/// export names `AcDbDataTable`'s record `ACDBDATATABLE` where later releases
/// (and the DWG's table) say `DATATABLE`.
pub fn write(w: &mut DxfWriter, classes: &ClassMap, with_counts: bool) {
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
        if with_counts {
            w.write_int(91, i64::from(c.num_objects));
        }
        w.write_int(280, i64::from(c.was_a_proxy));
        w.write_int(281, i64::from(c.item_class_id == 0x1F2));
    }
    w.end_section();
}
