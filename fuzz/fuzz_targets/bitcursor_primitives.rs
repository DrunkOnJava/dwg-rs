#![no_main]
use dwg::bitcursor::BitCursor;
use libfuzzer_sys::fuzz_target;

// Fuzz target: bit-level primitives must never panic on any byte slice.
// Run every primitive against the same fuzz input.
fuzz_target!(|data: &[u8]| {
    let mut c = BitCursor::new(data);
    let _ = c.read_b();
    let _ = c.read_bb();
    let _ = c.read_3b();
    let _ = c.read_bs();
    let _ = c.read_bl();
    let _ = c.read_bll();
    let _ = c.read_bd();
    let _ = c.read_rc();
    let _ = c.read_rs();
    let _ = c.read_rl();
    let _ = c.read_rd();
    let _ = c.read_mc();
    let _ = c.read_ms();
    let _ = c.read_handle();
});
