//! Try a candidate field list against one real **entity** record.
//!
//! `examples/probe_field_list.rs` does this for non-entity objects: it
//! skips the common *object* prefix (§20.4.1 "Common object data") and
//! then walks a token list. Entities carry a different, longer preamble
//! — the common entity data of §20.4.1, including the graphics-preview
//! block — so they need their own harness. This is it.
//!
//! The invariant is the same one every decoder in this crate is held
//! to: an entity's data-stream fields must end *exactly* on the first
//! bit of its string stream (R2010+), or on the start of its handle
//! stream when it carries no strings. `delta 0` is the only result that
//! means the field list is right.
//!
//! ```sh
//! cargo run --release --example probe_entity_field_list -- \
//!     samples/sample_AC1032.dwg 0xB1A BS,BL,B,B,3BD,3BD,BL,BL,BD
//! ```
//!
//! Field-spec tokens are the ODA spec's type codes: `B`, `BB`, `3B`,
//! `BS`, `BSU`, `BL`, `BLU`, `BLL`, `BD`, `RC`, `RS`, `RL`, `RD`,
//! `2RD`, `2BD`, `3BD`, `TV`, `CMC`, and `H`. Two of them consume no
//! data-stream bits on R2007+: `TV` (its characters live in the string
//! stream) and `H` (object references live in the handle stream).
//! Prefix a token with a count, e.g. `16*BD`, to repeat it.
//!
//! A fourth argument `--at=<bit>` starts the walk at an absolute bit
//! offset inside the payload instead of at the end of the common entity
//! preamble. That is how a tail-of-record hypothesis gets tested without
//! re-describing every field in front of it.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, Version};

fn expand(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in spec.split(',').filter(|t| !t.trim().is_empty()) {
        let token = token.trim();
        match token.split_once('*') {
            Some((n, t)) => {
                let n: usize = n.parse().expect("repeat count must be a number");
                for _ in 0..n {
                    out.push(t.to_ascii_uppercase());
                }
            }
            None => out.push(token.to_ascii_uppercase()),
        }
    }
    out
}

fn read_tv(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    version: Version,
) -> Result<String> {
    if let Some(reader) = strings.as_mut() {
        return reader.read_tv();
    }
    let len = c.read_bs_u()? as usize;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        bytes.push(c.read_rc()?);
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    let _ = version;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: <file.dwg> <handle> <field-spec>");
    let handle_arg = args
        .next()
        .expect("usage: <file.dwg> <handle> <field-spec>");
    let handle: u64 = if let Some(hex) = handle_arg.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).expect("handle must be decimal or 0x-hex")
    } else {
        handle_arg.parse().expect("handle must be decimal or 0x-hex")
    };
    let spec = args.next().unwrap_or_default();
    let start_at: Option<usize> = args.next().and_then(|a| {
        a.strip_prefix("--at=")
            .map(|v| v.parse().expect("--at= wants a bit offset"))
    });

    let file = DwgFile::open(&path)?;
    let version = file.version();
    let objects = file
        .all_objects()
        .expect("this version has no object-stream walk")?;
    let object = objects
        .iter()
        .find(|o| o.handle.value == handle)
        .unwrap_or_else(|| panic!("no object with handle {handle}"));

    let data_end = dwg::object::data_end_bit(object, version);
    let mut strings = match string_stream::locate(&object.raw, version) {
        Some(stream) => Some(StringReader::new(&object.raw, stream)?),
        None if version.is_r2007_plus() => Some(StringReader::empty(&object.raw)),
        None => None,
    };

    let mut c = dwg::object::body_cursor(object, version)?;
    let graphics = match start_at {
        Some(bit) => {
            while c.position_bits() < bit {
                let _ = c.read_b()?;
            }
            false
        }
        None => dwg::common_entity::read_common_entity_data(&mut c, version)?.had_graphics,
    };

    println!(
        "{path}  ({version})  handle 0x{handle:X}  type 0x{:04X}",
        object.type_code
    );
    println!(
        "walk starts at bit {}, data ends at {:?}, budget {:?}, graphics={graphics}",
        c.position_bits(),
        data_end,
        data_end.map(|e| e as isize - c.position_bits() as isize),
    );

    for (index, token) in expand(&spec).iter().enumerate() {
        let at = c.position_bits();
        let value: String = match token.as_str() {
            "B" => format!("{}", c.read_b()?),
            "BB" => format!("{}", c.read_bb()?),
            "3B" => format!("{}", c.read_3b()?),
            "BS" => format!("{}", c.read_bs()?),
            "BSU" => format!("{}", c.read_bs_u()?),
            "BL" => format!("{}", c.read_bl()?),
            "BLU" => format!("{}", c.read_bl_u()?),
            "BLL" => format!("{}", c.read_bll()?),
            "BD" => format!("{}", c.read_bd()?),
            "RC" => format!("{}", c.read_rc()?),
            "RS" => format!("{}", c.read_rs()?),
            "RL" => format!("{}", c.read_rl()?),
            "RD" => format!("{}", c.read_rd()?),
            "2RD" => {
                let p = dwg::entities::read_rd2(&mut c)?;
                format!("({}, {})", p.x, p.y)
            }
            "2BD" => {
                let p = dwg::entities::read_bd2(&mut c)?;
                format!("({}, {})", p.x, p.y)
            }
            "3BD" => {
                let p = dwg::entities::read_bd3(&mut c)?;
                format!("({}, {}, {})", p.x, p.y, p.z)
            }
            "CMC" => {
                let idx = c.read_bs()?;
                let rgb = c.read_bl_u()?;
                let flag = c.read_rc()?;
                let mut extra = String::new();
                if flag & 1 != 0 {
                    extra.push_str(&format!(" name {:?}", read_tv(&mut c, &mut strings, version)?));
                }
                if flag & 2 != 0 {
                    extra.push_str(&format!(" book {:?}", read_tv(&mut c, &mut strings, version)?));
                }
                format!("idx {idx} rgb {rgb:#010X} flag {flag}{extra}")
            }
            // A handle slot: object references live in the handle
            // stream from R2007 on, so the slot consumes no data bits.
            "H" => {
                if version.is_r2007_plus() {
                    "(handle stream)".to_string()
                } else {
                    format!("{:?}", c.read_handle()?)
                }
            }
            "TV" => format!("{:?}", read_tv(&mut c, &mut strings, version)?),
            // The shared R2007+ TEXT field body (§20.4.3), as TEXT /
            // ATTRIB / ATTDEF all open with it.
            "TEXTFIELDS" => {
                let t = dwg::entities::text::read_modern_fields(&mut c)?;
                format!("ins {:?} h {} rot {}", t.insertion_point, t.height, t.rotation_angle)
            }
            // The R2007+ MTEXT field body (§20.4.46) — what a
            // multi-line ATTRIB embeds.
            "MTEXTFIELDS" => {
                let m = dwg::entities::mtext::read_modern_fields_probe(&mut c, version)?;
                format!("ins {:?} h {} cols {}", m.insertion_point, m.nominal_text_height, m.column_type)
            }
            // The common entity preamble from the entity-mode bits on —
            // what an embedded MTEXT object writes inside an ATTRIB.
            "ENTMODE" => {
                let bb = c.read_bb()?;
                let reactors = c.read_bl()?;
                let _xdict = c.read_b()?;
                let _ds = c.read_b()?;
                let color = c.read_bs_u()?;
                let lts = c.read_bd()?;
                let _ltf = c.read_bb()?;
                let _ps = c.read_bb()?;
                let _mat = c.read_bb()?;
                let _shadow = c.read_rc()?;
                let _ = c.read_b()?;
                let _ = c.read_b()?;
                let _ = c.read_b()?;
                let invis = c.read_bs()?;
                let lw = c.read_rc()?;
                format!("entmode {bb} reactors {reactors} color {color:#06X} lts {lts} invis {invis} lw {lw:#04X}")
            }
            other => panic!("unknown field token {other}"),
        };
        let now = c.position_bits();
        println!(
            "  [{index:>3}] {token:<4} @{at:<6} w{:<3} = {value}",
            now - at
        );
    }

    let at = c.position_bits();
    match data_end {
        Some(end) => println!(
            "ended at {at}, boundary {end}, delta {}",
            at as isize - end as isize
        ),
        None => println!("ended at {at}, boundary unknown"),
    }
    if let Some(end) = data_end {
        println!("remaining bits from {at} to {end}:");
        let mut line = String::new();
        for bit in at..end {
            let byte = object.raw.get(bit / 8).copied().unwrap_or(0);
            line.push(if (byte >> (7 - (bit % 8))) & 1 != 0 {
                '1'
            } else {
                '0'
            });
            if line.len() == 64 {
                println!("  @{:<6} {line}", bit + 1 - 64);
                line.clear();
            }
        }
        if !line.is_empty() {
            println!("  @{:<6} {line}", end - line.len());
        }
    }
    if let Some(reader) = strings.as_mut() {
        let mut rest = Vec::new();
        while !reader.is_exhausted() {
            match reader.read_tv() {
                Ok(s) => rest.push(s),
                Err(_) => break,
            }
        }
        println!("unread strings: {rest:?}");
    }
    Ok(())
}
