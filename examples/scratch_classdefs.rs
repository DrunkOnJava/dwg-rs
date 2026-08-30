//! Scratch: full class-table rows.
use dwg::error::Result;
use dwg::DwgFile;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    let classes = file.class_map().unwrap()?;
    println!("max_class_number {}", classes.max_class_number);
    for d in &classes.classes {
        println!(
            "{:>4} {:<38} ver {:>3} proxy {:<5} item 0x{:04X} n {:>3} dwgver {:>3} maint {:>4} app {:?} cpp {:?}",
            d.class_number, d.dxf_class_name, d.version, d.was_a_proxy, d.item_class_id,
            d.num_objects, d.dwg_version, d.maintenance_version, d.app_name, d.cpp_class_name
        );
    }
    Ok(())
}
