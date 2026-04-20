//! dhat heap-profiling example.
//!
//! `dhat` is a heap profiler that instruments the global allocator
//! and emits a viewer-compatible JSON file on shutdown. Unlike
//! Criterion it does not measure wall-clock time — it measures
//! allocation count, peak heap, and total bytes allocated, which is
//! the right lens for a parser that may be run on untrusted files
//! inside a server process.
//!
//! # Run
//!
//! ```bash
//! cargo run --release --example dhat_profile --features dhat-heap
//! ```
//!
//! The `dhat-heap` feature is what installs `dhat::Alloc` as the
//! global allocator. Without it, this example compiles and runs but
//! does no profiling — so the default build is still fast and the
//! `dhat` dependency is only pulled in when the feature is active.
//!
//! On completion, dhat writes `dhat-heap.json` in the working
//! directory. Open it with the dhat viewer
//! (<https://nnethercote.github.io/dh_view/dh_view.html>) or with
//! `cargo install dhat-viewer` for a local tool.
//!
//! # What is profiled
//!
//! Three phases — mirroring the `libredwg_compare` harness:
//!
//! 1. `DwgFile::open` on `../../samples/sample_AC1032.dwg`
//! 2. `all_objects()` (handle-map-driven walk of `AcDbObjects`)
//! 3. `summary_info()`
//!
//! If the sample is absent the example prints a skip notice and
//! exits successfully, so it can live in `examples/` without
//! breaking downstream builds that vendor the crate without the
//! corpus.

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use dwg::DwgFile;
use std::path::PathBuf;
use std::process::ExitCode;

fn sample_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../samples");
    p.push(name);
    p
}

fn main() -> ExitCode {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    #[cfg(not(feature = "dhat-heap"))]
    eprintln!(
        "note: running without the `dhat-heap` feature; no profile will be emitted. \
         re-run with `cargo run --release --example dhat_profile --features dhat-heap`"
    );

    let path = sample_path("sample_AC1032.dwg");
    if !path.exists() {
        eprintln!(
            "sample {} absent — dhat profile has nothing to measure. skipping.",
            path.display()
        );
        return ExitCode::SUCCESS;
    }

    // Phase 1: open + container parse.
    let file = match DwgFile::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("open failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "opened {} ({} sections)",
        path.display(),
        file.sections().len()
    );

    // Phase 2: handle-map-driven object walk.
    match file.all_objects() {
        Some(Ok(objs)) => println!("all_objects: {} objects", objs.len()),
        Some(Err(e)) => eprintln!("all_objects error: {e}"),
        None => eprintln!("all_objects: not supported for this version"),
    }

    // Phase 3: summary metadata.
    match file.summary_info() {
        Some(Ok(si)) => println!("summary_info: title={:?}", si.title),
        Some(Err(e)) => eprintln!("summary_info error: {e}"),
        None => eprintln!("summary_info: section absent"),
    }

    #[cfg(feature = "dhat-heap")]
    println!("dhat-heap.json written to the current working directory");

    ExitCode::SUCCESS
}
