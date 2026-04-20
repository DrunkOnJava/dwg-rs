#![no_main]
use dwg::classes::ClassMap;
use dwg::version::Version;
use libfuzzer_sys::fuzz_target;

// Fuzz target: ClassMap::parse must never panic on any byte slice.
// We exercise all 8 supported versions so version-conditional parse
// branches are covered.
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
        let _ = ClassMap::parse(data, version);
    }
});
