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
    MapDemand, MapStep, PageEntry, R2004MapWalker, R2004SectionWalker, SectionDemand, SectionPage,
    SectionStep, SourceIdentity, StreamError, SystemKind,
};

const FILE_CAP: usize = 0x30_0000;
const CKPT: usize = 0x30_0000;
const CKPT_CAP: usize = 0x1_0000;
const DIGEST: usize = 0x31_0000;
const TABLE: usize = 0x31_1000;
const TABLE_CAP: usize = 0xf_f000;
const OUT: usize = 0x40_0000;
const OUT_CAP: usize = 0x10_0000;
const TOTAL: usize = OUT + OUT_CAP;
const ROW: usize = 32;
const NAME: usize = 64;

static mut SCRATCH: [u8; TOTAL] = [0; TOTAL];
static mut CKPT_LEN: usize = 0;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

fn memory() -> &'static mut [u8; TOTAL] {
    unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) }
}

struct Lines {
    at: usize,
}

impl Write for Lines {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.at + s.len();
        if end > OUT + OUT_CAP {
            return Err(core::fmt::Error);
        }
        memory()[self.at..end].copy_from_slice(s.as_bytes());
        self.at = end;
        Ok(())
    }
}

/// Where the host puts the source, checkpoint, digest and table and reads
/// the output: 0 the scratch base, then (offset, capacity) pairs; 9 the length
/// of the checkpoint the last `run_maps` wrote.
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
        _ => 0,
    }
}

fn identity(file_len: usize) -> SourceIdentity {
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&memory()[DIGEST..DIGEST + 32]);
    SourceIdentity {
        digest,
        length: file_len as u64,
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

fn source(offset: u64, n: usize, file_len: usize) -> Result<&'static [u8], StreamError> {
    let at = offset as usize;
    if at + n > file_len {
        return Err(StreamError::Truncated { offset, needed: n });
    }
    Ok(&memory()[at..at + n])
}

fn map_line(out: &mut Lines, step: MapStep) -> core::fmt::Result {
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
            name(&s.name),
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
    file_len: usize,
    chunk: usize,
    stop: Option<u32>,
    out: &mut Lines,
) -> i32 {
    let mut steps = 0u32;
    loop {
        if stop == Some(steps) {
            break;
        }
        let step = match walker.demand() {
            Ok(MapDemand::Done) => break,
            Ok(MapDemand::Pump) => walker.pump(),
            Ok(MapDemand::Read(r)) => {
                let n = r.len.min(chunk).min(256);
                match source(r.offset, n, file_len) {
                    Ok(bytes) => walker.feed(r.offset, bytes),
                    Err(e) => Err(e),
                }
            }
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
    let ckpt = &memory()[CKPT..CKPT + ckpt_len as usize];
    let Ok(mut walker) = R2004MapWalker::restore(ckpt, identity(file_len)) else {
        return -1;
    };
    let mut out = Lines { at: OUT };
    match walk_maps(&mut walker, file_len, chunk as usize, None, &mut out) {
        0 => (out.at - OUT) as i32,
        code => code,
    }
}

/// Walk the page map and section map from the start for `stop` steps, print
/// what they committed and leave the walker's checkpoint at the checkpoint
/// offset (its length is `layout(9)`). Same codes as `resume_maps`.
#[unsafe(no_mangle)]
pub extern "C" fn run_maps(file_len: u32, stop: u32, chunk: u32) -> i32 {
    let file_len = file_len as usize;
    let mut walker = R2004MapWalker::new(identity(file_len));
    let mut out = Lines { at: OUT };
    match walk_maps(&mut walker, file_len, chunk as usize, Some(stop), &mut out) {
        0 => (),
        code => return code,
    }
    let Ok(checkpoint) = walker.checkpoint() else {
        return -2;
    };
    let bytes = checkpoint.as_bytes();
    memory()[CKPT..CKPT + bytes.len()].copy_from_slice(bytes);
    unsafe { CKPT_LEN = bytes.len() };
    (out.at - OUT) as i32
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
    let mem = memory();
    let ckpt = &mem[CKPT..CKPT + ckpt_len as usize];
    let Ok(mut walker) = R2004SectionWalker::restore(ckpt, identity(file_len)) else {
        return -1;
    };
    let table = &mem[TABLE..TABLE + TABLE_CAP];
    let label = name(&table[..NAME]);
    let id = walker.section_id();
    let mut out = Lines { at: OUT };
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
            Ok(SectionDemand::Read(r)) => match source(r.offset, r.len.min(chunk), file_len) {
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
                "{{\"section_done\":{{\"index\":{},\"name\":\"{label}\",\"size\":{size},\"pages\":{pages},\"crc32\":{section_crc}}}}}",
                walker.section_index()
            ),
        };
        if wrote.is_err() {
            return -3;
        }
    }
    (out.at - OUT) as i32
}
