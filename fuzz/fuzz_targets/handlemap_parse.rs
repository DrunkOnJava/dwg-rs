#![no_main]
use dwg::handle_map::HandleMap;
use libfuzzer_sys::fuzz_target;

// Fuzz target: HandleMap::parse must never panic on any byte slice.
// The handle section is a variable-length bit-packed stream; arbitrary
// input can produce over-count loops, truncated fields, and invalid
// handle values — all must return Err, never panic.
fuzz_target!(|data: &[u8]| {
    let _ = HandleMap::parse(data);
});
