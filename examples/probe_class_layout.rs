//! Where does every field of every `AcDb:Classes` record start and end?
//!
//! # What this proves
//!
//! That the R2007+ class record ends with **five `BL`s**
//! (`num_objects`, `dwg_version`, `maintenance_version`, and two
//! unknowns) and not with the `BL`, `BS`, `BS`, `BL`, `BL` spelled out
//! in ODA Open Design Specification v5.4.1 §10.2. The two readings are
//! bit-identical while `dwg_version` and `maintenance_version` stay
//! below 256; they diverge by sixteen bits the first time one of them
//! does not, which is issue #37.
//!
//! The probe prints, per record, the absolute bit offset of every
//! field, the record's total width, and the decoded values — the class
//! numbers must run 500, 501, 502, … with no gap. It then prints the
//! §19.4.1 string-stream trailer of the section, which is the
//! independent check: the trailer's length word must equal the span
//! between the end of the last record and the end of the last string.
//!
//! ```sh
//! cargo run --release --example probe_class_layout -- samples/sample_AC1032.dwg
//! ```
//!
//! Expected on `sample_AC1032.dwg` (AC1032): 50 records, 500..=549,
//! last record ending on bit 4093, string block ending on bit 49897,
//! trailer length word 45804 = 49897 − 4093.
//!
//! On a pre-R2007 file the names are inline in each record and there is
//! no string block, so the check is the declared size instead: on
//! `arc_2004.dwg` the ten records consume 5161 of the 5168 bits the
//! header declares, the remaining 7 being byte padding.

use dwg::DwgFile;
use dwg::bitcursor::BitCursor;
use dwg::error::Result;

/// Read a `TU` (UTF-16LE) string the way the R2007+ class list stores
/// its three names: `BS` code-unit count, then that many LE `u16`s.
fn read_tu(c: &mut BitCursor<'_>) -> Result<String> {
    let len = c.read_bs_u()? as usize;
    let mut units = Vec::with_capacity(len);
    for _ in 0..len {
        let lo = u16::from(c.read_rc()?);
        let hi = u16::from(c.read_rc()?);
        units.push((hi << 8) | lo);
    }
    if units.last() == Some(&0) {
        units.pop();
    }
    Ok(String::from_utf16_lossy(&units))
}

/// Read a `TV` the pre-R2007 way: `BS` byte count, then that many
/// 8-bit characters.
fn read_tv8(c: &mut BitCursor<'_>) -> Result<String> {
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

/// A cursor parked on absolute bit `bit` of `bytes`.
fn cursor_at(bytes: &[u8], bit: usize) -> Option<BitCursor<'_>> {
    let mut c = BitCursor::new(bytes.get(bit / 8..)?);
    for _ in 0..(bit % 8) {
        c.read_b().ok()?;
    }
    Some(c)
}

/// Read a 16-bit value at `bit` with the `RS` byte order the string-
/// stream trailer uses (low byte first).
fn word_at(bytes: &[u8], bit: usize) -> Option<u16> {
    Some(cursor_at(bytes, bit)?.read_rs().ok()? as u16)
}

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: probe_class_layout <file.dwg>");
    let file = DwgFile::open(&path)?;
    let version = file.version();
    let Some(bytes) = file.read_section("AcDb:Classes") else {
        println!("{path} ({version}) has no named AcDb:Classes section");
        return Ok(());
    };
    let bytes = bytes?;

    let bit_start = if version.is_r2007_plus() { 28 } else { 20 };
    let size_bytes = u32::from_le_bytes(bytes[16..20].try_into().expect("4 bytes")) as usize;
    let size_bits = if version.is_r2007_plus() {
        u32::from_le_bytes(bytes[24..28].try_into().expect("4 bytes")) as usize
    } else {
        size_bytes * 8
    };
    println!("{path}  ({version})");
    println!(
        "section {} bytes, class data area {size_bytes} bytes / {size_bits} bits, \
         bit stream starts at byte {bit_start}",
        bytes.len()
    );

    let data = &bytes[bit_start..];
    let mut c = BitCursor::new(data);
    let max_class_number = c.read_bl_u()?;
    let flag = c.read_b()?;
    println!(
        "BL max_class_number = {max_class_number}, B = {flag} (header is {} bits)",
        c.position_bits()
    );

    let expected = (max_class_number as usize).saturating_sub(500) + 1;
    println!();
    println!(
        "{:>4} {:>7} {:>7} {:>5}  {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "cls", "start", "end", "bits", "ver", "proxy", "icid", "num", "dwgv", "maint"
    );
    let split = version.is_r2007_plus();
    let mut widths = Vec::new();
    let mut inline_names = Vec::new();
    for i in 0..expected {
        let start = c.position_bits();
        let class_number = c.read_bs_u()?;
        let version_flag = c.read_bs()?;
        if !split {
            // Pre-R2007 the three names sit inline, 8-bit, right here.
            inline_names.push((read_tv8(&mut c)?, read_tv8(&mut c)?, read_tv8(&mut c)?));
        }
        let was_a_proxy = c.read_b()?;
        let item_class_id = c.read_bs_u()?;
        let num_objects = c.read_bl_u()?;
        let dwg_version = c.read_bl_u()?;
        let maintenance_version = c.read_bl_u()?;
        let _unknown1 = c.read_bl()?;
        let _unknown2 = c.read_bl()?;
        let end = c.position_bits();
        widths.push(end - start);
        let flag = if class_number as usize == 500 + i {
            " "
        } else {
            "  <-- NOT CONSECUTIVE"
        };
        println!(
            "{class_number:>4} {start:>7} {end:>7} {:>5}  {version_flag:>6} {:>6} 0x{item_class_id:04X} \
             {num_objects:>6} {dwg_version:>6} {maintenance_version:>6}{flag}",
            end - start,
            u8::from(was_a_proxy),
        );
    }
    let records_end = c.position_bits();
    println!();
    println!(
        "{expected} records, {records_end} bits, widths {}..{}",
        widths.iter().min().copied().unwrap_or(0),
        widths.iter().max().copied().unwrap_or(0)
    );

    if !split {
        println!("pre-R2007: names are inline in each record, no string block");
        println!();
        for (i, (app, cpp, dxf)) in inline_names.iter().enumerate() {
            println!("{:>4}  {app:<24} {cpp:<38} {dxf}", 500 + i);
        }
        println!();
        println!(
            "class data area is {} bits; the records consumed {records_end}, \
             leaving {} bits of byte padding",
            size_bytes * 8,
            size_bytes * 8 - records_end
        );
        return Ok(());
    }

    println!();
    for i in 0..expected {
        let app = read_tu(&mut c)?;
        let cpp = read_tu(&mut c)?;
        let dxf = read_tu(&mut c)?;
        println!("{:>4}  {app:<24} {cpp:<38} {dxf}", 500 + i);
    }
    let strings_end = c.position_bits();

    // §19.4.1 string-stream trailer, measured to sit at
    // `size_bits - 32` relative to the start of the bit stream.
    let end = size_bits.saturating_sub(32);
    let present = match cursor_at(data, end.saturating_sub(1)) {
        Some(mut term) => term.read_b().unwrap_or(false),
        None => false,
    };
    let w1 = word_at(data, end.saturating_sub(17)).unwrap_or(0);
    let str_len = if w1 & 0x8000 != 0 {
        let w2 = word_at(data, end.saturating_sub(33)).unwrap_or(0);
        (u32::from(w1 & 0x7FFF)) | (u32::from(w2) << 15)
    } else {
        u32::from(w1)
    };
    println!();
    println!("string block read forward: {records_end}..{strings_end}");
    println!(
        "string-stream trailer at {end} (= size_bits - 32): present = {present}, \
         length word = {str_len}"
    );
    println!(
        "trailer agrees with the forward read: {}",
        u32::try_from(strings_end - records_end) == Ok(str_len)
    );
    Ok(())
}
