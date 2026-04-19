#![no_main]
use dwg::Version;
use libfuzzer_sys::fuzz_target;

// Fuzz target: object walker on arbitrary AcDbObjects bytes. First byte
// picks the version to simulate real-world version-specific parsing
// branches; the rest is the object-stream payload.
fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let version = match data[0] & 0x07 {
        0 => Version::R14,
        1 => Version::R2000,
        2 => Version::R2004,
        3 => Version::R2007,
        4 => Version::R2010,
        5 => Version::R2013,
        _ => Version::R2018,
    };
    let payload = &data[1..];
    let mut walker = dwg::ObjectWalker::new(payload, version);
    // Bound the work to prevent runaway iteration.
    for _ in 0..1024 {
        match walker.next() {
            Some(Ok(_)) => continue,
            Some(Err(_)) | None => break,
        }
    }
});
