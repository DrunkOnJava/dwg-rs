//! A WebAssembly kernel that restores a checkpoint made by a native build and
//! finishes the walk (or makes a checkpoint a native build restores), built
//! from the identical grammar with neither std nor an allocator. A host puts
//! the source bytes, the checkpoint and the caller's cursors in the module's
//! scratch memory, calls one export and reads back the JSON lines the native
//! examples print for the same steps. Not part of the library:
//! `tools/streaming/verify_wasm.py` builds it and drives it from node.
#![no_std]

use core::fmt::Write;

#[path = "../../src/crc.rs"]
pub mod crc;
#[path = "../../src/streaming/mod.rs"]
pub mod streaming;

use streaming::{
    MapDemand, MapStep, ObjectDemand, ObjectStep, PageEntry, R2004MapWalker, R2004ObjectWalker,
    R2004SectionWalker, R2004Walker, R2007Demand, R2007ObjectDemand, R2007ObjectStep,
    R2007ObjectWalker, R2007Step, R2007Walker, SectionDemand, SectionPage, SectionStep,
    SourceIdentity, StreamError, SystemKind, WalkDemand, WalkStep,
};

const FILE_CAP: usize = 0x30_0000;
const CKPT: usize = 0x30_0000;
const CKPT_CAP: usize = 0x2_8000;
const DIGEST: usize = 0x32_8000;
const TABLE: usize = 0x32_9000;
const TABLE_CAP: usize = 0xd_7000;
const OUT: usize = 0x40_0000;
const OUT_CAP: usize = 0x10_0000;
const TOTAL: usize = OUT + OUT_CAP;
const ROW: usize = 32;
const NAME: usize = 64;

static mut SCRATCH: [u8; TOTAL] = [0; TOTAL];
static mut CKPT_LEN: usize = 0;
static mut CURSORS: [u64; 5] = [0; 5];

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

/// The scratch memory split once into disjoint regions, so no two live
/// references overlap. Each export takes it once; the module is
/// single-threaded and exports do not re-enter.
struct Scratch {
    file: &'static mut [u8],
    ckpt: &'static mut [u8],
    digest: &'static mut [u8],
    table: &'static mut [u8],
    out: &'static mut [u8],
}

fn scratch() -> Scratch {
    let all: &'static mut [u8; TOTAL] = unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) };
    let (file, rest) = all.split_at_mut(FILE_CAP);
    let (ckpt, rest) = rest.split_at_mut(CKPT_CAP);
    let (digest, rest) = rest.split_at_mut(TABLE - DIGEST);
    let (table, out) = rest.split_at_mut(TABLE_CAP);
    Scratch {
        file,
        ckpt,
        digest,
        table,
        out,
    }
}

struct Lines<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl Write for Lines<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.at + s.len();
        if end > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.at..end].copy_from_slice(s.as_bytes());
        self.at = end;
        Ok(())
    }
}

/// Where the host puts the source, checkpoint, digest and table and reads
/// the output: 0 the scratch base, then (offset, capacity) pairs; 9 the length
/// of the checkpoint the last `run_maps`, `run_file`, `run_objects` or
/// `run_r2007` wrote; 10, 11 and 12 the page CRC-32, page length and page
/// number the last `run_file` ended on (for `run_objects`, the payload CRC-32
/// and length; for `run_r2007`, the page CRC-32, page length and section
/// index). The page id of the last `run_r2007` is `r2007_page_id`.
#[unsafe(no_mangle)]
pub extern "C" fn layout(which: u32) -> u32 {
    match which {
        0 => core::ptr::addr_of_mut!(SCRATCH) as u32,
        1 => FILE_CAP as u32,
        2 => CKPT as u32,
        3 => CKPT_CAP as u32,
        4 => DIGEST as u32,
        5 => TABLE as u32,
        6 => TABLE_CAP as u32,
        7 => OUT as u32,
        8 => OUT_CAP as u32,
        9 => unsafe { CKPT_LEN as u32 },
        10 => unsafe { CURSORS[0] as u32 },
        11 => unsafe { CURSORS[1] as u32 },
        12 => unsafe { CURSORS[2] as u32 },
        _ => 0,
    }
}

fn identity(file_len: usize, digest: &[u8]) -> SourceIdentity {
    let mut copy = [0u8; 32];
    copy.copy_from_slice(&digest[..32]);
    SourceIdentity {
        digest: copy,
        length: file_len as u64,
    }
}

/// A name as the body of a JSON string: the map walker admits only printable
/// ASCII, so the quote and the backslash are all that need escaping.
struct Json<'a>(&'a str);

impl core::fmt::Display for Json<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for c in self.0.chars() {
            match c {
                '"' => f.write_str("\\\"")?,
                '\\' => f.write_str("\\\\")?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

fn name(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end]).unwrap_or("")
}

fn u32_le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_le(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_le(bytes, at)) | u64::from(u32_le(bytes, at + 4)) << 32
}

fn source(file: &[u8], offset: u64, n: usize, file_len: usize) -> Result<&[u8], StreamError> {
    let at = offset as usize;
    if at + n > file_len {
        return Err(StreamError::Truncated { offset, needed: n });
    }
    Ok(&file[at..at + n])
}

fn map_line(out: &mut Lines<'_>, step: MapStep) -> core::fmt::Result {
    match step {
        MapStep::Framing => Ok(()),
        MapStep::Header(h) => writeln!(
            out,
            "{{\"header\":\"{}\",\"page_map_id\":{},\"page_map_address\":{},\"section_map_id\":{}}}",
            core::str::from_utf8(&h.magic).unwrap_or(""),
            h.page_map_id,
            h.page_map_address,
            h.section_map_id
        ),
        MapStep::SystemPage {
            kind,
            address,
            compressed,
            decompressed,
        } => writeln!(
            out,
            "{{\"system_page\":\"{}\",\"address\":{address},\"compressed\":{compressed},\"decompressed\":{decompressed}}}",
            match kind {
                SystemKind::PageMap => "page_map",
                SystemKind::SectionMap => "section_map",
            }
        ),
        MapStep::Page(p) => writeln!(
            out,
            "{{\"page\":{},\"size\":{},\"address\":{}}}",
            p.number, p.size, p.address
        ),
        MapStep::Sections(n) => writeln!(out, "{{\"sections\":{n}}}"),
        MapStep::Section(s) => writeln!(
            out,
            "{{\"section\":{},\"name\":\"{}\",\"size\":{},\"page_count\":{},\"max_page_size\":{},\"compressed\":{},\"section_id\":{},\"encrypted\":{}}}",
            s.index,
            Json(name(&s.name)),
            s.size,
            s.page_count,
            s.max_page_size,
            s.compressed,
            s.section_id,
            s.encrypted
        ),
        MapStep::SectionPage(r) => writeln!(
            out,
            "{{\"section_page\":{},\"section_id\":{},\"data_size\":{},\"start_offset\":{}}}",
            r.page_number, r.section_id, r.data_size, r.start_offset
        ),
        MapStep::Finished => writeln!(out, "{{\"finished\":true}}"),
    }
}

/// Walk the page map and section map from `walker`, `chunk` bytes at most per
/// read, printing each committed step, until the walk ends or `stop` steps
/// (feeds and pumps, as the native example counts them) have run.
fn walk_maps(
    walker: &mut R2004MapWalker,
    file: &[u8],
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines<'_>,
) -> i32 {
    let mut steps = 0u32;
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(MapDemand::Done) => break,
            Ok(MapDemand::Pump) => walker.pump(),
            Ok(MapDemand::Read(r)) => match source(file, r.offset, r.len.min(chunk).min(256), file_len) {
                Ok(bytes) => walker.feed(r.offset, bytes),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        steps += 1;
        if map_line(out, step).is_err() {
            return -3;
        }
    }
    0
}

/// Restore a `DWGM` checkpoint and finish the page map and section map walk.
/// Returns the output length, or a negative code: -1 restore refused, -2 the
/// walk failed, -3 output full.
#[unsafe(no_mangle)]
pub extern "C" fn resume_maps(file_len: u32, ckpt_len: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2004MapWalker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let mut out = Lines { buf: mem.out, at: 0 };
    match walk_maps(&mut walker, mem.file, file_len, chunk as usize, None, &mut out) {
        0 => out.at as i32,
        code => code,
    }
}

/// Walk the page map and section map from the start for `stop` steps, print
/// what they committed and leave the walker's checkpoint at the checkpoint
/// offset (its length is `layout(9)`). Same codes as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_maps(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let mut walker = R2004MapWalker::new(identity(file_len, mem.digest));
    let mut out = Lines { buf: mem.out, at: 0 };
    match walk_maps(&mut walker, mem.file, file_len, chunk as usize, Some(stop), &mut out) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    mem.ckpt[..bytes.len()].copy_from_slice(bytes);
    unsafe { CKPT_LEN = bytes.len() };
    out.at as i32
}

/// Restore a `DWGS` checkpoint and finish that section. The table holds the
/// section's 64-byte name, then one 32-byte row per page of the section: the
/// page map entry (number, size, address) and the section map reference
/// (page number, data size, start offset). `next_ref` is how many rows the
/// walk had been given; the three cursors are the caller's, as the native
/// example saves them. Same codes as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn resume_section(
    file_len: u32,
    ckpt_len: u32,
    chunk: u32,
    rows: u32,
    next_ref: u32,
    page_crc: u32,
    page_len: u32,
    section_crc: u32,
) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2004SectionWalker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let table: &[u8] = mem.table;
    let label = name(&table[..NAME]);
    let id = walker.section_id();
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut next = next_ref as usize;
    let (mut page_crc, mut page_len, mut section_crc) = (page_crc, u64::from(page_len), section_crc);
    let mut number = if next > 0 {
        u32_le(table, NAME + (next - 1) * ROW + 16)
    } else {
        0
    };
    let chunk = (chunk as usize).clamp(1, 256);
    loop {
        let step = match walker.demand() {
            Ok(SectionDemand::Done) => break,
            Ok(SectionDemand::NextPage) => {
                if next >= rows as usize {
                    return -2;
                }
                let row = &table[NAME + next * ROW..NAME + (next + 1) * ROW];
                let page = PageEntry {
                    number: u32_le(row, 0) as i32,
                    size: u32_le(row, 4),
                    address: u64_le(row, 8),
                };
                let reference = SectionPage {
                    section_id: id,
                    page_number: u32_le(row, 16),
                    data_size: u32_le(row, 20),
                    start_offset: u64_le(row, 24),
                };
                if walker.begin_page(page, reference).is_err() {
                    return -2;
                }
                number = reference.page_number;
                next += 1;
                continue;
            }
            Ok(SectionDemand::Pump) => walker.pump(),
            Ok(SectionDemand::Read(r)) => match source(mem.file, r.offset, r.len.min(chunk), file_len) {
                Ok(bytes) => walker.feed(r.offset, bytes),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        let wrote = match step {
            SectionStep::Framing => Ok(()),
            SectionStep::Page(p) => {
                page_crc = 0;
                page_len = 0;
                writeln!(
                    out,
                    "{{\"page\":{{\"section\":{},\"number\":{},\"address\":{},\"start\":{},\"data\":{},\"page_size\":{}}}}}",
                    p.section_id, p.page_number, p.address, p.start_offset, p.data_size, p.page_size
                )
            }
            SectionStep::Hole { offset, len } => {
                let zeros = [0u8; 64];
                let mut left = len;
                while left > 0 {
                    let n = left.min(64) as usize;
                    section_crc = crc::crc32(section_crc, &zeros[..n]);
                    left -= n as u64;
                }
                writeln!(
                    out,
                    "{{\"hole\":{{\"section\":{id},\"offset\":{offset},\"len\":{len}}}}}"
                )
            }
            SectionStep::Data { len, .. } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return -2;
                }
                page_crc = crc::crc32(page_crc, bytes);
                page_len += len as u64;
                section_crc = crc::crc32(section_crc, bytes);
                Ok(())
            }
            SectionStep::PageEnd {
                decompressed,
                emitted,
            } => {
                if u64::from(emitted) != page_len {
                    return -2;
                }
                writeln!(
                    out,
                    "{{\"page_end\":{{\"section\":{id},\"number\":{number},\"decompressed\":{decompressed},\"emitted\":{emitted},\"crc32\":{page_crc}}}}}"
                )
            }
            SectionStep::Finished { size, pages } => writeln!(
                out,
                "{{\"section_done\":{{\"index\":{},\"name\":\"{}\",\"size\":{size},\"pages\":{pages},\"crc32\":{section_crc}}}}}",
                walker.section_index(),
                Json(label)
            ),
        };
        if wrote.is_err() {
            return -3;
        }
    }
    out.at as i32
}

/// Walk the whole file from `walker`, printing what the native `stream_r2004_file`
/// prints for the same steps (no section files, so no `section_done` lines),
/// until the walk ends or `stop` steps have run. The cursors are the page
/// CRC-32, page length and page number.
fn walk_file(
    walker: &mut R2004Walker,
    file: &[u8],
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines<'_>,
    cursors: &mut (u32, u64, u32),
) -> i32 {
    let mut steps = 0u32;
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(WalkDemand::Done) => break,
            Ok(WalkDemand::Pump) => walker.pump(),
            Ok(WalkDemand::Read(r)) => match source(file, r.offset, r.len.min(chunk).min(256), file_len) {
                Ok(bytes) => walker.feed(r.offset, bytes),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        steps += 1;
        let wrote = match step {
            WalkStep::Framing | WalkStep::Header(_) => Ok(()),
            WalkStep::Section(s) => writeln!(
                out,
                "{{\"section\":{{\"index\":{},\"name\":\"{}\",\"size\":{},\"pages\":{},\"id\":{}}}}}",
                s.index,
                Json(name(&s.name)),
                s.size,
                s.page_count,
                s.section_id
            ),
            WalkStep::Page(p) => {
                *cursors = (0, 0, p.page_number);
                writeln!(
                    out,
                    "{{\"page\":{{\"section\":{},\"number\":{},\"address\":{},\"start\":{},\"data\":{},\"page_size\":{}}}}}",
                    p.section_id, p.page_number, p.address, p.start_offset, p.data_size, p.page_size
                )
            }
            WalkStep::Data { len, .. } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return -2;
                }
                cursors.0 = crc::crc32(cursors.0, bytes);
                cursors.1 += len as u64;
                Ok(())
            }
            WalkStep::PageEnd {
                decompressed,
                emitted,
            } => {
                if u64::from(emitted) != cursors.1 {
                    return -2;
                }
                writeln!(
                    out,
                    "{{\"page_end\":{{\"number\":{},\"decompressed\":{decompressed},\"emitted\":{emitted},\"crc32\":{}}}}}",
                    cursors.2, cursors.0
                )
            }
            WalkStep::Finished { sections, pages } => {
                writeln!(out, "{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}")
            }
        };
        if wrote.is_err() {
            return -3;
        }
    }
    0
}

/// Restore a `DWGF` checkpoint, with the page CRC-32, length and number the
/// native caller saved, and finish the whole walk. The checkpoint alone
/// carries the section table: no page index is passed. Same codes as
/// `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn resume_file(
    file_len: u32,
    ckpt_len: u32,
    chunk: u32,
    page_crc: u32,
    page_len: u32,
    number: u32,
) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2004Walker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (page_crc, u64::from(page_len), number);
    match walk_file(&mut walker, mem.file, file_len, chunk as usize, None, &mut out, &mut cursors) {
        0 => out.at as i32,
        code => code,
    }
}

/// Walk the whole file from the start for `stop` steps, print what it
/// committed and leave the walker's checkpoint at the checkpoint offset (its
/// length is `layout(9)`, the cursors `layout(10..=12)`). Same codes as
/// `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_file(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let mut walker = R2004Walker::new(identity(file_len, mem.digest));
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (0u32, 0u64, 0u32);
    match walk_file(&mut walker, mem.file, file_len, chunk as usize, Some(stop), &mut out, &mut cursors) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    if bytes.len() > mem.ckpt.len() {
        return -3;
    }
    mem.ckpt[..bytes.len()].copy_from_slice(bytes);
    unsafe {
        CKPT_LEN = bytes.len();
        CURSORS = [u64::from(cursors.0), cursors.1, u64::from(cursors.2), 0, 0];
    }
    out.at as i32
}

/// One R2007 read or gather of at most `chunk` bytes into `buf`; the bytes
/// gathered are at `offset`, `offset + stride`, ...
fn r2007_bytes<'a>(
    file: &[u8],
    file_len: usize,
    offset: u64,
    stride: u64,
    want: usize,
    chunk: usize,
    buf: &'a mut [u8; 256],
) -> Result<&'a [u8], StreamError> {
    let n = want.min(chunk).min(256);
    for (k, slot) in buf[..n].iter_mut().enumerate() {
        let at = offset + k as u64 * stride;
        if at as usize >= file_len {
            return Err(StreamError::Truncated { offset: at, needed: 1 });
        }
        *slot = file[at as usize];
    }
    Ok(&buf[..n])
}

struct Units<'a>(&'a streaming::R2007Section);

impl core::fmt::Display for Units<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for c in char::decode_utf16(self.0.name_units()) {
            match c.unwrap_or('\u{fffd}') {
                '"' => f.write_str("\\\"")?,
                '\\' => f.write_str("\\\\")?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

/// The caller's cursors over the output of an R2007 walk: the page CRC-32 and
/// length, the section index and the page id.
struct R2007Cursor {
    crc: u32,
    len: u64,
    section: u32,
    id: i64,
}

/// Walk the whole R2007 file from `walker`, printing what the native
/// `stream_r2007` prints for the same steps (no section files, so no
/// `section_done` lines), until the walk ends or `stop` steps have run.
fn walk_r2007(
    walker: &mut R2007Walker,
    file: &[u8],
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines<'_>,
    cursor: &mut R2007Cursor,
) -> i32 {
    let mut steps = 0u32;
    let mut buf = [0u8; 256];
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(R2007Demand::Done) => break,
            Ok(R2007Demand::Pump) => walker.pump(),
            Ok(R2007Demand::Read(r)) => {
                match r2007_bytes(file, file_len, r.offset, 1, r.len, chunk, &mut buf) {
                    Ok(bytes) => walker.feed(r.offset, bytes),
                    Err(e) => Err(e),
                }
            }
            Ok(R2007Demand::Gather(g)) => {
                match r2007_bytes(file, file_len, g.offset, g.stride, g.len, chunk, &mut buf) {
                    Ok(bytes) => walker.feed(g.offset, bytes),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        steps += 1;
        let wrote = match step {
            R2007Step::Framing => Ok(()),
            R2007Step::Header(h) => {
                let mut r = write!(out, "{{\"header\":[");
                for (k, v) in h.fields.iter().enumerate() {
                    r = r.and_then(|()| write!(out, "{}{v}", if k == 0 { "" } else { "," }));
                }
                r.and_then(|()| writeln!(out, "]}}"))
            }
            R2007Step::Entry(e) => writeln!(
                out,
                "{{\"page_entry\":{{\"id\":{},\"size\":{},\"offset\":{}}}}}",
                e.id, e.size, e.offset
            ),
            R2007Step::Section(s) => {
                cursor.section = s.index;
                writeln!(
                    out,
                    "{{\"section\":{{\"index\":{},\"name\":\"{}\",\"size\":{},\"max\":{},\"encryption\":{},\"hash\":{},\"encoding\":{},\"pages\":{}}}}}",
                    s.index,
                    Units(&s),
                    s.size,
                    s.max_size,
                    s.encryption,
                    s.hash_code,
                    s.encoding,
                    s.page_count
                )
            }
            R2007Step::Page(p) => {
                cursor.crc = 0;
                cursor.len = 0;
                cursor.id = p.id;
                writeln!(
                    out,
                    "{{\"page\":{{\"section\":{},\"id\":{},\"address\":{},\"data_offset\":{},\"size\":{},\"uncompressed\":{},\"compressed\":{},\"checksum\":{},\"crc\":{},\"codewords\":{}}}}}",
                    p.section,
                    p.id,
                    p.address,
                    p.data_offset,
                    p.size,
                    p.uncompressed,
                    p.compressed,
                    p.checksum,
                    p.crc,
                    p.codewords
                )
            }
            R2007Step::Hole { offset, len } => writeln!(
                out,
                "{{\"hole\":{{\"section\":{},\"offset\":{offset},\"len\":{len}}}}}",
                cursor.section
            ),
            R2007Step::Data { len, .. } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return -2;
                }
                cursor.crc = crc::crc32(cursor.crc, bytes);
                cursor.len += len as u64;
                Ok(())
            }
            R2007Step::PageEnd { plain, emitted } => {
                if emitted != cursor.len {
                    return -2;
                }
                writeln!(
                    out,
                    "{{\"page_end\":{{\"section\":{},\"id\":{},\"plain\":{plain},\"emitted\":{emitted},\"crc32\":{}}}}}",
                    cursor.section, cursor.id, cursor.crc
                )
            }
            R2007Step::SectionEnd { index, covered } => writeln!(
                out,
                "{{\"section_end\":{{\"index\":{index},\"covered\":{covered}}}}}"
            ),
            R2007Step::Finished { sections, pages } => {
                writeln!(out, "{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}")
            }
        };
        if wrote.is_err() {
            return -3;
        }
    }
    0
}

/// The signed 64-bit id of the page the last `run_r2007` stopped in.
#[unsafe(no_mangle)]
pub extern "C" fn r2007_page_id() -> i64 {
    unsafe { CURSORS[3] as i64 }
}

/// Restore a `DWGR` checkpoint, with the page CRC-32, length, section index
/// and page id the native caller saved, and finish the whole walk. Same codes
/// as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn resume_r2007(
    file_len: u32,
    ckpt_len: u32,
    chunk: u32,
    page_crc: u32,
    page_len: u32,
    section: u32,
    id: i64,
) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2007Walker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursor = R2007Cursor {
        crc: page_crc,
        len: u64::from(page_len),
        section,
        id,
    };
    match walk_r2007(&mut walker, mem.file, file_len, chunk as usize, None, &mut out, &mut cursor) {
        0 => out.at as i32,
        code => code,
    }
}

/// Walk the whole R2007 file from the start for `stop` steps, print what it
/// committed and leave the walker's checkpoint at the checkpoint offset (its
/// length is `layout(9)`, the cursors `layout(10..=12)` and `r2007_page_id`).
/// Same codes as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_r2007(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let mut walker = R2007Walker::new(identity(file_len, mem.digest));
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursor = R2007Cursor {
        crc: 0,
        len: 0,
        section: 0,
        id: 0,
    };
    match walk_r2007(&mut walker, mem.file, file_len, chunk as usize, Some(stop), &mut out, &mut cursor) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    if bytes.len() > mem.ckpt.len() {
        return -3;
    }
    mem.ckpt[..bytes.len()].copy_from_slice(bytes);
    unsafe {
        CKPT_LEN = bytes.len();
        CURSORS = [
            u64::from(cursor.crc),
            cursor.len,
            u64::from(cursor.section),
            cursor.id as u64,
            0,
        ];
    }
    out.at as i32
}

/// Walk the whole file with its object section framed, printing what the native
/// `stream_r2004_objects` prints for the same steps, until the walk ends or
/// `stop` steps have run. The cursors are the CRC-32 and length of the open
/// record's payload so far.
fn walk_objects(
    walker: &mut R2004ObjectWalker,
    file: &[u8],
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines<'_>,
    cursors: &mut (u32, u64),
) -> i32 {
    let mut steps = 0u32;
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(ObjectDemand::Done) => break,
            Ok(ObjectDemand::Pump) => walker.pump(),
            Ok(ObjectDemand::Read(r)) => match source(file, r.offset, r.len.min(chunk).min(256), file_len) {
                Ok(bytes) => walker.feed(r.offset, bytes),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        steps += 1;
        let wrote = match step {
            ObjectStep::Walk(WalkStep::Finished { sections, pages }) => {
                writeln!(out, "{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}")
            }
            ObjectStep::Walk(_) => Ok(()),
            ObjectStep::Payload { len } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return -2;
                }
                cursors.0 = crc::crc32(cursors.0, bytes);
                cursors.1 += len as u64;
                Ok(())
            }
            ObjectStep::Object(r) => {
                let wrote = writeln!(
                    out,
                    "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"len\":{},\"crc32\":{}}}",
                    r.handle, r.offset, r.size, r.type_code, cursors.1, cursors.0
                );
                *cursors = (0, 0);
                wrote
            }
            ObjectStep::Objects { records, bytes } => {
                writeln!(out, "{{\"objects\":{{\"records\":{records},\"bytes\":{bytes}}}}}")
            }
        };
        if wrote.is_err() {
            return -3;
        }
    }
    0
}

/// Restore a `DWGO` checkpoint, with the payload CRC-32 and length the native
/// caller saved, and finish the walk. The checkpoint alone carries the section
/// table, the object section and the open record. Same codes as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn resume_objects(file_len: u32, ckpt_len: u32, chunk: u32, digest: u32, len: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2004ObjectWalker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (digest, u64::from(len));
    match walk_objects(&mut walker, mem.file, file_len, chunk as usize, None, &mut out, &mut cursors) {
        0 => out.at as i32,
        code => code,
    }
}

/// Walk from the start for `stop` steps, print what it committed and leave the
/// walker's checkpoint at the checkpoint offset (its length is `layout(9)`, the
/// payload CRC-32 and length `layout(10)` and `layout(11)`). Same codes as
/// `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_objects(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let mut walker = R2004ObjectWalker::new(identity(file_len, mem.digest));
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (0u32, 0u64);
    match walk_objects(&mut walker, mem.file, file_len, chunk as usize, Some(stop), &mut out, &mut cursors) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    if bytes.len() > mem.ckpt.len() {
        return -3;
    }
    mem.ckpt[..bytes.len()].copy_from_slice(bytes);
    unsafe {
        CKPT_LEN = bytes.len();
        CURSORS = [u64::from(cursors.0), cursors.1, 0, 0, 0];
    }
    out.at as i32
}

/// Walk the whole R2007 file with its object section framed, printing what the
/// native `stream_r2007_objects` prints for the same steps, until the walk ends
/// or `stop` steps have run. The cursors are the CRC-32 and length of the open
/// record's payload so far.
fn walk_r2007_objects(
    walker: &mut R2007ObjectWalker,
    file: &[u8],
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines<'_>,
    cursors: &mut (u32, u64),
) -> i32 {
    let mut steps = 0u32;
    let mut buf = [0u8; 256];
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(R2007ObjectDemand::Done) => break,
            Ok(R2007ObjectDemand::Pump) => walker.pump(),
            Ok(R2007ObjectDemand::Read(r)) => {
                match r2007_bytes(file, file_len, r.offset, 1, r.len, chunk, &mut buf) {
                    Ok(bytes) => walker.feed(r.offset, bytes),
                    Err(e) => Err(e),
                }
            }
            Ok(R2007ObjectDemand::Gather(g)) => {
                match r2007_bytes(file, file_len, g.offset, g.stride, g.len, chunk, &mut buf) {
                    Ok(bytes) => walker.feed(g.offset, bytes),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        let Ok(step) = step else { return -2 };
        steps += 1;
        let wrote = match step {
            R2007ObjectStep::Walk(R2007Step::Finished { sections, pages }) => {
                writeln!(out, "{{\"finished\":{{\"sections\":{sections},\"pages\":{pages}}}}}")
            }
            R2007ObjectStep::Walk(_) => Ok(()),
            R2007ObjectStep::Payload { len } => {
                let bytes = walker.data();
                if bytes.len() != len {
                    return -2;
                }
                cursors.0 = crc::crc32(cursors.0, bytes);
                cursors.1 += len as u64;
                Ok(())
            }
            R2007ObjectStep::Object(r) => {
                let wrote = writeln!(
                    out,
                    "{{\"object\":{},\"offset\":{},\"size\":{},\"type\":{},\"len\":{},\"crc32\":{}}}",
                    r.handle, r.offset, r.size, r.type_code, cursors.1, cursors.0
                );
                *cursors = (0, 0);
                wrote
            }
            R2007ObjectStep::Objects { records, bytes } => {
                writeln!(out, "{{\"objects\":{{\"records\":{records},\"bytes\":{bytes}}}}}")
            }
        };
        if wrote.is_err() {
            return -3;
        }
    }
    0
}

/// Restore a `DWGQ` checkpoint, with the payload CRC-32 and length the native
/// caller saved, and finish the walk. The checkpoint alone carries the header,
/// the readers, the object section and the open record. Same codes as
/// `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn resume_r2007_objects(
    file_len: u32,
    ckpt_len: u32,
    chunk: u32,
    digest: u32,
    len: u32,
) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let Ok(mut walker) =
        R2007ObjectWalker::restore(&mem.ckpt[..ckpt_len as usize], identity(file_len, mem.digest))
    else {
        return -1;
    };
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (digest, u64::from(len));
    match walk_r2007_objects(&mut walker, mem.file, file_len, chunk as usize, None, &mut out, &mut cursors) {
        0 => out.at as i32,
        code => code,
    }
}

/// Walk from the start for `stop` steps, print what it committed and leave the
/// walker's checkpoint at the checkpoint offset (its length is `layout(9)`, the
/// payload CRC-32 and length `layout(10)` and `layout(11)`). Same codes as
/// `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_r2007_objects(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mem = scratch();
    let mut walker = R2007ObjectWalker::new(identity(file_len, mem.digest));
    let mut out = Lines { buf: mem.out, at: 0 };
    let mut cursors = (0u32, 0u64);
    match walk_r2007_objects(&mut walker, mem.file, file_len, chunk as usize, Some(stop), &mut out, &mut cursors) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    if bytes.len() > mem.ckpt.len() {
        return -3;
    }
    mem.ckpt[..bytes.len()].copy_from_slice(bytes);
    unsafe {
        CKPT_LEN = bytes.len();
        CURSORS = [u64::from(cursors.0), cursors.1, 0, 0, 0];
    }
    out.at as i32
}
