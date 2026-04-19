#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz target: handle map parser must reject malformed input via Result,
// never via panic. This exercises the AcDb:Handles parser against
// randomized byte streams — a real attack surface for CRC-less sections.
fuzz_target!(|data: &[u8]| {
    let _ = dwg::handle_map::HandleMap::parse(data);
});
