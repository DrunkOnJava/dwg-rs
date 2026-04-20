#![no_main]
use dwg::header_vars::HeaderVars;
use dwg::version::Version;
use libfuzzer_sys::fuzz_target;

// Fuzz target: HeaderVars::parse_lossy must never panic on any byte
// slice. The HEADER section is a bit-packed stream of ~400 header vars
// spanning multiple primitive types; arbitrary input can produce
// truncation, overlong BL/BD/MC payloads, or invalid code-page bytes —
// all must return Err, never panic. We exercise every supported version
// so version-conditional branches are covered.
fuzz_target!(|data: &[u8]| {
    for version in [
        Version::R14,
        Version::R2000,
        Version::R2004,
        Version::R2007,
        Version::R2010,
        Version::R2013,
        Version::R2018,
    ] {
        let _ = HeaderVars::parse_lossy(data, version);
        let _ = HeaderVars::parse_strict(data, version);
    }
});
