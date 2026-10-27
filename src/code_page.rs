//! The code page of a pre-R2007 drawing's text (ODA v5.4.1 §3.2.5).
//!
//! Before R2007 every `TV` is a run of single bytes in the drawing's code
//! page, and the file header says which: "Bytes at 0x13 and 0x14 are a raw
//! short indicating the value of the code page for this drawing file"
//! (§3.2.5). The specification prints no list of those values, so the table
//! here holds only the numbers AutoCAD's own output has named: the header
//! short of a pinned drawing against the `$DWGCODEPAGE` of its DXF export
//! (`examples/probe_code_page.rs`, ARCHITECTURE.md §7ap). A number outside
//! the table is [`CodePage::Unknown`] and its text is read as it was before
//! the table existed, as UTF-8 with U+FFFD for what is not; nothing is
//! guessed for it, and a probe can list the strings that took that path
//! ([`observe`]).
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
/// Only the values measured against AutoCAD's `$DWGCODEPAGE` are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodePage {
    /// Header short 29, `$DWGCODEPAGE` `ANSI_1251`: Windows-1251 (Cyrillic).
    Ansi1251,
    /// Header short 30, `$DWGCODEPAGE` `ANSI_1252`: Windows-1252 (Western European).
    Ansi1252,
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
            29 => Self::Ansi1251,
            30 => Self::Ansi1252,
            other => Self::Unknown(other),
        }
    }

    /// The raw header short (§3.2.5) this code page stands for.
    pub const fn header_value(self) -> u16 {
        match self {
            Self::Ansi1251 => 29,
            Self::Ansi1252 => 30,
            Self::Unknown(n) => n,
        }
    }

    /// The DXF `$DWGCODEPAGE` name AutoCAD writes for this header short, or
    /// `None` for [`CodePage::Unknown`].
    pub const fn dxf_name(self) -> Option<&'static str> {
        match self {
            Self::Ansi1251 => Some("ANSI_1251"),
            Self::Ansi1252 => Some("ANSI_1252"),
            Self::Unknown(_) => None,
        }
    }

    fn encoding(self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            Self::Ansi1251 => Some(encoding_rs::WINDOWS_1251),
            Self::Ansi1252 => Some(encoding_rs::WINDOWS_1252),
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
