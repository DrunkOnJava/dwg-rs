//! The code page of a pre-R2007 drawing's text (ODA v5.4.1 §3.2.5).
//!
//! Before R2007 every `TV` is a run of bytes in the drawing's code page (one byte
//! a character, or one or two on a double-byte page), and the file header says
//! which: "Bytes at 0x13 and 0x14 are a raw short indicating the value of the
//! code page for this drawing file" (§3.2.5). The specification prints no list
//! of those values, and they are not Windows code page numbers (29 and 30 are
//! 1251 and 1252, 38 is 932, 39 is 936): they are AutoCAD's own enumeration. The
//! table here holds the numbers real drawings have named, each by the evidence
//! ARCHITECTURE.md §7au records: the `$DWGCODEPAGE` of AutoCAD's DXF export of the
//! same drawing, or a known text (a localized layout name, a Greek word) found in
//! the drawing's bytes in the page. A number outside the table is
//! [`CodePage::Unknown`] and its text is read as it was before the table existed,
//! as UTF-8 with U+FFFD for what is not; nothing is guessed for it, and a probe can
//! list the strings that took that path ([`observe`]). A row is added by a drawing
//! that proves it, in the same pull request (`examples/probe_code_page.rs`).
//!
//! # One decode path
//!
//! [`CodePage::decode`] is the only place a pre-R2007 byte string becomes a
//! `String`. The per-record decoders reach it through the page in force for
//! the record: [`crate::object::RawObject::code_page`] is stamped by the walker
//! from the file header, [`crate::entities::decode_from_raw`] puts it in force
//! with [`with_code_page`] for the length of the decode, and the shared `TV`
//! readers read it back with [`active`]. [`crate::reader::DwgFile`] does the
//! same around the header variables, the class table and the metadata
//! sections. A decoder called on a bare cursor, outside both, sees
//! [`CodePage::default`], which keeps the old behaviour.
//!
//! R2007 and later store text as UTF-16 in the string stream and never come
//! here.

use std::cell::{Cell, RefCell};

/// A drawing's text code page, as named by the header short at 0x13 (§3.2.5).
///
/// The shorts are AutoCAD's own enumeration of code pages, not Windows code page
/// numbers (29 and 30 are 1251 and 1252, but 38 is 932 and 39 is 936). Only the
/// values measured on real drawings are named (ARCHITECTURE.md §7au).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodePage {
    /// Header short 28, `$DWGCODEPAGE` `ANSI_1250`: Windows-1250 (Central European).
    Ansi1250,
    /// Header short 29, `$DWGCODEPAGE` `ANSI_1251`: Windows-1251 (Cyrillic).
    Ansi1251,
    /// Header short 30, `$DWGCODEPAGE` `ANSI_1252`: Windows-1252 (Western European).
    Ansi1252,
    /// Header short 32, `$DWGCODEPAGE` `ANSI_1253`: Windows-1253 (Greek).
    Ansi1253,
    /// Header short 33, `$DWGCODEPAGE` `ANSI_1254`: Windows-1254 (Turkish).
    Ansi1254,
    /// Header short 36, `$DWGCODEPAGE` `ANSI_1257`: Windows-1257 (Baltic).
    Ansi1257,
    /// Header short 38, `$DWGCODEPAGE` `ANSI_932`: Shift_JIS, Windows code page 932 (Japanese, double byte).
    Ansi932,
    /// Header short 39, `$DWGCODEPAGE` `ANSI_936`: GBK, Windows code page 936 (Simplified Chinese, double byte).
    Ansi936,
    /// Header short 40, `$DWGCODEPAGE` `ANSI_949`: EUC-KR, Windows code page 949 (Korean, double byte).
    Ansi949,
    /// Header short 41, `$DWGCODEPAGE` `ANSI_950`: Big5, Windows code page 950 (Traditional Chinese, double byte).
    Ansi950,
    /// A header short that no measured drawing names (or none stated, as
    /// [`CodePage::default`] holds 0): its text is not decoded in a code page.
    Unknown(u16),
}

impl Default for CodePage {
    fn default() -> Self {
        Self::Unknown(0)
    }
}

impl CodePage {
    /// The code page for the raw short at 0x13 of the file header (§3.2.5).
    pub const fn from_header(short: u16) -> Self {
        match short {
            28 => Self::Ansi1250,
            29 => Self::Ansi1251,
            30 => Self::Ansi1252,
            32 => Self::Ansi1253,
            33 => Self::Ansi1254,
            36 => Self::Ansi1257,
            38 => Self::Ansi932,
            39 => Self::Ansi936,
            40 => Self::Ansi949,
            41 => Self::Ansi950,
            other => Self::Unknown(other),
        }
    }

    /// The raw header short (§3.2.5) this code page stands for.
    pub const fn header_value(self) -> u16 {
        match self {
            Self::Ansi1250 => 28,
            Self::Ansi1251 => 29,
            Self::Ansi1252 => 30,
            Self::Ansi1253 => 32,
            Self::Ansi1254 => 33,
            Self::Ansi1257 => 36,
            Self::Ansi932 => 38,
            Self::Ansi936 => 39,
            Self::Ansi949 => 40,
            Self::Ansi950 => 41,
            Self::Unknown(n) => n,
        }
    }

    /// The DXF `$DWGCODEPAGE` name AutoCAD writes for this header short, or
    /// `None` for [`CodePage::Unknown`].
    pub const fn dxf_name(self) -> Option<&'static str> {
        match self {
            Self::Ansi1250 => Some("ANSI_1250"),
            Self::Ansi1251 => Some("ANSI_1251"),
            Self::Ansi1252 => Some("ANSI_1252"),
            Self::Ansi1253 => Some("ANSI_1253"),
            Self::Ansi1254 => Some("ANSI_1254"),
            Self::Ansi1257 => Some("ANSI_1257"),
            Self::Ansi932 => Some("ANSI_932"),
            Self::Ansi936 => Some("ANSI_936"),
            Self::Ansi949 => Some("ANSI_949"),
            Self::Ansi950 => Some("ANSI_950"),
            Self::Unknown(_) => None,
        }
    }

    fn encoding(self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            Self::Ansi1250 => Some(encoding_rs::WINDOWS_1250),
            Self::Ansi1251 => Some(encoding_rs::WINDOWS_1251),
            Self::Ansi1252 => Some(encoding_rs::WINDOWS_1252),
            Self::Ansi1253 => Some(encoding_rs::WINDOWS_1253),
            Self::Ansi1254 => Some(encoding_rs::WINDOWS_1254),
            Self::Ansi1257 => Some(encoding_rs::WINDOWS_1257),
            Self::Ansi932 => Some(encoding_rs::SHIFT_JIS),
            Self::Ansi936 => Some(encoding_rs::GBK),
            Self::Ansi949 => Some(encoding_rs::EUC_KR),
            Self::Ansi950 => Some(encoding_rs::BIG5),
            Self::Unknown(_) => None,
        }
    }

    /// `bytes` as text in this code page, or `None` when they leave 7-bit
    /// ASCII and the page is [`CodePage::Unknown`]. ASCII reads the same in
    /// every page, so it is always `Some`.
    pub fn try_decode(self, bytes: &[u8]) -> Option<String> {
        let text = if bytes.is_ascii() {
            Some(String::from_utf8_lossy(bytes).into_owned())
        } else {
            self.encoding()
                .map(|encoding| encoding.decode_without_bom_handling(bytes).0.into_owned())
        };
        SINK.with(|sink| {
            if let Some(seen) = sink.borrow_mut().as_mut() {
                seen.push(Observed {
                    page: self,
                    bytes: bytes.to_vec(),
                    text: text.clone(),
                });
            }
        });
        text
    }

    /// The bytes this code page holds `c` as, for a character the page can hold:
    /// `Some` (one byte, or two on a double-byte page) for ASCII under every page
    /// and for the characters of a named page, the inverse of
    /// [`decode`](Self::decode) (the bytes are returned only when they decode back to
    /// `c`, which excludes the WHATWG encoders' extra mappings, such as U+00A5 to
    /// 0x5C on Shift_JIS); `None` for a character outside the page and for
    /// every character above ASCII under [`CodePage::Unknown`].
    pub fn encode_char(self, c: char) -> Option<Vec<u8>> {
        if c.is_ascii() {
            return Some(vec![u8::try_from(u32::from(c)).ok()?]);
        }
        let mut buf = [0u8; 4];
        let (bytes, _, unmappable) = self.encoding()?.encode(c.encode_utf8(&mut buf));
        if unmappable {
            return None;
        }
        let bytes = bytes.into_owned();
        self.decode(&bytes).chars().eq([c]).then_some(bytes)
    }

    /// `text` as the bytes a pre-R2007 writer stores: each character the page
    /// holds as its bytes ([`encode_char`](Self::encode_char)), every other
    /// character as the escape `\U+XXXX` (four uppercase hexadecimal digits of each
    /// UTF-16 unit) that AutoCAD's own R2000 and R2004 saves hold for a character
    /// their page cannot (ARCHITECTURE.md §7at). The result is ASCII under
    /// [`CodePage::Unknown`], where no character is known to be in the page. Text
    /// that is already such an escape, as MTEXT's `\U+00B0` is, passes unchanged:
    /// a backslash is ASCII.
    pub fn encode(self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        let mut units = [0u16; 2];
        for c in text.chars() {
            match self.encode_char(c) {
                Some(bytes) => out.extend_from_slice(&bytes),
                None => {
                    for unit in c.encode_utf16(&mut units) {
                        out.extend_from_slice(format!("\\U+{unit:04X}").as_bytes());
                    }
                }
            }
        }
        out
    }

    /// `bytes` as text in this code page; for a byte above 0x7F under
    /// [`CodePage::Unknown`], as UTF-8 with U+FFFD for what is not.
    pub fn decode(self, bytes: &[u8]) -> String {
        self.try_decode(bytes)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
    }
}

thread_local! {
    static ACTIVE: Cell<CodePage> = const { Cell::new(CodePage::Unknown(0)) };
}

/// Runs `f` with `page` as the code page of every pre-R2007 string decoded on
/// this thread, then restores the previous one, also when `f` panics.
pub fn with_code_page<R>(page: CodePage, f: impl FnOnce() -> R) -> R {
    struct Restore(CodePage);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(ACTIVE.with(|c| c.replace(page)));
    f()
}

/// The code page [`with_code_page`] has in force on this thread, or
/// [`CodePage::default`] outside it.
pub fn active() -> CodePage {
    ACTIVE.with(Cell::get)
}

/// The pre-R2007 string `bytes` in the code page in force ([`active`]).
pub(crate) fn decode_active(bytes: &[u8]) -> String {
    active().decode(bytes)
}

/// The pre-R2007 string `bytes` in the code page in force, or `None` when
/// they need a page the table does not name ([`CodePage::try_decode`]).
pub(crate) fn try_decode_active(bytes: &[u8]) -> Option<String> {
    active().try_decode(bytes)
}

thread_local! {
    static SINK: RefCell<Option<Vec<Observed>>> = const { RefCell::new(None) };
}

/// A pre-R2007 string, as [`observe`] saw it decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// The code page in force.
    pub page: CodePage,
    /// The string's bytes, as the file holds them.
    pub bytes: Vec<u8>,
    /// The text in [`page`](Self::page), or `None` for a page the table does
    /// not name ([`CodePage::Unknown`]).
    pub text: Option<String>,
}

/// Runs `f` and returns what it returned with every pre-R2007 string decoded
/// on this thread while it ran, ASCII included, in order. For a probe that sets
/// the strings a drawing holds beside AutoCAD's own export.
pub fn observe<R>(f: impl FnOnce() -> R) -> (R, Vec<Observed>) {
    struct Restore(Option<Vec<Observed>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SINK.with(|sink| *sink.borrow_mut() = self.0.take());
        }
    }
    let restore = Restore(SINK.with(|sink| sink.borrow_mut().replace(Vec::new())));
    let result = f();
    let seen = SINK
        .with(|sink| sink.borrow_mut().take())
        .unwrap_or_default();
    drop(restore);
    (result, seen)
}
