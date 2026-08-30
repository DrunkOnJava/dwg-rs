//! Where does the R2007+ common entity preamble (§19.4.1) end, and
//! does the graphics-preview block use an `RL` or a `BLL` size?
//!
//! # What this measures
//!
//! Every entity record carries a self-checking boundary
//! ([`dwg::object::data_end_bit`]): on R2010+ it is the first bit of the
//! record's string stream (or the start of its handle stream when the
//! record holds no strings). The common entity preamble must fit inside
//! that budget — a preamble that overruns it is decoding the wrong
//! fields.
//!
//! For every entity record this probe traces the preamble field by
//! field, twice:
//!
//! - **RL** — the graphics-preview block's size is read as a 32-bit `RL`
//!   (the R13-R2007 shape);
//! - **BLL** — the size is read as a `BLL` (§2.4: a 3-bit byte count
//!   then that many little-endian bytes), the R2010+ shape.
//!
//! It prints, per record, whether each variant completed and how many
//! bits are left between the end of the preamble and `data_end_bit`. A
//! variant that overruns the record, or that leaves a negative budget,
//! is wrong.
//!
//! ```sh
//! cargo run --release --example probe_entity_preamble -- samples/sample_AC1032.dwg
//! cargo run --release --example probe_entity_preamble -- samples/sample_AC1032.dwg 526
//! ```

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::version::Version;
use dwg::{DwgFile, ObjectType};

/// How the graphics-preview block's size field is encoded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GfxSize {
    Rl,
    Bll,
}

/// Trace one entity's preamble, returning `(end_bit, had_graphics, gfx_bytes, log)`.
fn trace(
    c: &mut BitCursor<'_>,
    version: Version,
    gfx: GfxSize,
    log: &mut Vec<String>,
) -> Result<(bool, u64)> {
    // EED chain.
    let mut eed_groups = 0usize;
    loop {
        let size = c.read_bs_u()?;
        if size == 0 {
            break;
        }
        eed_groups += 1;
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
        if eed_groups > 256 {
            break;
        }
    }
    log.push(format!("eed_groups={eed_groups} @{}", c.position_bits()));

    let had_graphics = c.read_b()?;
    let mut gfx_bytes = 0u64;
    if had_graphics {
        let at = c.position_bits();
        gfx_bytes = match gfx {
            GfxSize::Rl => c.read_rl()? as u64,
            GfxSize::Bll => c.read_bll()?,
        };
        log.push(format!(
            "gfx size field @{at}..{} = {gfx_bytes}",
            c.position_bits()
        ));
        for _ in 0..gfx_bytes {
            let _ = c.read_rc()?;
        }
    }
    log.push(format!(
        "had_graphics={had_graphics} gfx_bytes={gfx_bytes} @{}",
        c.position_bits()
    ));

    let raw_mode = c.read_bb()?;
    let num_reactors = c.read_bl()?;
    let no_xdict = if version.is_r2004_plus() {
        c.read_b()?
    } else {
        true
    };
    let binary_chain = if matches!(version, Version::R2013 | Version::R2018) {
        c.read_b()?
    } else {
        false
    };
    log.push(format!(
        "entmode={raw_mode} reactors={num_reactors} no_xdict={no_xdict} ds={binary_chain} @{}",
        c.position_bits()
    ));

    let color_raw = c.read_bs_u()?;
    if version.is_r2004_plus() {
        let flags = color_raw >> 8;
        if flags & 0x20 != 0 {
            let _ = c.read_bl()?;
        }
        if flags & 0x40 == 0 && flags & 0x80 != 0 {
            let _ = c.read_bl()?;
        }
    }
    let ltype_scale = c.read_bd()?;
    let ltype_flags = c.read_bb()?;
    let plotstyle = c.read_bb()?;
    log.push(format!(
        "color=0x{color_raw:04X} lts={ltype_scale} ltf={ltype_flags} ps={plotstyle} @{}",
        c.position_bits()
    ));
    let (material, shadow) = if version.is_r2007_plus() {
        (c.read_bb()?, c.read_rc()?)
    } else {
        (0, 0)
    };
    if version.is_r2010_plus() {
        let _ = c.read_b()?;
        let _ = c.read_b()?;
        let _ = c.read_b()?;
    }
    let invis = c.read_bs()?;
    let lw = c.read_rc()?;
    log.push(format!(
        "mat={material} shadow={shadow} invis={invis} lw={lw} END @{}",
        c.position_bits()
    ));
    Ok((had_graphics, gfx_bytes))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: <file.dwg> [type-code]");
    let want: Option<u16> = args.next().map(|s| {
        let t = s.trim_start_matches("0x");
        u16::from_str_radix(t, if s.starts_with("0x") { 16 } else { 10 })
            .expect("type code must be decimal or 0x-hex")
    });

    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;
    let class_map = file.class_map().and_then(std::result::Result::ok);

    println!("{path}  ({version})");
    for object in &objects {
        if let Some(code) = want {
            if object.type_code != code {
                continue;
            }
        } else if !object.is_entity() {
            continue;
        }
        let label = match object.kind {
            ObjectType::Custom(code) => class_map
                .as_ref()
                .and_then(|m| m.by_type_code(code))
                .map(|d| d.dxf_class_name.clone())
                .unwrap_or_else(|| format!("{}", object.kind)),
            other => format!("{other}"),
        };
        let data_end = dwg::object::data_end_bit(object, version);
        let body = dwg::object::body_cursor(object, version).map(|c| c.position_bits());
        println!();
        println!(
            "0x{:04X} {label} handle=0x{:X} payload={}b body_at={:?} data_end={:?}",
            object.type_code,
            object.handle.value,
            object.raw.len() * 8,
            body.as_ref().ok(),
            data_end
        );
        for (name, gfx) in [("RL ", GfxSize::Rl), ("BLL", GfxSize::Bll)] {
            let mut log = Vec::new();
            let mut c = match dwg::object::body_cursor(object, version) {
                Ok(c) => c,
                Err(e) => {
                    println!("  {name}: body_cursor failed: {e}");
                    continue;
                }
            };
            match trace(&mut c, version, gfx, &mut log) {
                Ok((had_gfx, gfx_bytes)) => {
                    let end = c.position_bits();
                    let budget = data_end.map(|d| d as isize - end as isize);
                    println!("  {name}: ok end={end} budget={budget:?} gfx={had_gfx}/{gfx_bytes}B");
                }
                Err(e) => println!("  {name}: ERR {e}"),
            }
            for line in &log {
                println!("      {line}");
            }
        }
    }
    Ok(())
}
