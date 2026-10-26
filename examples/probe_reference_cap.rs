//! Does the lossy walk keep its reference diagnostics bounded on a
//! crafted file?
//!
//! # What this proves
//!
//! `DwgFile::all_objects_lossy` resolves every record's handle stream
//! against the handle map and records each unanswered reference in
//! `ObjectWalkSummary::unanswered_references` (#81). A handle stream is
//! attacker-controlled: every byte `0x60` is a complete §2.13 reference
//! (code 6, counter 0, "the owner's handle + 1"), so a record whose
//! handle stream is N bytes of `0x60` yields N references to a handle
//! the map does not list.
//!
//! This probe builds such a file in memory with the crate's own R2004
//! writer: one object record whose handle stream is `--bytes N` (default
//! 2,000,000) bytes of `0x60`, and a handle map that lists only that
//! record. It then opens the bytes with `DwgFile::from_bytes`, runs the
//! lossy walk and prints what the summary holds. The walk must decode all
//! N references, keep at most `WalkerLimits::max_retained_references` of
//! them, and count the rest in `unanswered_references_suppressed`.
//!
//! ```sh
//! cargo run --release --example probe_reference_cap
//! cargo run --release --example probe_reference_cap -- --bytes 500000
//! ```
//!
//! Exit status is non-zero if any of those three properties fails.

use dwg::bitwriter::BitWriter;
use dwg::file_writer::{WriterScaffold, assemble_dwg_bytes};
use dwg::handle_map::{HandleEntry, HandleMap, write_handle_map};
use dwg::limits::WalkerLimits;
use dwg::version::Version;
use dwg::{DwgFile, Error};
use std::process::ExitCode;

/// The `RL 0x0dca` every R2004-family object stream opens with.
const OBJECTS_STREAM_PREFIX: [u8; 4] = [0xCA, 0x0D, 0x00, 0x00];

/// Hard references (code 3, counter 1, handle 5: a record the map does
/// not list) appended after the `0x60` bytes, so past the retention cap
/// they are suppressed from the list yet must still be counted.
const HARD_TAIL: usize = 10;

/// One R2004 object record: `MS` size, then `BS` type, `RL` object-data
/// size in bits, the record's handle, and `n` bytes of `0x60` then
/// [`HARD_TAIL`] hard references as its handle stream; then a 2-byte CRC
/// slot.
fn crafted_record(n: usize) -> Vec<u8> {
    let mut head = BitWriter::new();
    head.write_bs(0x2A); // DICTIONARY; the walker does not decode it
    head.write_rl(0); // placeholder, rewritten below
    head.write_handle(0, 1);
    // The data stream ends at the first byte boundary after the
    // handle; everything after it is the handle stream.
    let head_bits = head.position_bits().div_ceil(8) * 8;
    let mut body = BitWriter::new();
    body.write_bs(0x2A);
    body.write_rl(head_bits as u32);
    body.write_handle(0, 1);
    let mut payload = body.into_bytes();
    payload.extend(std::iter::repeat_n(0x60u8, n));
    for _ in 0..HARD_TAIL {
        payload.extend_from_slice(&[0x31, 0x05]);
    }
    let mut record = BitWriter::new();
    record.write_ms(payload.len() as u64);
    let mut out = record.into_bytes();
    out.extend_from_slice(&payload);
    out.extend_from_slice(&[0, 0]);
    out
}

fn build(n: usize) -> Result<Vec<u8>, Error> {
    let version = Version::R2004;
    let mut stream = OBJECTS_STREAM_PREFIX.to_vec();
    let entries = vec![HandleEntry {
        handle: 1,
        offset: stream.len() as u64,
    }];
    stream.extend_from_slice(&crafted_record(n));
    let handles = write_handle_map(&HandleMap { entries }, &mut BitWriter::new(), version)?;
    let mut scaffold = WriterScaffold::new(version);
    scaffold.add_section("AcDb:Header", vec![0u8; 64]);
    scaffold.add_section("AcDb:Handles", handles);
    scaffold.add_section("AcDb:AcDbObjects", stream);
    let built = scaffold.build_sections()?;
    assemble_dwg_bytes(&built, version)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n = match args.as_slice() {
        [] => 2_000_000,
        [flag, value] if flag == "--bytes" => match value.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("--bytes wants a number");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: probe_reference_cap [--bytes N]");
            return ExitCode::FAILURE;
        }
    };
    let bytes = match build(n) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("could not build the crafted file: {e}");
            return ExitCode::FAILURE;
        }
    };
    let file = match DwgFile::from_bytes(bytes) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("could not open the crafted file: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(Ok((objects, summary))) = file.all_objects_lossy() else {
        eprintln!("no lossy walk of the crafted file");
        return ExitCode::FAILURE;
    };
    let cap = WalkerLimits::default().max_retained_references;
    println!("records walked              : {}", objects.len());

    println!("handle-stream bytes         : {n}");
    println!(
        "references decoded          : {}",
        summary.references_decoded
    );
    println!(
        "unanswered references kept  : {} (cap {cap})",
        summary.unanswered_references.len()
    );
    println!(
        "unanswered, suppressed      : {}",
        summary.unanswered_references_suppressed
    );
    println!(
        "unanswered hard, counted    : {} ({} of them in the kept list)",
        summary.unanswered_hard_count,
        summary.unanswered_hard_references().count()
    );
    let total = n + HARD_TAIL;
    let all_decoded = summary.references_decoded == total;
    let bounded = summary.unanswered_references.len() <= cap;
    let counted =
        summary.unanswered_references.len() + summary.unanswered_references_suppressed == total;
    // The hard tail sits after n soft references: past the cap it is not
    // in the kept list, and the count must still hold every one of them.
    let hard_exact = summary.unanswered_hard_count == HARD_TAIL
        && (n < cap || summary.unanswered_hard_references().count() == 0);
    println!(
        "RESULT: {}",
        if all_decoded && bounded && counted && hard_exact {
            "every reference decoded and counted, at most the cap retained, every hard one counted"
        } else {
            "FAILED"
        }
    );
    if all_decoded && bounded && counted && hard_exact {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
