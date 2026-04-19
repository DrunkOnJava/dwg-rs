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

    #[error("Reserved bit pattern \"{pattern}\" encountered in {code_type} (spec says \"not used\")")]
    ReservedBitPattern {
        code_type: &'static str,
        pattern: &'static str,
    },

    #[error("Invalid UTF-8 in text field: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}
