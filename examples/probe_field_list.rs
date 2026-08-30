//! Try a candidate field list against one real object record.
//!
//! The honesty rule this crate holds every object decoder to is that a
//! record counts as decoded only when its data fields end *exactly* on
//! the data-stream boundary — the first bit of the object's string
//! stream on R2007+, or the `RL` object-data-size on R2000-R2007. This
//! probe is how a candidate field list gets measured against that
//! boundary before any of it is written into a decoder.
//!
//! ```sh
//! cargo run --release --example probe_field_list -- \
//!     samples/arc_2010.dwg 42 TV,BS,BS,BS,BD,BD,BS,RC,BS,BL
//! ```
//!
//! Field-spec tokens are the ODA spec's type codes: `B`, `BB`, `3B`,
//! `BS`, `BSU`, `BL`, `BLU`, `BLL`, `BD`, `RC`, `RS`, `RL`, `RD`, `TV`,
//! and `CMC`. A `TV` consumes no data-stream bits on R2007+ (its
//! characters live in the string stream) and is read inline otherwise.
//! Prefix a token with a count, e.g. `4*BD`, to repeat it.
//!
//! Each field prints its bit offset, width and decoded value, and the
//! run ends with the delta from the data-stream boundary: `delta 0` is
//! the only result that means the field list is right.

use dwg::bitcursor::BitCursor;
use dwg::error::Result;
use dwg::string_stream::{self, StringReader};
use dwg::{DwgFile, Version};

/// Advance `c` past the EED chain of the common object data.
fn skip_eed(c: &mut BitCursor<'_>) -> Result<()> {
    for _ in 0..256 {
        let size = c.read_bs_u()? as usize;
        if size == 0 {
            return Ok(());
        }
        let _appid = c.read_handle()?;
        for _ in 0..size {
            let _ = c.read_rc()?;
        }
    }
    Ok(())
}

/// Read a `TV` from the string stream when the file has one, inline otherwise.
fn read_tv(
    c: &mut BitCursor<'_>,
    strings: &mut Option<StringReader<'_>>,
    _version: Version,
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
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

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

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: <file.dwg> <handle> <field-spec>");
    let handle: u64 = args
        .next()
        .expect("usage: <file.dwg> <handle> <field-spec>")
        .parse()
        .expect("handle must be decimal");
    let spec = args.next().unwrap_or_default();

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
    skip_eed(&mut c)?;
    let num_reactors = c.read_bl()?;
    if version.is_r2004_plus() {
        let _ = c.read_b()?;
    }
    if matches!(version, Version::R2013 | Version::R2018) && c.read_b()? {
        let _ = c.read_rc()?;
    }

    println!("{path}  ({version})  handle {handle}");
    println!(
        "body starts at bit {}, data ends at {:?}, budget {:?}, num_reactors {num_reactors}",
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
            "CMC" => {
                let index_ = c.read_bs()?;
                let mut extra = String::new();
                if version.is_r2004_plus() {
                    let rgb = c.read_bl()?;
                    let flag = c.read_rc()?;
                    extra = format!(" rgb {rgb:#010X} flag {flag}");
                    if flag & 1 != 0 {
                        let _ = read_tv(&mut c, &mut strings, version)?;
                    }
                    if flag & 2 != 0 {
                        let _ = read_tv(&mut c, &mut strings, version)?;
                    }
                }
                format!("{index_}{extra}")
            }
            "TV" => format!("{:?}", read_tv(&mut c, &mut strings, version)?),
            other => panic!("unknown field token {other}"),
        };
        let now = c.position_bits();
        println!(
            "  [{index:>3}] {token:<4} @{at:<5} w{:<3} = {value}",
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
