#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz target: DwgFile::from_bytes on arbitrary input must never panic.
// Any malformed DWG must surface as Error, not a stack trace.
fuzz_target!(|data: &[u8]| {
    let _ = dwg::DwgFile::from_bytes(data.to_vec());
});
