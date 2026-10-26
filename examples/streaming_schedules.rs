//! Proptest drives the built public streaming entry point in child processes.
//! Usage: streaming_schedules <stream_header binary> <DWG path> <receipt path>.
//! A failing schedule is printed and shrunk; the JSONL artifact preserves every
//! attempted schedule. This is executable E2E acceptance, not a unit test.

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let binary = args.next().ok_or("missing built entry point")?;
    let input = args.next().ok_or("missing DWG path")?;
    let receipt = RefCell::new(File::create(args.next().ok_or("missing receipt path")?)?);
    let baseline = Command::new(&binary).arg(&input).arg("256").output()?;
    if !baseline.status.success() {
        return Err(format!(
            "baseline failed: {}",
            String::from_utf8_lossy(&baseline.stderr)
        )
        .into());
    }
    let mut runner = TestRunner::new(Config {
        cases: 128,
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(
            &proptest::collection::vec(1usize..=256, 1..=32),
            |schedule| {
                writeln!(receipt.borrow_mut(), "{schedule:?}")
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let chunks = schedule
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let output = Command::new(&binary)
                    .arg(&input)
                    .args(["--chunks", &chunks, "--pending"])
                    .output()
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                prop_assert_eq!(output.status.code(), baseline.status.code());
                prop_assert_eq!(&output.stdout, &baseline.stdout);
                prop_assert_eq!(&output.stderr, &baseline.stderr);
                Ok(())
            },
        )
        .map_err(|e| format!("chunk equivalence failed: {e}"))?;
    println!("128 generated chunk schedules matched exact output");
    Ok(())
}
