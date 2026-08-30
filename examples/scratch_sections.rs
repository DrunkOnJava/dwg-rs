//! Scratch: list sections.
use dwg::error::Result;
use dwg::DwgFile;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("usage: <file.dwg>");
    let file = DwgFile::open(&path)?;
    for s in file.sections() {
        println!("{:?}", s);
    }
    Ok(())
}
