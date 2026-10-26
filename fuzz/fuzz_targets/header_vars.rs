#![no_main]
use dwg::header_vars::HeaderVars;
use dwg::limits::ParseLimits;
use dwg::version::Version;
use libfuzzer_sys::fuzz_target;

const SENTINEL: [u8; 16] = HeaderVars::SENTINEL;
const END_SENTINEL: [u8; 16] = [
    0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC, 0xA0,
];

// Fuzz target: the AcDb:Header decoder must never panic on any byte slice.
// The section is a bit-packed stream of ~300 slots across three streams
// from R2007 (data, string, handle), so arbitrary input is mostly refused
// at the frame. Each input is therefore also wrapped in a well-formed frame
// (both sentinels, a size that fits, the CRC the section would carry) for
// every release, which reaches the slot decoder, the string-stream trailer
// and the handle stream with the fuzzer's bytes.
fuzz_target!(|data: &[u8]| {
    let limits = ParseLimits::paranoid();
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
        let _ = HeaderVars::decode(data, version, &limits);

        let (bits, unknown) = match version {
            Version::R14 | Version::R2000 | Version::R2004 => (false, false),
            Version::R2007 => (true, false),
            _ => (true, true),
        };
        let words = if bits { 4 } else { 0 };
        let size = (words + data.len()) as u32;
        let mut section = Vec::with_capacity(data.len() + 64);
        section.extend_from_slice(&SENTINEL);
        section.extend_from_slice(&size.to_le_bytes());
        if unknown {
            section.extend_from_slice(&0u32.to_le_bytes());
        }
        if bits {
            let claimed = (data.len() as u32).saturating_mul(8).saturating_add(32);
            section.extend_from_slice(&claimed.to_le_bytes());
        }
        section.extend_from_slice(data);
        let crc = dwg::crc::crc8(0xC0C1, &section[16..]);
        section.extend_from_slice(&crc.to_le_bytes());
        section.extend_from_slice(&END_SENTINEL);
        let _ = HeaderVars::decode(&section, version, &limits);
        let _ = HeaderVars::parse_lossy(&section, version);
    }
});
