//! Check every Reed-Solomon codeword of R2007 drawings against §5.13 (#275).
//!
//! # What this proves
//!
//! ODA spec §5.13 gives R2007 two Reed-Solomon codes: (255, 239) over the
//! field polynomial `0x169` for system pages and (255, 251) over `0x11D`
//! for data pages. For every R2007 drawing given, this probe takes each
//! codeword the file holds, de-interleaved per §5.13.2:
//!
//! - the file header page at 0x80 (3 system codewords, §5.2),
//! - the page map and the section map (system pages, §5.3), and
//! - every page of a section whose encoding is 4 (data pages, §5.4),
//!
//! and checks, through `dwg::reed_solomon` and `dwg::reed_solomon_encode`:
//!
//! 1. **intact**: [`Code::verify`] finds zero syndromes;
//! 2. **re-encoded**: [`encode_with`] on the codeword's message returns
//!    AutoCAD's own codeword, parity bytes included;
//! 3. **corrected**: with `(n - k) / 2` of its bytes overwritten (message
//!    and parity alike), [`Code::verify`] reports that many corrections and
//!    restores the codeword;
//! 4. **interleaved**: with one byte of every codeword of a page
//!    overwritten in place, [`Code::verify_interleaved`] restores the page.
//!
//! A codeword of either kind that verifies under the other code is also
//! counted, which is how the measurement tells the two polynomials apart.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example probe_r2007_rs -- <file-or-dir> ...
//! ```
//!
//! Directories are searched recursively for `.dwg` files. With no
//! argument it reads `../../samples` (the local corpus, see
//! CLAUDE.md). Files that are not R2007 are skipped; with no R2007 file at
//! all it prints `NOT MEASURED` and exits 0. Any failed check exits 1.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dwg::r2007::{Container, PAGE_BASE};
use dwg::reed_solomon::{CODEWORD_BYTES, Code};
use dwg::reed_solomon_encode::encode_with;
use dwg::{DwgFile, Version};

/// One page's worth of interleaved codewords.
struct Page {
    kind: &'static str,
    code: Code,
    offset: usize,
    blocks: usize,
}

#[derive(Default)]
struct Tally {
    codewords: usize,
    intact: usize,
    reencoded: usize,
    corrected: usize,
    interleaved: usize,
    pages: usize,
    other_code: usize,
}

/// §5.3: a system page holds `factor` copies of its data padded to 8 bytes,
/// in `ceil(factor · align8(stored) / 239)` codewords.
fn system_blocks(compressed: u64, uncompressed: u64, factor: u64) -> usize {
    let stored = compressed.min(uncompressed) as usize;
    (factor as usize * stored.div_ceil(8) * 8).div_ceil(Code::SYSTEM_PAGE.message_bytes())
}

fn pages_of(c: &Container) -> Vec<Page> {
    let h = &c.header;
    let mut pages = vec![Page {
        kind: "file header",
        code: Code::SYSTEM_PAGE,
        offset: 0x80,
        blocks: 3,
    }];
    pages.push(Page {
        kind: "page map",
        code: Code::SYSTEM_PAGE,
        offset: PAGE_BASE + h.pages_map_offset as usize,
        blocks: system_blocks(
            h.pages_map_size_compressed,
            h.pages_map_size_uncompressed,
            h.pages_map_correction_factor,
        ),
    });
    if let Some(offset) = c.page_map.file_offset_of(h.sections_map_id as i64) {
        pages.push(Page {
            kind: "section map",
            code: Code::SYSTEM_PAGE,
            offset,
            blocks: system_blocks(
                h.sections_map_size_compressed,
                h.sections_map_size_uncompressed,
                h.sections_map_correction_factor,
            ),
        });
    }
    for section in c.section_map.sections.iter().filter(|s| s.encoding == 4) {
        for page in &section.pages {
            if let Some(offset) = c.page_map.file_offset_of(page.id) {
                pages.push(Page {
                    kind: "data page",
                    code: Code::DATA_PAGE,
                    offset,
                    blocks: Code::DATA_PAGE
                        .codewords_for(page.compressed as usize)
                        .max(1),
                });
            }
        }
    }
    pages
}

fn other(code: Code) -> Code {
    if code == Code::SYSTEM_PAGE {
        Code::DATA_PAGE
    } else {
        Code::SYSTEM_PAGE
    }
}

fn check_page(bytes: &[u8], page: &Page, t: &mut Tally, failures: &mut Vec<String>) {
    let span = page.blocks * CODEWORD_BYTES;
    let Some(raw) = bytes.get(page.offset..page.offset + span) else {
        failures.push(format!(
            "{} at {:#X}: {span} bytes past the end of the file",
            page.kind, page.offset
        ));
        return;
    };
    t.pages += 1;
    let k = page.code.message_bytes();
    let t_max = page.code.parity_bytes() / 2;
    for j in 0..page.blocks {
        let original: Vec<u8> = (0..CODEWORD_BYTES)
            .map(|i| raw[page.blocks * i + j])
            .collect();
        let at = format!("{} at {:#X}, codeword {j}", page.kind, page.offset);
        t.codewords += 1;
        let mut cw = original.clone();
        match page.code.verify(&mut cw) {
            Ok(0) => t.intact += 1,
            other => failures.push(format!("{at}: verify gave {other:?} on the stored bytes")),
        }
        match encode_with(page.code, &original[..k]) {
            Ok(re) if re == original => t.reencoded += 1,
            Ok(_) => failures.push(format!(
                "{at}: re-encoding the message gives other parity bytes"
            )),
            Err(e) => failures.push(format!("{at}: encode_with: {e}")),
        }
        let mut damaged = original.clone();
        for e in 0..t_max {
            let pos = (j * 37 + e * 59 + 11) % CODEWORD_BYTES;
            damaged[pos] ^= 0xA5 ^ e as u8;
        }
        match page.code.verify(&mut damaged) {
            Ok(n) if n == t_max && damaged == original => t.corrected += 1,
            other => failures.push(format!("{at}: {t_max} injected errors gave {other:?}")),
        }
        if other(page.code)
            .syndromes(&original)
            .is_ok_and(|s| s.iter().all(|&b| b == 0))
        {
            t.other_code += 1;
        }
    }
    let mut damaged = raw.to_vec();
    for j in 0..page.blocks {
        damaged[page.blocks * ((j * 101 + 3) % CODEWORD_BYTES) + j] ^= 0x5A;
    }
    match page.code.verify_interleaved(&mut damaged, page.blocks) {
        Ok(n) if n == page.blocks && damaged == raw => t.interleaved += 1,
        other => failures.push(format!(
            "{} at {:#X}: verify_interleaved gave {other:?}",
            page.kind, page.offset
        )),
    }
}

fn inputs() -> Vec<PathBuf> {
    let mut args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if args.is_empty() {
        args.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples"));
    }
    let mut files = Vec::new();
    for a in args {
        collect(&a, &mut files);
    }
    files
}

/// `path` itself, or every `.dwg` below it, sorted.
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    if !path.is_dir() {
        files.push(path.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, files);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dwg")) {
            files.push(p);
        }
    }
}

fn main() -> ExitCode {
    let mut failures = Vec::new();
    let mut measured = 0;
    println!("§5.13 Reed-Solomon on every R2007 page (#275)");
    for path in inputs() {
        let Ok(bytes) = std::fs::read(&path) else {
            failures.push(format!("{}: unreadable", path.display()));
            continue;
        };
        let Ok(file) = DwgFile::from_bytes(bytes.clone()) else {
            continue;
        };
        if file.version() != Version::R2007 {
            continue;
        }
        let Some(container) = file.r2007_container() else {
            failures.push(format!("{}: R2007 container did not parse", path.display()));
            continue;
        };
        measured += 1;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let (mut system, mut data) = (Tally::default(), Tally::default());
        let before = failures.len();
        for page in pages_of(container) {
            let t = if page.code == Code::SYSTEM_PAGE {
                &mut system
            } else {
                &mut data
            };
            check_page(&bytes, &page, t, &mut failures);
        }
        for f in &mut failures[before..] {
            *f = format!("{name}: {f}");
        }
        for (label, code, t) in [
            ("system", Code::SYSTEM_PAGE, &system),
            ("data", Code::DATA_PAGE, &data),
        ] {
            println!(
                "  {name:<24} {label:<6} (255,{}) {:#05X}: {} pages, {} codewords: {} intact, {} re-encoded, {} corrected ({} errors each), {} interleaved pages restored; {} verify under the other code",
                code.message_bytes(),
                code.primitive_polynomial(),
                t.pages,
                t.codewords,
                t.intact,
                t.reencoded,
                t.corrected,
                code.parity_bytes() / 2,
                t.interleaved,
                t.other_code,
            );
        }
    }
    if measured == 0 {
        println!("NOT MEASURED: no R2007 drawing among the inputs");
        return ExitCode::SUCCESS;
    }
    if failures.is_empty() {
        println!("OK: every codeword of {measured} R2007 file(s) checks out under §5.13");
        ExitCode::SUCCESS
    } else {
        println!("{} failure(s):", failures.len());
        for f in &failures {
            println!("  {f}");
        }
        ExitCode::FAILURE
    }
}
