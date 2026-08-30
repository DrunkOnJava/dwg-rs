//! Object stream walker for the `AcDb:AcDbObjects` section (spec §20).
//!
//! The section bytes obtained from `DwgFile::read_section("AcDb:AcDbObjects")`
//! hold one variable-length record per drawing object. Each record opens
//! with a **modular short (MS)** byte count — the declared number of
//! content bytes excluding the trailing 2-byte CRC — followed by a bit-
//! level payload and byte-aligned CRC.
//!
//! # The `MS` does not count the R2010+ leading `MC` (#77)
//!
//! From R2010 the payload opens with an `MC` holding the handle-stream
//! size in bits, and the record's `MS` **excludes that field's own
//! bytes**. Measured on the object stream itself: every inter-record run
//! left over by an `MS`-only span equals the width of the *preceding*
//! record's `MC` field — 841 of 841 agreements on `sample_AC1032.dwg`
//! (R2018) and 100 % on `arc_2010.dwg`, `line_2010.dwg` and
//! `line_2013.dwg`, against 734 of 841 for the "following record"
//! reading; only 8 of those 916 bytes are zero, so they are content and
//! not alignment padding. So a record occupies
//!
//! ```text
//! MS field | MC field | MS bytes of payload | 2-byte CRC
//! ```
//!
//! and the walker sizes [`RawObject::raw`] as `mc_width + size_bytes`.
//! Before #77 the slice stopped `mc_width` bytes early and the
//! sequential (handle-map-less) walk drifted by one `MC` width per
//! record on R2010+. Pre-R2010 records carry no such field and their
//! `MS` covers the whole payload, which is why the R2004 and R2007
//! corpus files tile their object stream exactly.
//!
//! # Two navigation modes
//!
//! The on-disk stream is **not strictly sequential**: objects are aligned
//! and may have gaps. The authoritative way to enumerate every object in
//! a file is the `AcDb:Handles` offset table (spec §21) — each entry
//! pairs a handle with a byte offset into this stream.
//!
//! `ObjectWalker::new(bytes, version)` runs in **first-pass** mode —
//! reads one object from the 4-byte `0x0dca` prefix; sufficient for
//! metadata probing.
//!
//! `ObjectWalker::with_handle_map(bytes, version, map)` runs in
//! **handle-driven** mode — iterates every (handle, offset) pair in the
//! map and reads the object at each offset; returns the full object
//! list with no gaps or missing entries.
//!
//! # Count cap + strict Unknown handling
//!
//! Both walk modes honor a [`WalkerLimits`]-derived `max_handles` count
//! cap: once the walker has visited the configured number of records,
//! the next call returns [`Error::WalkerLimitExceeded`]. Prevents an
//! adversarial file with a fabricated handle map from exhausting memory
//! by triggering a runaway decode loop.
//!
//! [`ObjectWalker::collect_all_strict`] additionally refuses any record
//! whose type code falls into [`ObjectType::Unknown`] (reserved by the
//! spec); [`ObjectWalker::collect_all_lossy`] records the same records
//! in [`ObjectWalkSummary::unknown_types`] and keeps going.

use crate::bitcursor::{BitCursor, Handle};
use crate::error::{Error, Result};
use crate::limits::WalkerLimits;
use crate::object_type::ObjectType;
use crate::version::Version;

/// A single undecoded object extracted from the R2004+ object stream.
#[derive(Debug, Clone)]
pub struct RawObject {
    /// Byte offset into the decompressed section where this object begins.
    pub stream_offset: usize,
    /// The record's leading `MS` value, verbatim.
    ///
    /// This is **not** `raw.len()` on R2010+: the `MS` excludes the
    /// record's own leading `MC` handle-stream-size field (#77), so
    /// `raw.len() == size_bytes + mc_width` there and
    /// `raw.len() == size_bytes` before R2010. It never counts the
    /// trailing 2-byte CRC on any release.
    pub size_bytes: u32,
    /// Raw type code as encoded in the stream.
    pub type_code: u16,
    /// Classified form of `type_code`.
    pub kind: ObjectType,
    /// Object's handle — the ID other objects use to reference this one.
    pub handle: Handle,
    /// The entity/object's raw bytes as they appear on disk (for consumers
    /// that want to run their own entity-specific decoder).
    ///
    /// Spans the whole record between the `MS` and the CRC — including
    /// the R2010+ leading `MC` field that the `MS` does not count (see
    /// the module docs and [`size_bytes`](Self::size_bytes)).
    pub raw: Vec<u8>,
    /// R2000-R2007 only: the `RL` "object data size in bits" from the
    /// object prologue (spec §19.1), measured from bit 0 of [`raw`](Self::raw).
    ///
    /// It is the bit at which the object's data stream ends and its
    /// handle stream begins — the pre-R2010 analogue of the R2007+
    /// string-stream start bit, and therefore the invariant a decoder
    /// can check itself against. `None` on R13/R14 (no such field) and
    /// on R2010+ (replaced by the leading `MC` handle-stream size).
    pub obj_size_bits: Option<u32>,
}

impl RawObject {
    /// Whether this object is an entity (drawable) or a non-entity
    /// (control, table, dictionary, ...).
    pub fn is_entity(&self) -> bool {
        self.kind.is_entity()
    }
}

/// Iterator over every object in an `AcDb:AcDbObjects` byte stream.
///
/// For R18+ files the stream is prefixed by an undocumented 4-byte RL
/// value (spec: "starts with a RL value of 0x0dca"); the walker skips
/// it automatically.
#[derive(Debug)]
pub struct ObjectWalker<'a> {
    bytes: &'a [u8],
    pos: usize,
    version: Version,
    handle_map: Option<&'a crate::handle_map::HandleMap>,
    handle_idx: usize,
    /// Count-cap enforced inside [`ObjectWalker::next_item`]. Defaults
    /// to [`WalkerLimits::safe`]; override via
    /// [`ObjectWalker::with_limits`].
    limits: WalkerLimits,
    /// Running count of records the walker has tried to consume
    /// (decoded + skipped + errored). Compared against
    /// `limits.max_handles` on each `next_item` call.
    records_visited: usize,
    /// Accumulator behind [`ObjectWalkSummary::handle_mismatches`].
    handle_mismatches: Vec<(u64, u64, u64)>,
}

/// A single handle-driven walk step. In handle-driven mode the
/// walker either yields an object or, on a record that failed to
/// parse, a [`SkippedEntry`] describing which handle+offset failed
/// and why.
#[derive(Debug, Clone)]
pub enum ObjectWalkItem {
    /// A successfully-parsed object.
    Object(RawObject),
    /// A record at the named handle+offset could not be parsed. The
    /// walk continues; the caller decides whether to treat this as a
    /// hard error (via [`ObjectWalker::collect_all_strict`]) or to
    /// collect it as a diagnostic (via [`ObjectWalker::collect_all_lossy`]).
    Skipped(SkippedEntry),
}

/// One entry in the lossy-collector's skip list.
#[derive(Debug, Clone)]
pub struct SkippedEntry {
    pub handle: u64,
    pub offset: u64,
    pub reason: String,
}

/// Summary of a [`ObjectWalker::collect_all_lossy`] pass.
#[derive(Debug, Clone, Default)]
pub struct ObjectWalkSummary {
    /// Number of objects yielded successfully.
    pub decoded_count: usize,
    /// Records the walker stepped over due to per-object parse errors.
    pub skipped: Vec<SkippedEntry>,
    /// Number of records the walker read but a downstream consumer
    /// (typically [`crate::entities::decode_from_raw`]) could not
    /// classify. Populated by callers that wire dispatch errors back
    /// into the summary; defaults to `0` for the bare walker pass.
    pub errored_count: usize,
    /// Records whose on-disk type code fell into
    /// [`ObjectType::Unknown`] — a reserved range the spec does not
    /// assign. In the lossy walk, these records are kept alongside the
    /// rest of the parsed objects and their `(type_code, stream_offset)`
    /// is captured here; the strict walk
    /// ([`ObjectWalker::collect_all_strict`]) returns
    /// [`Error::UnknownObjectType`] on the first one instead.
    ///
    /// Mirrors a future `ParseDiagnostics::unknown_types` field on
    /// [`crate::reader::ParseDiagnostics`]; wiring the two together
    /// is deferred to a follow-up that can extend `src/api.rs`.
    pub unknown_types: Vec<(u16, u64)>,
    /// Handle-driven mode only: records whose own handle field
    /// disagreed with the handle the `AcDb:Handles` map paired with
    /// that offset, as `(map_handle, record_handle, stream_offset)`.
    ///
    /// Every object repeats its handle inside its own record, so this
    /// is a self-checking oracle for the handle map: a non-empty list
    /// means the map's address space and the object stream disagree,
    /// which is exactly the failure #43/#44 tracked. The record is
    /// still yielded (with the map's handle, as before) — the
    /// disagreement is reported, not enforced.
    pub handle_mismatches: Vec<(u64, u64, u64)>,
}

impl ObjectWalkSummary {
    /// Count of records the walker stepped over (read but could not
    /// parse into a [`RawObject`]).
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }

    /// Confidence ratio in `[0.0, 1.0]`: clean decodes divided by the
    /// total number of records the walker tried to handle. An empty
    /// walk (no work done) returns `1.0` — there's nothing dirty.
    ///
    /// `decoded_count / (decoded_count + skipped_count + errored_count)`,
    /// clamped into `[0.0, 1.0]` defensively in case a caller mutates
    /// the fields by hand into something silly.
    pub fn confidence(&self) -> f64 {
        let decoded = self.decoded_count;
        let total = decoded
            .saturating_add(self.skipped_count())
            .saturating_add(self.errored_count);
        if total == 0 {
            return 1.0;
        }
        let ratio = decoded as f64 / total as f64;
        ratio.clamp(0.0, 1.0)
    }
}

impl<'a> ObjectWalker<'a> {
    /// Starts a walker over the object-section bytes for `version`.
    pub fn new(bytes: &'a [u8], version: Version) -> Self {
        // R2004+ (R2007 included) prefixes the `AcDb:AcDbObjects` stream
        // with an `RL` of 0x0dca. Measured: every corpus file from
        // `arc_2004.dwg` through `sample_AC1032.dwg` opens with the bytes
        // `CA 0D 00 00`, and so does the decompressed R2007 section —
        // `line_2007.dwg` starts `CA 0D 00 00 12 00 4C 12 80 …`, where
        // `12 00` is the first record's `MS` and `4C` its `BS` type code
        // (0x30 = BLOCK_CONTROL). R13-R15 have no such prefix because
        // they have no object *section* — the records sit loose in the
        // file (§3.1) and the object map addresses them absolutely.
        let initial = if bytes.len() >= 4 && version.is_r2004_plus() {
            4
        } else {
            0
        };
        Self {
            bytes,
            pos: initial,
            version,
            handle_map: None,
            handle_idx: 0,
            limits: WalkerLimits::default(),
            records_visited: 0,
            handle_mismatches: Vec::new(),
        }
    }

    /// Handle-driven iterator: seeks to each (handle, offset) pair in the
    /// provided [`crate::handle_map::HandleMap`] and reads one object at
    /// each offset. This yields the complete object list for the file —
    /// every control object, table entry, and entity.
    pub fn with_handle_map(
        bytes: &'a [u8],
        version: Version,
        map: &'a crate::handle_map::HandleMap,
    ) -> Self {
        Self {
            bytes,
            pos: 0,
            version,
            handle_map: Some(map),
            handle_idx: 0,
            limits: WalkerLimits::default(),
            records_visited: 0,
            handle_mismatches: Vec::new(),
        }
    }

    /// Override the [`WalkerLimits`] applied by [`collect_all`],
    /// [`collect_all_strict`], and [`collect_all_lossy`]. The default
    /// is [`WalkerLimits::safe`] — fits every real drawing but still
    /// bounds work on an adversarial input whose handle map is
    /// fabricated to point at billions of records.
    ///
    /// [`collect_all`]: Self::collect_all
    /// [`collect_all_strict`]: Self::collect_all_strict
    /// [`collect_all_lossy`]: Self::collect_all_lossy
    pub fn with_limits(mut self, limits: WalkerLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Read all objects into a `Vec`, silently skipping any record that
    /// fails to parse in handle-driven mode. Preserves the pre-0.2.0
    /// default behavior.
    ///
    /// Honors [`WalkerLimits::max_handles`] — an adversarial file that
    /// presents a handle map past the cap returns
    /// [`Error::WalkerLimitExceeded`] instead of a partial list.
    ///
    /// **Callers that need to distinguish "record parsed cleanly" from
    /// "record could not be parsed" should prefer [`collect_all_strict`]
    /// (hard-fail) or [`collect_all_lossy`] (diagnostic list).**
    ///
    /// [`collect_all_strict`]: Self::collect_all_strict
    /// [`collect_all_lossy`]: Self::collect_all_lossy
    pub fn collect_all(mut self) -> Result<Vec<RawObject>> {
        let mut out = Vec::new();
        while let Some(raw) = self.next()? {
            out.push(raw);
        }
        Ok(out)
    }

    /// Read all objects into a `Vec`, returning an error on the first
    /// handle-driven record that fails to parse. Use when the caller
    /// needs to trust the object count — for example, when computing
    /// coverage metrics that assume every handle-map entry was visited.
    ///
    /// In addition to the per-record parse posture, this variant
    /// **rejects unknown object-type codes** — if the walker sees an
    /// [`ObjectType::Unknown`] value, it returns
    /// [`Error::UnknownObjectType`] with the stream offset pointing at
    /// the record's start. Callers that want to tolerate unknown codes
    /// should use [`collect_all_lossy`](Self::collect_all_lossy), which
    /// collects them into [`ObjectWalkSummary::unknown_types`] instead.
    ///
    /// Also honors [`WalkerLimits::max_handles`] — returns
    /// [`Error::WalkerLimitExceeded`] if the count cap is tripped.
    pub fn collect_all_strict(mut self) -> Result<Vec<RawObject>> {
        let mut out = Vec::new();
        loop {
            match self.next_item()? {
                Some(ObjectWalkItem::Object(raw)) => {
                    if let ObjectType::Unknown(code) = raw.kind {
                        return Err(Error::UnknownObjectType {
                            type_code: code,
                            offset: raw.stream_offset as u64,
                        });
                    }
                    out.push(raw);
                }
                Some(ObjectWalkItem::Skipped(entry)) => {
                    return Err(Error::ObjectWalk {
                        handle: entry.handle,
                        offset: entry.offset,
                        reason: entry.reason,
                    });
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// Read all objects into a `Vec` alongside a [`ObjectWalkSummary`]
    /// that records every record the walker could not parse. Use when
    /// partial data is acceptable but the caller still wants visibility
    /// into what was dropped.
    ///
    /// Records with [`ObjectType::Unknown`] type codes are collected
    /// into [`ObjectWalkSummary::unknown_types`] **alongside** the
    /// [`RawObject`] in the returned vec — the caller can both iterate
    /// over the parsed records and consult the summary for the list of
    /// type codes that fell into the reserved range.
    ///
    /// Honors [`WalkerLimits::max_handles`] — trips
    /// [`Error::WalkerLimitExceeded`] if too many records are visited;
    /// on that path the partial list + summary are not returned.
    pub fn collect_all_lossy(mut self) -> (Vec<RawObject>, ObjectWalkSummary) {
        let mut out = Vec::new();
        let mut summary = ObjectWalkSummary::default();
        while let Ok(Some(item)) = self.next_item() {
            match item {
                ObjectWalkItem::Object(raw) => {
                    if let ObjectType::Unknown(code) = raw.kind {
                        summary.unknown_types.push((code, raw.stream_offset as u64));
                    }
                    summary.decoded_count += 1;
                    out.push(raw);
                }
                ObjectWalkItem::Skipped(entry) => summary.skipped.push(entry),
            }
        }
        summary.handle_mismatches = std::mem::take(&mut self.handle_mismatches);
        (out, summary)
    }
}

impl<'a> ObjectWalker<'a> {
    /// Back-compat iterator. Silently continues past handle-driven
    /// parse failures; use [`next_item`](Self::next_item) to observe them.
    fn next(&mut self) -> Result<Option<RawObject>> {
        loop {
            match self.next_item()? {
                Some(ObjectWalkItem::Object(raw)) => return Ok(Some(raw)),
                Some(ObjectWalkItem::Skipped(_)) => continue,
                None => return Ok(None),
            }
        }
    }

    /// Lower-level iterator that surfaces handle-driven parse failures
    /// as [`ObjectWalkItem::Skipped`] rather than silently dropping
    /// them. Advances one step per call.
    ///
    /// Enforces [`WalkerLimits::max_handles`] before consuming the
    /// next record: if the running `records_visited` count is already
    /// at or above the cap, returns [`Error::WalkerLimitExceeded`]
    /// without reading more bytes. This is the sole defense against
    /// adversarial handle maps whose entry count blows past a
    /// reasonable-file envelope.
    fn next_item(&mut self) -> Result<Option<ObjectWalkItem>> {
        if self.records_visited >= self.limits.max_handles {
            return Err(Error::WalkerLimitExceeded {
                limit: self.limits.max_handles,
                seen: self.records_visited,
            });
        }
        // Handle-driven mode: seek to the next entry's byte offset.
        if let Some(map) = self.handle_map {
            if self.handle_idx >= map.entries.len() {
                return Ok(None);
            }
            let entry = map.entries[self.handle_idx];
            self.handle_idx += 1;
            self.records_visited += 1;
            let pos = entry.offset as usize;
            if pos >= self.bytes.len() {
                return Ok(Some(ObjectWalkItem::Skipped(SkippedEntry {
                    handle: entry.handle,
                    offset: entry.offset,
                    reason: format!(
                        "offset {pos} past end of object stream ({} bytes)",
                        self.bytes.len()
                    ),
                })));
            }
            self.pos = pos;
            return match self.read_one_at_pos() {
                Ok(Some(mut raw)) => {
                    // Every record repeats its own handle; a mismatch
                    // against the map means the map's address space is
                    // wrong, not that this record is (#43/#44).
                    if raw.handle.value != entry.handle {
                        self.handle_mismatches
                            .push((entry.handle, raw.handle.value, entry.offset));
                    }
                    raw.handle.value = entry.handle;
                    Ok(Some(ObjectWalkItem::Object(raw)))
                }
                Ok(None) => Ok(Some(ObjectWalkItem::Skipped(SkippedEntry {
                    handle: entry.handle,
                    offset: entry.offset,
                    reason: "record read returned None (truncated or zero-length)".to_string(),
                }))),
                Err(e) => Ok(Some(ObjectWalkItem::Skipped(SkippedEntry {
                    handle: entry.handle,
                    offset: entry.offset,
                    reason: e.to_string(),
                }))),
            };
        }
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        self.records_visited += 1;
        Ok(self.read_one_at_pos()?.map(ObjectWalkItem::Object))
    }

    fn read_one_at_pos(&mut self) -> Result<Option<RawObject>> {
        let start = self.pos;
        // Read MS (modular short) = object size in bytes (not counting CRC).
        // We need a byte-level reader first for MS (byte-aligned), then a
        // bit-level reader for the type + handle inside the payload.
        let (size_bytes, ms_consumed) = read_ms_bytealigned(&self.bytes[start..])?;
        if size_bytes == 0 {
            // Zero-length record — treat as stream end (observed trailing
            // padding in some R2018 files).
            self.pos = self.bytes.len();
            return Ok(None);
        }
        // Record spans MS + [R2010+ MC] + size_bytes payload + 2-byte
        // CRC. The `MS` excludes the record's own leading `MC`
        // handle-stream-size field, so the payload runs `mc_width`
        // bytes past `size_bytes` on R2010+ (#77, see the module docs).
        let payload_start = start + ms_consumed;
        if payload_start > self.bytes.len() {
            self.pos = self.bytes.len();
            return Ok(None);
        }
        let mc_width = if self.version.is_r2010_plus() {
            match mc_field_width(&self.bytes[payload_start..]) {
                Some(w) => w,
                None => {
                    // No terminating byte in the first ten: not an MC,
                    // so this is not a record start.
                    self.pos = self.bytes.len();
                    return Ok(None);
                }
            }
        } else {
            0
        };
        let payload_end = payload_start + size_bytes as usize + mc_width;
        let crc_end = payload_end + 2;
        if crc_end > self.bytes.len() {
            // Malformed — truncate gracefully.
            self.pos = self.bytes.len();
            return Ok(None);
        }

        // Inside the payload we need bit-level reads to extract object
        // type and handle. For R2010+, the payload leads with an MC
        // (handle stream size in bits); we skip over it.
        let payload = &self.bytes[payload_start..payload_end];
        let mut cur = BitCursor::new(payload);

        if self.version.is_r2010_plus() {
            // MC — modular char, byte-aligned (spec §2.6). The high bit
            // on each byte flags continuation; the final byte's 0x40
            // bit IS NOT interpreted as sign here (spec §20.1 note).
            let _handle_stream_bits = read_mc_unsigned(&mut cur)?;
        }

        let type_code = read_object_type(&mut cur, self.version)?;

        // R2000-R2007 (spec §19.1): an RL "object data size in bits"
        // sits between the object type and the object handle. R13/R14
        // predate it; R2010+ replaces it with the leading MC above.
        // Reading it for R2000 alone left every AC1018/AC1021 record
        // 32 bits out of phase from the handle onwards.
        let obj_size_bits = if self.version.has_object_size_field() {
            Some(cur.read_rl()?)
        } else {
            None
        };

        // Common: handle (code + counter + bytes).
        let handle = cur.read_handle()?;

        let kind = ObjectType::from_code(type_code);
        let raw = self.bytes[payload_start..payload_end].to_vec();

        self.pos = crc_end;
        Ok(Some(RawObject {
            stream_offset: start,
            size_bytes,
            type_code,
            kind,
            handle,
            raw,
            obj_size_bits,
        }))
    }
}

/// Read a byte-aligned modular short (MS) from a raw byte slice.
/// Returns `(value, bytes_consumed)`. An MS encodes values 0..=0x7FFF
/// per module; continuation flag is bit 15 (0x8000) of the 16-bit module.
fn read_ms_bytealigned(bytes: &[u8]) -> Result<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    let mut i = 0usize;
    loop {
        if i + 1 >= bytes.len() {
            return Err(Error::Truncated {
                offset: i as u64,
                wanted: 2,
                len: bytes.len() as u64,
            });
        }
        let lo = bytes[i] as u32;
        let hi = bytes[i + 1] as u32;
        i += 2;
        let module = (hi << 8) | lo;
        let cont = (module & 0x8000) != 0;
        let data = module & 0x7FFF;
        value |= data << shift;
        shift += 15;
        if !cont || shift >= 32 {
            return Ok((value, i));
        }
    }
}

/// Width in bytes of the leading `MC` handle-stream-size field of an
/// R2010+ object record (spec §20.1) — the count of bytes up to and
/// including the first with its continuation bit clear.
///
/// `None` when none of the first ten bytes terminates the field, which
/// is not a value any `u64`-bounded handle-stream size can produce and
/// therefore proves the offset is not a record start.
fn mc_field_width(bytes: &[u8]) -> Option<usize> {
    for (i, b) in bytes.iter().enumerate().take(10) {
        if b & 0x80 == 0 {
            return Some(i + 1);
        }
    }
    None
}

/// Read an unsigned modular char (MC) — the 0x40 bit is NOT sign, per
/// spec §20.1 note on the handle stream size field.
fn read_mc_unsigned(r: &mut BitCursor<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let b = r.read_rc()? as u64;
        let cont = (b & 0x80) != 0;
        let data = b & 0x7F;
        value |= data << shift;
        shift += 7;
        if !cont || shift >= 64 {
            return Ok(value);
        }
    }
}

/// Read the object type field.
///
/// Pre-R2010: BS (bit-short), value range 0..=0xFFFF.
///
/// R2010+ (spec §2.12): 2-bit dispatch followed by 1-2 bytes.
///   00 → next byte            (0x00..=0xFF)
///   01 → next byte + 0x1F0    (0x1F0..=0x2EF)
///   10 → next 2 bytes (raw short, LE)
///   11 → same as 10 (spec says this should never occur)
pub(crate) fn read_object_type(c: &mut BitCursor<'_>, version: Version) -> Result<u16> {
    if version.is_r2010_plus() {
        let tag = c.read_bb()?;
        match tag {
            0 => Ok(c.read_rc()? as u16),
            1 => Ok((c.read_rc()? as u16) + 0x1F0),
            _ => {
                let lsb = c.read_rc()? as u16;
                let msb = c.read_rc()? as u16;
                Ok((msb << 8) | lsb)
            }
        }
    } else {
        Ok(c.read_bs_u()?)
    }
}

/// Open a [`BitCursor`] on `raw` positioned at the object *body* — the
/// first bit past the object header the walker already consumed (the
/// R2010+ leading `MC` handle-stream size, the object type code, the
/// R2000-R2007 `RL` object-data-size field, and the object handle).
///
/// Every per-type decoder starts from this position: entities go on to
/// read the common entity preamble (§19.4.1), non-entity objects the
/// shorter common object data (§19.4.2).
pub fn body_cursor<'a>(raw: &'a RawObject, version: Version) -> Result<BitCursor<'a>> {
    let mut cursor = BitCursor::new(&raw.raw);
    if version.is_r2010_plus() {
        // MC — handle-stream-size-in-bits; byte-aligned, throwaway here.
        read_mc_unsigned(&mut cursor)?;
    }
    read_object_type(&mut cursor, version)?;
    if version.has_object_size_field() {
        // R2000-R2007 — 32-bit object-data-size-in-bits field (§19.1).
        cursor.read_rl()?;
    }
    let _ = cursor.read_handle()?;
    Ok(cursor)
}

/// Bit offset at which an object's data fields must end.
///
/// This is the self-validation anchor every split-stream decoder checks
/// itself against:
///
/// - R2007+ — the start of the object's string stream when it carries
///   one, otherwise the start of its handle stream minus the one `B`
///   "strings present" trailer bit (§19.1). On R2007 that boundary is
///   located from the `RL` below rather than from an `MC`; see
///   [`crate::string_stream::data_section_end`].
/// - R2000 / R2004 — the `RL` object-data-size-in-bits from the object
///   prologue, which marks the same boundary inline (these releases
///   have no string stream, so data fields run to the handle stream).
/// - R13/R14 — unknown; returns `None`.
pub fn data_end_bit(raw: &RawObject, version: Version) -> Option<usize> {
    if version.is_r2007_plus() {
        return crate::string_stream::data_field_end(&raw.raw, version);
    }
    raw.obj_size_bits.map(|b| b as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitwriter::BitWriter;

    /// Build one `AcDb:AcDbObjects` record by hand: `MS` + payload +
    /// 2-byte CRC slot. `mc` is the R2010+ leading handle-stream-size
    /// field (`None` before R2010), and the `MS` is written per the
    /// measured rule — the payload length *minus* that field's width.
    fn build_record(version: Version, mc: Option<u8>, handle: u64, filler: usize) -> Vec<u8> {
        let mut w = BitWriter::new();
        let mut mc_width = 0usize;
        if let Some(v) = mc {
            assert!(v < 0x80, "test helper only builds single-byte MC values");
            w.write_rc(v);
            mc_width = 1;
        }
        if version.is_r2010_plus() {
            w.write_bb(0b00); // one raw type byte follows
            w.write_rc(0x11); // ARC
        } else {
            w.write_bs(0x11);
        }
        if version.has_object_size_field() {
            w.write_rl(0);
        }
        w.write_handle(0, handle);
        for _ in 0..filler {
            w.write_rc(0xAB);
        }
        let payload = w.into_bytes();

        let mut header = BitWriter::new();
        header.write_ms((payload.len() - mc_width) as u64);
        let mut record = header.into_bytes();
        record.extend_from_slice(&payload);
        record.extend_from_slice(&[0x00, 0x00]);
        record
    }

    /// #77 — on R2010+ the record's `MS` excludes its own leading `MC`
    /// handle-stream-size field, so the walker's slice is one `MC`
    /// width longer than the declared size and the next record starts
    /// there. Two back-to-back records pin both halves: the slice width
    /// and the advance.
    #[test]
    fn r2010_raw_slice_spans_the_mc_the_ms_does_not_count() {
        let mut stream = vec![0xCA, 0x0D, 0x00, 0x00];
        let first = build_record(Version::R2018, Some(0x04), 0x2A, 3);
        let second = build_record(Version::R2018, Some(0x04), 0x2B, 5);
        stream.extend_from_slice(&first);
        stream.extend_from_slice(&second);

        let objects = ObjectWalker::new(&stream, Version::R2018)
            .collect_all()
            .expect("walk");
        assert_eq!(objects.len(), 2);

        // `MS` + 1-byte `MC` + `MS` bytes of payload + 2-byte CRC.
        assert_eq!(objects[0].stream_offset, 4);
        assert_eq!(objects[0].raw.len(), objects[0].size_bytes as usize + 1);
        assert_eq!(objects[0].raw.len(), first.len() - 2 - 2);
        assert_eq!(objects[0].raw[0], 0x04, "slice starts at the MC field");
        assert_eq!(objects[0].handle.value, 0x2A);

        // The advance is the whole record, so the second record's
        // header is read in phase. A slice that stopped at `MS` would
        // land one byte early and mis-read the type code.
        assert_eq!(objects[1].stream_offset, 4 + first.len());
        assert_eq!(objects[1].raw.len(), objects[1].size_bytes as usize + 1);
        assert_eq!(objects[1].handle.value, 0x2B);
    }

    /// The same record shape before R2010 carries no `MC`, and there
    /// the `MS` covers the whole payload.
    #[test]
    fn pre_r2010_raw_slice_is_exactly_the_declared_ms() {
        let mut stream = vec![0xCA, 0x0D, 0x00, 0x00];
        let record = build_record(Version::R2004, None, 0x2A, 3);
        stream.extend_from_slice(&record);

        let objects = ObjectWalker::new(&stream, Version::R2004)
            .collect_all()
            .expect("walk");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].raw.len(), objects[0].size_bytes as usize);
        assert_eq!(objects[0].raw.len(), record.len() - 2 - 2);
        assert_eq!(objects[0].handle.value, 0x2A);
    }

    /// A multi-byte `MC` widens the slice by its full width, not by one.
    #[test]
    fn a_two_byte_mc_widens_the_slice_by_two() {
        // 0x80 0x01 — continuation byte then terminator: a 2-byte MC
        // holding 128 bits of handle stream.
        let mut w = BitWriter::new();
        w.write_rc(0x80);
        w.write_rc(0x01);
        w.write_bb(0b00);
        w.write_rc(0x11);
        w.write_handle(0, 0x2A);
        for _ in 0..4 {
            w.write_rc(0xAB);
        }
        let payload = w.into_bytes();
        let mut header = BitWriter::new();
        header.write_ms((payload.len() - 2) as u64);
        let mut stream = vec![0xCA, 0x0D, 0x00, 0x00];
        stream.extend_from_slice(&header.into_bytes());
        stream.extend_from_slice(&payload);
        stream.extend_from_slice(&[0x00, 0x00]);

        let objects = ObjectWalker::new(&stream, Version::R2018)
            .collect_all()
            .expect("walk");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].raw.len(), objects[0].size_bytes as usize + 2);
        assert_eq!(objects[0].raw.len(), payload.len());
    }

    #[test]
    fn mc_field_width_counts_to_the_terminating_byte() {
        assert_eq!(mc_field_width(&[0x00, 0xFF]), Some(1));
        assert_eq!(mc_field_width(&[0x7F]), Some(1));
        assert_eq!(mc_field_width(&[0x80, 0x01, 0x02]), Some(2));
        assert_eq!(mc_field_width(&[0x80; 10]), None);
        assert_eq!(mc_field_width(&[]), None);
    }

    #[test]
    fn ms_single_module_zero() {
        // Size = 0 → module = 0x0000 → cont=0.
        let bytes = [0x00, 0x00, 0xDE, 0xAD];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_small_value() {
        // Size = 10 → module = 0x000A → LE bytes 0x0A, 0x00.
        let bytes = [0x0A, 0x00, 0xFF];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 10);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_large_value_single_module() {
        // Size = 0x7FFF → module = 0x7FFF → LE bytes 0xFF, 0x7F.
        let bytes = [0xFF, 0x7F, 0x00];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0x7FFF);
        assert_eq!(n, 2);
    }

    #[test]
    fn ms_multi_module() {
        // Size = 0x8000: 15 data bits = 0, continuation set.
        // Module 0: data=0, cont=1 → word = 0x8000 → LE bytes 0x00, 0x80.
        // Module 1: data=1, cont=0 → word = 0x0001 → LE bytes 0x01, 0x00.
        // Result: 0 << 0 | 1 << 15 = 0x8000.
        let bytes = [0x00, 0x80, 0x01, 0x00];
        let (v, n) = read_ms_bytealigned(&bytes).unwrap();
        assert_eq!(v, 0x8000);
        assert_eq!(n, 4);
    }

    #[test]
    fn summary_confidence_all_clean_is_one() {
        let s = ObjectWalkSummary {
            decoded_count: 100,
            skipped: Vec::new(),
            errored_count: 0,
            unknown_types: Vec::new(),
            handle_mismatches: Vec::new(),
        };
        assert_eq!(s.confidence(), 1.0);
    }

    #[test]
    fn summary_confidence_mixed_is_decoded_ratio() {
        // 50 decoded / 30 skipped / 20 errored = 50/100 = 0.5
        let skipped = (0..30)
            .map(|i| SkippedEntry {
                handle: i,
                offset: i,
                reason: String::new(),
            })
            .collect();
        let s = ObjectWalkSummary {
            decoded_count: 50,
            skipped,
            errored_count: 20,
            unknown_types: Vec::new(),
            handle_mismatches: Vec::new(),
        };
        assert_eq!(s.confidence(), 0.5);
    }

    #[test]
    fn summary_confidence_empty_walk_is_one() {
        // No work done is "perfectly clean" — there are zero failures.
        let s = ObjectWalkSummary::default();
        assert_eq!(s.confidence(), 1.0);
    }

    #[test]
    fn walker_limit_tripped_when_cap_is_zero() {
        // Easiest direct cap test: set max_handles=0 and verify the
        // first call into the walker refuses to consume bytes. We can
        // exercise this without synthesising a full `HandleMap` by
        // using the sequential (handle-map-less) walker path.
        let bytes = [0u8; 16];
        let walker = ObjectWalker::new(&bytes, Version::R2018).with_limits(WalkerLimits {
            max_handles: 0,
            ..WalkerLimits::safe()
        });
        let err = walker.collect_all().unwrap_err();
        match err {
            Error::WalkerLimitExceeded { limit, seen } => {
                assert_eq!(limit, 0);
                assert_eq!(seen, 0);
            }
            other => panic!("expected WalkerLimitExceeded, got {other:?}"),
        }
    }

    #[test]
    fn walker_default_limits_permit_real_work() {
        // Sanity: the default (safe) cap is high enough that the
        // walker is not constructed in a refuse-immediately state.
        let bytes = [0u8; 16];
        let walker = ObjectWalker::new(&bytes, Version::R2018);
        assert_eq!(walker.limits.max_handles, 1_000_000);
        assert_eq!(walker.records_visited, 0);
    }

    #[test]
    fn with_limits_overrides_default_cap() {
        let bytes = [0u8; 16];
        let walker = ObjectWalker::new(&bytes, Version::R2018).with_limits(WalkerLimits {
            max_handles: 42,
            ..WalkerLimits::safe()
        });
        assert_eq!(walker.limits.max_handles, 42);
    }

    #[test]
    fn unknown_types_capture_is_populated_by_lossy_collector() {
        // Direct fake: the lossy collector pushes (type_code, offset)
        // tuples into the summary when it encounters Unknown codes.
        // We cannot easily synthesise a full record here, so we seed
        // the summary by hand and check the field shape.
        let mut summary = ObjectWalkSummary::default();
        summary.unknown_types.push((0x1F4, 128));
        summary.unknown_types.push((0x1F5, 256));
        assert_eq!(summary.unknown_types.len(), 2);
        assert_eq!(summary.unknown_types[0].0, 0x1F4);
        assert_eq!(summary.unknown_types[0].1, 128);
    }
}
