#![no_main]
use libfuzzer_sys::fuzz_target;

// Fuzz target: the LZ77 decompressor should never panic, infinite-loop, or
// OOM on arbitrary input. Every malformed byte sequence must surface as
// an Error variant.
fuzz_target!(|data: &[u8]| {
    let _ = dwg::lz77::decompress(data, Some(1024 * 1024));
});
