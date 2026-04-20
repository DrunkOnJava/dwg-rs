#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz target (SEC-21): the Reed-Solomon (255,239) decoder in
// dwg::reed_solomon must never panic, infinite-loop, or OOM on
// arbitrary 255-byte codeword input. Valid-codeword result (either
// Ok or Err) is fine; the contract is "no panic".
//
// Input shape: accept any byte slice; truncate or pad to exactly
// 255 bytes (the fixed codeword size) so we exercise the full
// syndrome / Berlekamp-Massey / Chien / Forney path on every call
// even when libFuzzer hands us a short sample.
fuzz_target!(|data: &[u8]| {
    let mut codeword = [0u8; dwg::reed_solomon::CODEWORD_BYTES];
    let copy_len = data.len().min(dwg::reed_solomon::CODEWORD_BYTES);
    codeword[..copy_len].copy_from_slice(&data[..copy_len]);
    let _ = dwg::reed_solomon::verify(&mut codeword);
});
