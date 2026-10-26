//! Run the ODA specification's own worked examples through the crate (#107).
//!
//! # What this proves
//!
//! The *Open Design Specification for .dwg files* v5.4.1 prints worked
//! examples: bit streams with the values they parse to (§2), the LZ77
//! length encodings (§4.7), a complete R14 file header (§9) and 78 R14
//! object records (§20.4.N.1). It also prints tables the crate must
//! reproduce: the CRC-8 and CRC-32 tables (§2.14), the locator-CRC XOR
//! values (§3.2.6), the header magic sequence (§4.1), the 16-byte sentinels
//! (§3.2.6, §9, §10.1) and the R2007 literal-copy table (§5.10.1).
//! `tests/fixtures/spec-examples/` holds them verbatim with page numbers;
//! `tools/spec/extract_examples.py` regenerates and audits them from the PDF.
//!
//! Every example runs the crate's real public code on the spec's bytes and
//! compares the spec's stated result:
//!
//! - §2: [`BitCursor`] `read_bs`, `read_bl`, `read_mc`, `read_ms`,
//!   `read_handle`, consuming exactly the printed stream.
//! - §2.14, §3.2.6, §4.1: `crc::crc8`, `crc::crc32`,
//!   `crc::r13_locator_seed`, `cipher::magic_sequence` and the streaming
//!   grammar's sentinel constants.
//! - §4.7: both LZ77 decoders, `lz77::decompress` and the streaming
//!   `Lz77Decoder`, on the smallest stream whose output length depends only
//!   on the encoding under test.
//! - §5.10.1: `r21_lz::decompress` on a literal-only stream per row.
//! - §9: `R13R15Header::parse`, the locator and section CRCs, the streaming
//!   `FlatWalker` (which verifies both) and `HeaderVars`.
//! - §20.4.N.1: `ObjectWalker` must return the printed type, length and
//!   handle, the printed CRC must be CRC-8 over the record, and
//!   `decode_from_raw` must end exactly on the record's own §20.4.1 `RL`
//!   boundary. The spec prints no decoded field values for these dumps, so
//!   "closes" is the crate's self-validation run on spec bytes, not a value
//!   comparison. Records numbered from 500 are classes, and the spec prints no
//!   `AcDb:Classes` for them, so they are checked to the walk and CRC only.
//!
//! `r14-objects.tsv` carries an `expect` column (`closes`, `unhandled`,
//! `walk-only`, or `error #N` naming the issue that tracks the
//! disagreement). Any result that differs from it, in either direction,
//! fails the run, so a fixed decoder has to update its row.
//!
//! # How to verify
//!
//! ```sh
//! cargo run --release --example spec_examples
//! ```
//!
//! Exit status 0 means every example matched its fixture.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use dwg::bitcursor::BitCursor;
use dwg::entities::{DecodedEntity, decode_from_raw};
use dwg::header::R13R15Header;
use dwg::lz77::DecompressLimits;
use dwg::object::ObjectWalker;
use dwg::streaming::{
    CLASS_SENTINELS, END_SENTINEL, FlatWalker, HEADER_VARS_SENTINELS, Lz77Decoder, Part,
    SourceIdentity, Step,
};
use dwg::{ClassMap, HeaderVars, Version, cipher, crc, lz77, r21_lz};

/// Collects every comparison so one run reports all disagreements.
#[derive(Default)]
struct Report {
    checks: usize,
    failures: Vec<String>,
}

impl Report {
    fn check(&mut self, id: &str, ok: bool, detail: impl FnOnce() -> String) -> bool {
        self.checks += 1;
        if !ok {
            self.failures.push(format!("{id}: {}", detail()));
        }
        ok
    }
}

fn fixture(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "spec-examples",
        name,
    ]
    .iter()
    .collect();
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The rows of a fixture TSV, keyed by its header line.
fn rows(name: &str) -> Vec<HashMap<String, String>> {
    let text = fixture(name);
    let mut lines = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let columns: Vec<&str> = lines.next().expect("header line").split('\t').collect();
    lines
        .map(|line| {
            columns
                .iter()
                .zip(line.split('\t'))
                .map(|(c, v)| ((*c).to_string(), v.to_string()))
                .collect()
        })
        .collect()
}

fn hex_bytes(text: &str) -> Vec<u8> {
    let digits: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex byte"))
        .collect()
}

/// Every `0x…` word of a fixture, in order.
fn hex_words(text: &str) -> Vec<u64> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .flat_map(|l| l.split(|c: char| c.is_whitespace() || c == ','))
        .filter_map(|w| w.strip_prefix("0x"))
        .map(|w| u64::from_str_radix(w, 16).expect("hex word"))
        .collect()
}

/// A printed bit stream, MSB first, zero-padded to whole bytes.
fn pack(bits: &str) -> Vec<u8> {
    bits.as_bytes()
        .chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |b, (i, &c)| b | (u8::from(c == b'1') << (7 - i)))
        })
        .collect()
}

fn section_cell(row: &HashMap<String, String>) -> String {
    format!("§{} p{}", row["section"], row["page"])
}

// ---------------------------------------------------------------- §2

/// Read what one `bitcodes.tsv` row's `code` prescribes, as printable values.
fn read_codes(c: &mut BitCursor<'_>, code: &str, count: usize) -> dwg::Result<Vec<String>> {
    Ok(match code {
        "BS" => (0..count)
            .map(|_| c.read_bs().map(|v| v.to_string()))
            .collect::<dwg::Result<_>>()?,
        "BL" => (0..count)
            .map(|_| c.read_bl().map(|v| v.to_string()))
            .collect::<dwg::Result<_>>()?,
        "MC" => vec![c.read_mc()?.to_string()],
        "MS" => vec![c.read_ms()?.to_string()],
        "H" => {
            let h = c.read_handle()?;
            vec![
                h.code.to_string(),
                h.counter.to_string(),
                format!("0x{:X}", h.value),
            ]
        }
        "CRC" => {
            let value = c.read_bs()?.to_string();
            let mut unused = String::new();
            while c.position_bits() % 8 != 0 {
                unused.push(if c.read_b()? { '1' } else { '0' });
            }
            c.align_to_byte();
            let crc = format!("{:02X}{:02X}", c.read_rc()?, c.read_rc()?);
            vec![value, unused, crc]
        }
        other => panic!("bitcodes.tsv: unknown code {other}"),
    })
}

fn bit_codes(r: &mut Report) {
    let rows = rows("bitcodes.tsv");
    let mut reproduced = 0;
    for row in &rows {
        let bits = &row["bits"];
        let bytes = pack(bits);
        let mut c = BitCursor::new(&bytes);
        let expected: Vec<&str> = row["expected"].split(' ').collect();
        let got = read_codes(&mut c, &row["code"], expected.len());
        let id = format!("{} {} ({})", row["code"], row["id"], section_cell(row));
        let ok = match &got {
            Ok(values) => r.check(&id, *values == expected, || {
                format!("spec prints {expected:?}, crate read {values:?}")
            }),
            Err(e) => r.check(&id, false, || {
                format!("spec prints {expected:?}, crate failed: {e}")
            }),
        };
        let whole = r.check(&id, c.position_bits() == bits.len(), || {
            format!(
                "parse ended at bit {}, the printed stream has {}",
                c.position_bits(),
                bits.len()
            )
        });
        if ok && whole {
            reproduced += 1;
        }
    }
    println!(
        "§2 bit codes            {reproduced}/{} worked examples reproduced",
        rows.len()
    );
}

// ---------------------------------------------------------------- printed tables

fn printed_tables(r: &mut Report) {
    let table = hex_words(&fixture("crc8-table.txt"));
    let crc8_ok = table.len() == 256
        && table
            .iter()
            .enumerate()
            .all(|(i, &t)| u64::from(crc::crc8(0, &[i as u8])) == t);
    r.check("§2.14.1 p15-16 crctable", crc8_ok, || {
        "crc8(0, [i]) differs from the printed table".into()
    });

    let table = hex_words(&fixture("crc32-table.txt"));
    let crc32_ok = table.len() == 256
        && table
            .iter()
            .enumerate()
            .all(|(i, &t)| u64::from(!crc::crc32(u32::MAX, &[i as u8])) == t);
    r.check("§2.14.2 p16-18 crc32Table", crc32_ok, || {
        "!crc32(0xFFFFFFFF, [i]) differs from the printed table".into()
    });

    let mut xor_ok = 0;
    let xor_rows = rows("locator-xor.tsv");
    for row in &xor_rows {
        let count: usize = row["count"].parse().expect("count");
        let xor = u16::from_str_radix(&row["xor"], 16).expect("xor");
        let got = crc::r13_locator_seed(count);
        if r.check(&section_cell(row), got == Some(xor), || {
            format!("{count} records: spec prints {xor:04X}, crate has {got:04X?}")
        }) {
            xor_ok += 1;
        }
    }

    let printed = hex_bytes(
        &fixture("magic-sequence.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<String>(),
    );
    let magic = cipher::magic_sequence();
    let magic_ok = r.check(
        "§4.1 p23 magic sequence",
        magic.len() == 0x6C && magic.starts_with(&printed),
        || {
            format!(
                "the crate's 0x6C bytes do not begin with the {} printed",
                printed.len()
            )
        },
    );

    let crate_sentinels: [(&str, &[u8; 16]); 6] = [
        ("file-header-end", &END_SENTINEL),
        ("header-vars-start", &HEADER_VARS_SENTINELS[0]),
        ("header-vars-start", &HeaderVars::SENTINEL),
        ("header-vars-end", &HEADER_VARS_SENTINELS[1]),
        ("classes-start", &CLASS_SENTINELS[0]),
        ("classes-end", &CLASS_SENTINELS[1]),
    ];
    let printed_sentinels: HashMap<String, (Vec<u8>, String)> = rows("sentinels.tsv")
        .into_iter()
        .map(|row| {
            let cell = section_cell(&row);
            (row["id"].clone(), (hex_words_inline(&row["quote"]), cell))
        })
        .collect();
    let mut sentinel_ok = 0;
    for (id, constant) in crate_sentinels
        .iter()
        .copied()
        .chain([("classes-start", &ClassMap::SENTINEL_START)])
    {
        let (bytes, cell) = &printed_sentinels[id];
        if r.check(&format!("{cell} sentinel {id}"), bytes == constant, || {
            "the crate's constant differs from the printed bytes".into()
        }) {
            sentinel_ok += 1;
        }
    }

    println!(
        "§2.14 / §3.2.6 / §4.1   CRC-8 table {}, CRC-32 table {}, locator XOR {xor_ok}/{}, magic sequence {} ({} printed bytes of 0x6C), sentinels {sentinel_ok}/7",
        verdict(crc8_ok),
        verdict(crc32_ok),
        xor_rows.len(),
        verdict(magic_ok),
        printed.len(),
    );
}

fn hex_words_inline(text: &str) -> Vec<u8> {
    hex_words(text).into_iter().map(|w| w as u8).collect()
}

fn verdict(ok: bool) -> &'static str {
    if ok { "reproduced" } else { "DIFFERS" }
}

// ---------------------------------------------------------------- §4.7

/// Feed `stream` to the streaming §4.7 decoder in one call.
fn stream_decode(stream: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut decoder = Lz77Decoder::new(limit as u64);
    let mut out = vec![0u8; limit];
    let mut produced = 0;
    let mut consumed = 0;
    loop {
        let progress = decoder
            .decode(&stream[consumed..], &mut out[produced..])
            .map_err(|e| e.to_string())?;
        consumed += progress.consumed;
        produced += progress.produced;
        if progress.done {
            break;
        }
        if progress.consumed == 0 && progress.produced == 0 {
            return Err("no progress before the terminator".into());
        }
    }
    decoder.end_of_input().map_err(|e| e.to_string())?;
    out.truncate(produced);
    Ok(out)
}

fn lz77_examples(r: &mut Report) {
    let rows = rows("lz77.tsv");
    let mut reproduced = 0;
    for row in &rows {
        let encoded = hex_bytes(&row["encoded"]);
        let value = usize::from_str_radix(&row["expected"], 16).expect("expected");
        // Literal length: the encoding, that many literals, the 0x11
        // terminator. Long compression offset: four equal literals, then
        // opcode 0x20 (compressedBytes = offset + 0x21) with a back-reference
        // into them, so the output length depends only on the encoding and
        // the output bytes on neither offset convention (ARCHITECTURE.md).
        let (stream, expected) = match row["kind"].as_str() {
            "literal-length" => {
                let literals: Vec<u8> = (0..value).map(|i| b'A' + (i % 26) as u8).collect();
                let mut s = encoded.clone();
                s.extend_from_slice(&literals);
                s.push(0x11);
                (s, literals)
            }
            "long-compression-offset" => {
                let mut s = vec![0x01, b'A', b'A', b'A', b'A', 0x20];
                s.extend_from_slice(&encoded);
                s.extend_from_slice(&[0x04, 0x00, 0x11]);
                (s, vec![b'A'; 4 + value + 0x21])
            }
            other => panic!("lz77.tsv: unknown kind {other}"),
        };
        let id = format!("{} {} ({})", row["kind"], row["id"], section_cell(row));
        let reader = lz77::decompress(&stream, Some(expected.len()));
        let a = r.check(
            &format!("{id} lz77::decompress"),
            reader.as_ref().is_ok_and(|o| *o == expected),
            || {
                format!(
                    "spec value 0x{value:X}, decoder gave {:?}",
                    reader.as_ref().map(Vec::len)
                )
            },
        );
        let streamed = stream_decode(&stream, expected.len());
        let b = r.check(
            &format!("{id} streaming"),
            streamed.as_ref().is_ok_and(|o| *o == expected),
            || {
                format!(
                    "spec value 0x{value:X}, decoder gave {:?}",
                    streamed.as_ref().map(Vec::len)
                )
            },
        );
        if a && b {
            reproduced += 1;
        }
    }
    println!(
        "§4.7 LZ77               {reproduced}/{} worked examples reproduced by both decoders",
        rows.len()
    );
}

// ---------------------------------------------------------------- §5.10.1

/// Output order of a `count`-byte literal under the §5.10.1 table, each
/// block expanded through its own row (CLEANROOM.md, 2026-08-30, item 1).
fn copy_order(table: &HashMap<usize, Vec<(usize, usize)>>, count: usize) -> Vec<usize> {
    if count == 1 {
        return vec![0];
    }
    table[&count]
        .iter()
        .flat_map(|&(size, index)| copy_order(table, size).into_iter().map(move |i| index + i))
        .collect()
}

fn r2007_copy_blocks(r: &mut Report) {
    let rows = rows("r2007-copy-blocks.tsv");
    let table: HashMap<usize, Vec<(usize, usize)>> = rows
        .iter()
        .map(|row| {
            let blocks = row["blocks"]
                .split(' ')
                .map(|b| {
                    let (size, index) = b.trim_end_matches(']').split_once('[').expect("n[i]");
                    (size.parse().expect("size"), index.parse().expect("index"))
                })
                .collect();
            (row["count"].parse().expect("count"), blocks)
        })
        .collect();
    let mut reproduced = 0;
    for count in 1..=32 {
        let id = format!("§5.10.1 p54-55 copy table row {count}");
        let order = copy_order(&table, count);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        let partition = r.check(&id, sorted == (0..count).collect::<Vec<_>>(), || {
            format!("the row's blocks cover {sorted:?}, not 0..{count}")
        });
        let literal: Vec<u8> = (0..count).map(|i| 0x40 + i as u8).collect();
        let mut stream = match count {
            1..=7 => vec![0x20, 0x00, 0x00, count as u8],
            8..=0x16 => vec![count as u8 - 8],
            _ => vec![0x0F, count as u8 - 0x17],
        };
        stream.extend_from_slice(&literal);
        let expected: Vec<u8> = order.iter().map(|&i| literal[i]).collect();
        let got = r21_lz::decompress(&stream, count, DecompressLimits::default());
        let same = r.check(&id, got.as_ref().is_ok_and(|g| *g == expected), || {
            format!("table order {expected:02X?}, r21_lz gave {got:02X?}")
        });
        if partition && same {
            reproduced += 1;
        }
    }
    println!("§5.10.1 R2007 copy      {reproduced}/32 table rows reproduced by r21_lz::decompress");
}

// ---------------------------------------------------------------- §9

fn r14_header(r: &mut Report) {
    let text = fixture("r14-header.hex");
    let mut dump = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (address, bytes) = line.split_once(' ').expect("address and bytes");
        let address = usize::from_str_radix(address, 16).expect("address");
        r.check("§9 p80-83 dump", address == dump.len(), || {
            format!("row {address:05X} after {} bytes", dump.len())
        });
        dump.extend(hex_bytes(bytes));
    }
    let id = "§9 p80-83 R14 header";
    let header = match R13R15Header::parse(&dump) {
        Ok(h) => h,
        Err(e) => {
            r.check(id, false, || format!("R13R15Header::parse: {e}"));
            return;
        }
    };
    r.check(id, header.common.version == Version::R14, || {
        format!("version {:?}", header.common.version)
    });
    let count = header.locators.len();
    let crc_at = 0x19 + count * 9;
    let stored = u16::from_le_bytes([dump[crc_at], dump[crc_at + 1]]);
    let seed = crc::r13_locator_seed(count).unwrap_or(0);
    r.check(
        &format!("{id} §3.2.6 locator CRC"),
        crc::crc8(0, &dump[..crc_at]) ^ seed == stored,
        || format!("{count} locators, stored {stored:04X}"),
    );
    r.check(
        &format!("{id} §3.2.6 sentinel"),
        dump[crc_at + 2..crc_at + 18] == END_SENTINEL,
        || "the bytes after the CRC are not the printed sentinel".into(),
    );
    let vars = header.locators[0];
    let start = vars.seeker as usize;
    let end = start + vars.size as usize;
    r.check(&format!("{id} locator 0"), end == dump.len(), || {
        format!(
            "header variables span {start:#X}..{end:#X}, the dump ends at {:#X}",
            dump.len()
        )
    });
    let size = u32::from_le_bytes(dump[start + 16..start + 20].try_into().expect("RL")) as usize;
    let data_end = start + 20 + size;
    let section_crc = u16::from_le_bytes([dump[data_end], dump[data_end + 1]]);
    r.check(
        &format!("{id} §9 sentinels"),
        dump[start..start + 16] == HEADER_VARS_SENTINELS[0]
            && dump[data_end + 2..end] == HEADER_VARS_SENTINELS[1],
        || "a header-variable sentinel differs from the printed one".into(),
    );
    r.check(
        &format!("{id} §9 CRC"),
        crc::crc8(0xC0C1, &dump[start + 16..data_end]) == section_crc,
        || format!("stored {section_crc:04X}"),
    );
    let bds = HeaderVars::parse_strict(&dump[start..], Version::R14)
        .map(|v| v.read_first_n_bds(4))
        .unwrap_or_default();
    r.check(
        &format!("{id} §9 p70 leading BDs"),
        bds == [412_148_564_080.0, 1.0, 1.0, 1.0],
        || format!("first four BDs {bds:?}, §9 prints the defaults 412148564080.0, 1.0, 1.0, 1.0"),
    );

    // The streaming grammar verifies the same CRCs and sentinels itself.
    // The dump does not give the file's length; the image seeker is the
    // least it can be.
    let mut walker = FlatWalker::new(SourceIdentity {
        digest: [0; 32],
        length: u64::from(header.common.image_seeker),
    });
    let mut steps = Vec::new();
    let next = loop {
        match walker.request() {
            Ok(Some(req)) if (req.offset as usize) < dump.len() => {
                let from = req.offset as usize;
                let to = (from + req.len).min(dump.len());
                match walker.feed(req.offset, &dump[from..to]) {
                    Ok(step) => steps.push(step),
                    Err(e) => break Err(e.to_string()),
                }
            }
            Ok(req) => break Ok(req),
            Err(e) => break Err(e.to_string()),
        }
    };
    let flat_header = steps.iter().find_map(|s| match s {
        Step::Header(h) => Some(*h),
        _ => None,
    });
    let section = steps.iter().find_map(|s| match s {
        Step::Section(s) if s.part == Part::HeaderVars => Some(*s),
        _ => None,
    });
    let locators_agree = flat_header.is_some_and(|h| {
        usize::from(h.locator_count) == count
            && h.locators[..count]
                .iter()
                .zip(&header.locators)
                .all(|(a, b)| a.number == b.number && a.offset == b.seeker && a.size == b.size)
    });
    let walk_ok = locators_agree
        && section.is_some_and(|s| s.size as usize == size)
        && next
            .as_ref()
            .is_ok_and(|n| n.is_some_and(|n| n.offset == u64::from(header.locators[1].seeker)));
    r.check(&format!("{id} streaming FlatWalker"), walk_ok, || {
        format!("header {flat_header:?}, section {section:?}, next request {next:?}")
    });
    println!(
        "§9 R14 file header      AC1014, {count} locators, locator CRC {stored:04X}, header-variable CRC {section_crc:04X}: {}",
        verdict(r.failures.iter().all(|f| !f.starts_with("§9")))
    );
}

// ---------------------------------------------------------------- §20.4.N.1

fn classify(raw: &dwg::RawObject) -> (String, String) {
    if raw.type_code >= 500 {
        return ("walk-only".into(), String::new());
    }
    match decode_from_raw(raw, Version::R14) {
        DecodedEntity::Unhandled { .. } => ("unhandled".into(), String::new()),
        DecodedEntity::Error { message, .. } => ("error".into(), message),
        _ => ("closes".into(), String::new()),
    }
}

fn r14_objects(r: &mut Report) {
    let rows = rows("r14-objects.tsv");
    let mut tally: HashMap<String, usize> = HashMap::new();
    let mut walked = 0;
    println!("§20.4.N.1 R14 objects:");
    for row in &rows {
        let record = hex_bytes(&row["record"]);
        let id = format!("§{} p{} {}", row["section"], row["pages"], row["label"]);
        let type_code = u16::from_str_radix(&row["type"], 16).expect("type");
        let len: u32 = row["len"].parse().expect("len");
        let handle = u64::from_str_radix(&row["handle"], 16).expect("handle");
        let body = &record[..record.len() - 2];
        let stored = u16::from_le_bytes([record[record.len() - 2], record[record.len() - 1]]);
        let crc_ok = r.check(&id, crc::crc8(0xC0C1, body) == stored, || {
            format!("printed CRC {stored:04X} is not CRC-8(0xC0C1) of the record")
        });
        let raws = ObjectWalker::new(&record, Version::R14).collect_all_strict();
        let raw = match raws {
            Ok(mut v) if v.len() == 1 => v.remove(0),
            other => {
                r.check(&id, false, || format!("ObjectWalker: {other:?}"));
                continue;
            }
        };
        let walk_ok = r.check(&id, raw.type_code == type_code && raw.size_bytes == len && raw.handle.value == handle, || {
            format!(
                "spec prints type {type_code:#X} len {len} handle {handle:X}; walker read type {:#X} len {} handle {:X}",
                raw.type_code, raw.size_bytes, raw.handle.value
            )
        });
        if crc_ok && walk_ok {
            walked += 1;
        }
        let (result, message) = classify(&raw);
        let expect = &row["expect"];
        let matches = expect.split(' ').next() == Some(result.as_str());
        r.check(&id, matches, || {
            format!("expected {expect}, got {result} {message}")
        });
        *tally.entry(result.clone()).or_default() += 1;
        println!(
            "  §{:<9} p{:<8} {:<18} type {:>5} len {:>4} handle {:>4}  {:<10} {}{}",
            row["section"],
            row["pages"],
            row["label"],
            format!("{type_code:#X}"),
            len,
            row["handle"],
            result,
            if matches {
                String::new()
            } else {
                format!("(fixture expects {expect}) ")
            },
            message,
        );
    }
    println!(
        "§20.4.N.1 R14 objects   {walked}/{} dumps: printed type, length, handle and CRC reproduced; {} close on their own RL boundary, {} have no decoder, {} error, {} are classes (walk only)",
        rows.len(),
        tally.get("closes").copied().unwrap_or(0),
        tally.get("unhandled").copied().unwrap_or(0),
        tally.get("error").copied().unwrap_or(0),
        tally.get("walk-only").copied().unwrap_or(0),
    );
}

fn main() -> ExitCode {
    let mut r = Report::default();
    println!("ODA spec v5.4.1 worked examples through dwg's public API (#107)");
    bit_codes(&mut r);
    printed_tables(&mut r);
    lz77_examples(&mut r);
    r2007_copy_blocks(&mut r);
    r14_header(&mut r);
    r14_objects(&mut r);
    if r.failures.is_empty() {
        println!("OK: {} checks, every result matches its fixture", r.checks);
        ExitCode::SUCCESS
    } else {
        println!(
            "{} of {} checks differ from their fixture:",
            r.failures.len(),
            r.checks
        );
        for f in &r.failures {
            println!("  {f}");
        }
        ExitCode::FAILURE
    }
}
