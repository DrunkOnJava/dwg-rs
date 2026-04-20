use std::io;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Not a DWG file: expected ASCII magic \"AC10xx\" at offset 0, got {got:?}")]
    NotDwg { got: [u8; 6] },

    #[error("Unsupported DWG version: {0:?} (known: AC1014 .. AC1032)")]
    UnsupportedVersion([u8; 6]),

    #[error("File truncated: wanted {wanted} bytes at offset {offset}, file is {len} bytes")]
    Truncated {
        offset: u64,
        wanted: usize,
        len: u64,
    },

    #[error("Bit cursor exhausted: wanted {wanted} bits, {remaining} bits remain")]
    BitsExhausted { wanted: usize, remaining: usize },

    #[error("CRC mismatch at {context}: expected {expected:#06x}, got {actual:#06x}")]
    CrcMismatch {
        context: String,
        expected: u16,
        actual: u16,
    },

    #[error("R2004+ header decrypt failed: {0}")]
    R2004Decrypt(String),

    #[error("Section locator malformed: {0}")]
    SectionLocator(String),

    #[error(
        "Reserved bit pattern \"{pattern}\" encountered in {code_type} (spec says \"not used\")"
    )]
    ReservedBitPattern {
        code_type: &'static str,
        pattern: &'static str,
    },

    #[error("Invalid UTF-8 in text field: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("LZ77 stream truncated (spec §4.7)")]
    Lz77Truncated,

    #[error("LZ77 back-reference points before start of output (spec §4.7)")]
    Lz77InvalidOffset,

    #[error(
        "LZ77 reserved opcode 0x{opcode:02X} at input position {pos} (spec §4.7: 0x00-0x0F not used), output len = {out_len}"
    )]
    Lz77InvalidOpcode {
        opcode: u8,
        pos: usize,
        out_len: usize,
    },

    #[error("LZ77 literal-only encoder cannot emit {0} bytes (valid: 0 or >=4, gap at 1..=3)")]
    Lz77UnencodableLength(usize),

    #[error(
        "LZ77 decompressed output exceeded configured limit \
         ({limit} bytes; decompression-bomb defense per SECURITY.md)"
    )]
    Lz77OutputLimitExceeded { limit: usize },

    #[error(
        "LZ77 back-reference copy length ({length}) exceeds configured limit \
         ({limit}); malformed or adversarial compressed stream"
    )]
    Lz77BackrefTooLong { length: usize, limit: usize },

    #[error("Section map parse failed: {0}")]
    SectionMap(String),

    /// The BLL encoding (`spec §2.4`) uses a 3-bit prefix-coded length
    /// whose representable set is `{0, 2, 6, 7}` bytes. Values in the
    /// top byte of a `u64` (`v >= 1 << 56`) cannot fit in the largest
    /// allowed length (7 bytes) and must be rejected at write time.
    #[error(
        "BLL value 0x{value:016X} requires more than 56 bits; BLL encoding \
         caps at 7 bytes (spec §2.4)"
    )]
    BllOverflow { value: u64 },

    /// The writer was asked to emit a 3B prefix-coded value outside the
    /// representable set `{0, 2, 6, 7}` (spec §2.1). Internal callers
    /// normalize upstream; this variant surfaces the programmer error
    /// without a panic.
    #[error("invalid 3B value {value}; representable: {{0, 2, 6, 7}} (spec §2.1)")]
    Invalid3B { value: u8 },

    /// A feature is known to exist in the file format but is not yet
    /// implemented by this crate. Surfaces instead of producing
    /// misaligned output from a best-effort partial decode.
    #[error("unsupported feature: {feature}")]
    Unsupported { feature: String },

    /// A single object in the handle-driven object walk could not be
    /// parsed. Surfaced only by [`crate::object::ObjectWalker::collect_all_strict`];
    /// the lossy variant records these in a summary instead.
    #[error(
        "object walk: record at offset {offset} (handle 0x{handle:X}) \
         failed to parse: {reason}"
    )]
    ObjectWalk {
        handle: u64,
        offset: u64,
        reason: String,
    },
}
