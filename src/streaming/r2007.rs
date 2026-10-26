//! The whole R2007 (`AC1021`) file walk (ODA v5.4.1 §5.1–§5.4, §5.10–§5.13)
//! behind one fixed-size state and one checkpoint: the file header, the page
//! map, the section map and every data page of every section, handed out in
//! section order with each page's bytes checked before any of them is.
//!
//! R2007 has neither a page header nor a page index a fixed state could
//! build, so the walk differs from the R2004 family's. A data page is bare:
//! its section, its place in it, its sizes, its checksum and its CRC come only
//! from its reference in the section map, and its address only from the page
//! map. The section map is therefore the outer loop. For each page reference
//! the walker scans the page map from its start for the page's entry (one
//! page map pass per page reference; the page map is small, and a scan stops
//! at the entry), then walks the page. Pages come out in the order each
//! section lists them, which is the order of its bytes, tagged with their
//! offset; a gap before a page is a `Hole` of zeros, as in the legacy reader,
//! and a section that ends short of its size ends short.
//!
//! Every page, system or data, is a run of Reed-Solomon codewords interleaved
//! across the page (§5.13.2): byte `i` of codeword `j` of `n` is at `n * i +
//! j`. A page's message is read codeword by codeword (a [`Gather`] demand
//! names the 255 offsets), each codeword's syndromes are checked, and its
//! message bytes feed the §5.10 decoder, so no page is ever held whole and
//! no state grows with it. Each page is walked twice. The first pass reads
//! every codeword of the page (the redundant copies of a system page
//! included), checks every syndrome, the CRC-64 of the stored bytes, and
//! decodes the whole page to its declared size, checking the CRC-64 or the
//! 32-bit checksum of the decompressed bytes; it hands out nothing. Only then
//! does the second pass decode again and hand out bytes.
//!
//! State is the two readers (the section map's, kept while its pages are
//! walked, and the page scan's or the data page's) each with a 64 KiB window,
//! a codeword and a 256-byte output stage, and a few hundred bytes of
//! cursors. [`R2007Checkpoint`](super::R2007Checkpoint) (`DWGR`) holds the
//! walk's cursors, the verified header and each reader's progress; the
//! geometry of every page is derived from them on restore, never stored.

use super::r2007_codes::{
    CODEWORD, Code, Crc64, Sum32, check_data, codeword_intact, decode_seed, seed1, seed2,
    sequence_crc,
};
use super::r2007_lz::{R2007Lz, WINDOW};
use super::{CHUNK_BYTES, Request, SourceIdentity, StreamError};

/// The R2007 reader's decoder window: the largest distance of §5.10.
pub const R2007_WINDOW: usize = WINDOW;
/// Longest section name the walker reads, in bytes of UTF-16LE.
pub const MAX_NAME: usize = 128;

pub(super) const PAGE_BASE: u64 = 0x480;
const HEAD_BYTES: usize = 0x480;
const BLOCK_AT: usize = 0x80;
const BLOCK_BYTES: usize = 0x400;
const DECODED: usize = 3 * 239;
const PLAIN: usize = 0x110;
pub(super) const STAGE: usize = CHUNK_BYTES;
const ENTRY: usize = 16;
const DESCRIPTOR: usize = 0x40;
const REFERENCE: usize = 56;

/// `R2007Header` codes: a file offset of the meta data below `0x1000`,
/// `0x1000 + n` for byte `n` of the decoded file header block and `0x2000 +
/// n` for byte `n` of the 0x110-byte header.
const AT_BLOCK: u16 = 0x1000;
const AT_HEADER: u16 = 0x2000;

/// The remaining bytes of one codeword, each `stride` bytes after the last:
/// byte `k` of the demand is at `offset + k * stride`. A source that does
/// not read strided can issue one single-byte read per byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gather {
    pub offset: u64,
    pub stride: u64,
    pub len: usize,
}

/// The walker's next need: source bytes read contiguously, the bytes of a
/// codeword gathered across a page, a pump with no source bytes, or nothing
/// more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R2007Demand {
    Read(Request),
    Gather(Gather),
    Pump,
    Done,
}

/// The verified file header (§5.2): its 34 little-endian `u64` fields in
/// the order of the spec's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R2007Header {
    pub fields: [u64; 34],
}

/// One entry of the page map (§5.2) in map order. `offset` is relative to
/// the first page map, accumulated from the sizes before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R2007Entry {
    pub id: i64,
    pub size: i64,
    pub offset: u64,
}

/// One section description of the section map (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R2007Section {
    /// Position among the named sections, counted from 0.
    pub index: u32,
    pub size: u64,
    pub max_size: u64,
    pub encryption: u64,
    pub hash_code: u64,
    pub encoding: u64,
    pub page_count: u64,
    /// UTF-16LE name without its terminator.
    pub name: [u8; MAX_NAME],
    /// Bytes of `name` in use.
    pub name_len: u8,
}

impl R2007Section {
    /// The name's UTF-16 code units.
    pub fn name_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.name[..usize::from(self.name_len)]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

/// A data page whose reed-solomon codewords, CRC-64 and checksum all passed;
/// its bytes follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R2007Page {
    pub section: u32,
    /// The page id as the section map names it.
    pub id: i64,
    pub address: u64,
    pub data_offset: u64,
    /// The section map's "page size" field.
    pub size: u64,
    pub uncompressed: u64,
    pub compressed: u64,
    pub checksum: u64,
    pub crc: u64,
    /// Codewords the page spans; 0 for a page stored bare.
    pub codewords: u64,
}

/// What one feed or pump produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R2007Step {
    Framing,
    /// The verified file header, once.
    Header(R2007Header),
    /// A page map entry, in map order, during the first pass over the map.
    Entry(R2007Entry),
    /// A section description, in the section map's order, before its pages.
    Section(R2007Section),
    /// A data page that verified; its bytes follow.
    Page(R2007Page),
    /// `len` zero bytes of the current section at `offset`: a gap before
    /// the page that follows (§5.2: a page of zeros is not stored).
    Hole {
        offset: u64,
        len: u64,
    },
    /// `len` bytes of the current section at `offset`, in
    /// [`R2007Walker::data`] until the next call.
    Data {
        offset: u64,
        len: usize,
    },
    /// The page's stream ended; `emitted` of its `plain` bytes lay inside
    /// the section.
    PageEnd {
        plain: u64,
        emitted: u64,
    },
    /// The section's last page was walked; `covered` bytes of it are
    /// described by pages (the legacy reader returns exactly that many).
    SectionEnd {
        index: u32,
        covered: u64,
    },
    /// Every section's pages were walked.
    Finished {
        sections: u32,
        pages: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Pass {
    /// Read every codeword, check everything, hand out nothing.
    Verify = 0,
    /// Read what the stream needs and hand out its bytes.
    Emit = 1,
    /// Like `Emit`, for a scan that may stop early: no totals are checked.
    Scan = 2,
}

/// What must hold of the decompressed bytes of a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Check {
    /// System page: the CRC-64 of the decompressed bytes (seed already
    /// updated with their length).
    Crc {
        expected: u64,
        seed: u64,
    },
    /// Data page: the §5.4.1 checksum.
    Sum {
        expected: u32,
    },
    Nothing,
}

/// Where a page is and what must hold of it. Derived from the header, the
/// maps and the page reference; never serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Geom {
    pub(super) base: u64,
    pub(super) code: Option<Code>,
    pub(super) blocks: u64,
    pub(super) stored: u64,
    pub(super) plain: u64,
    pub(super) lz: bool,
    pub(super) stored_crc: u64,
    pub(super) stored_seed: u64,
    pub(super) check: Check,
}

impl Geom {
    pub(super) const EMPTY: Geom = Geom {
        base: 0,
        code: None,
        blocks: 0,
        stored: 0,
        plain: 0,
        lz: false,
        stored_crc: 0,
        stored_seed: 0,
        check: Check::Nothing,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    /// The reader needs source bytes.
    Need,
    /// The reader can make progress without source bytes.
    Work,
    /// The pass is complete.
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fill {
    Read(Request),
    Gather(Gather),
}

/// One page being read: its codewords, its decoder and its checks.
#[derive(Debug, Clone)]
pub(super) struct Reader {
    pub(super) geom: Geom,
    pub(super) pass: Pass,
    /// Codewords completed this pass.
    pub(super) cw: u64,
    /// Bare pages: bytes read this pass.
    pub(super) src: u64,
    pub(super) buf: [u8; STAGE],
    /// Bytes of the codeword or chunk being filled.
    pub(super) have: usize,
    /// Stored bytes of the last complete fill, and how many the decoder
    /// has taken.
    pub(super) msg: usize,
    pub(super) take: usize,
    /// Stored bytes handed to the decoder's input so far.
    pub(super) fed: u64,
    /// Decompressed bytes produced so far.
    pub(super) produced: u64,
    pub(super) lz: R2007Lz,
    pub(super) stage: [u8; STAGE],
    pub(super) crc_s: Crc64,
    pub(super) crc_p: Crc64,
    pub(super) sum: Sum32,
}

impl Reader {
    pub(super) fn blank() -> Self {
        Self {
            geom: Geom::EMPTY,
            pass: Pass::Verify,
            cw: 0,
            src: 0,
            buf: [0; STAGE],
            have: 0,
            msg: 0,
            take: 0,
            fed: 0,
            produced: 0,
            lz: R2007Lz::blank(),
            stage: [0; STAGE],
            crc_s: Crc64::new(0, true),
            crc_p: Crc64::new(0, true),
            sum: Sum32::new(0),
        }
    }

    pub(super) fn clear(&mut self) {
        self.begin(Geom::EMPTY, Pass::Verify);
    }

    pub(super) fn begin(&mut self, geom: Geom, pass: Pass) {
        self.geom = geom;
        self.restart(pass);
    }

    pub(super) fn restart(&mut self, pass: Pass) {
        let g = self.geom;
        self.pass = pass;
        self.cw = 0;
        self.src = 0;
        self.buf = [0; STAGE];
        self.have = 0;
        self.msg = 0;
        self.take = 0;
        self.fed = 0;
        self.produced = 0;
        self.lz.reset(if g.lz { g.plain } else { 0 });
        self.crc_s = Crc64::new(g.stored_seed, true);
        self.crc_p = Crc64::new(
            match g.check {
                Check::Crc { seed, .. } => seed,
                _ => 0,
            },
            true,
        );
        self.sum = Sum32::new(g.plain);
    }

    /// Whether the reader holds no page and no progress.
    pub(super) fn is_blank(&self) -> bool {
        self.geom == Geom::EMPTY && self.progress_blank()
    }

    fn progress_blank(&self) -> bool {
        self.pass == Pass::Verify
            && self.cw == 0
            && self.src == 0
            && self.have == 0
            && self.msg == 0
            && self.take == 0
            && self.fed == 0
            && self.produced == 0
            && self.buf == [0; STAGE]
            && self.lz.mode == super::r2007_lz::Mode::Start
            && self.lz.consistent()
            && self.lz.limit == 0
            && self.crc_s == Crc64::new(self.geom.stored_seed, true)
            && self.crc_p == Crc64::new(0, true)
            && self.sum == Sum32::new(self.geom.plain)
    }

    /// Bytes the fill in progress needs in all.
    fn fill_total(&self) -> usize {
        match self.geom.code {
            Some(_) => CODEWORD,
            None => (self.geom.stored - self.src).min(STAGE as u64) as usize,
        }
    }

    pub(super) fn status(&self) -> Result<Status, StreamError> {
        let g = &self.geom;
        let done = self.produced == g.plain;
        if done && self.pass != Pass::Verify {
            return Ok(Status::Finished);
        }
        let working = !done && (self.take < self.msg || (g.lz && self.lz.has_pending_output()));
        if working {
            return Ok(Status::Work);
        }
        if !done && g.lz && self.fed == g.stored && self.take >= self.msg {
            return Err(StreamError::Lz77Truncated {
                consumed: self.lz.consumed(),
            });
        }
        let more = match g.code {
            Some(_) if self.pass == Pass::Verify => self.cw < g.blocks,
            Some(_) => self.fed < g.stored,
            None => self.src < g.stored,
        };
        if more {
            Ok(Status::Need)
        } else if done {
            Ok(Status::Finished)
        } else {
            Err(StreamError::Lz77Truncated {
                consumed: self.lz.consumed(),
            })
        }
    }

    /// The next fill. Only meaningful when [`Self::status`] says `Need`.
    pub(super) fn fill(&self) -> Fill {
        let g = &self.geom;
        match g.code {
            Some(_) => Fill::Gather(Gather {
                offset: g.base + self.have as u64 * g.blocks + self.cw,
                stride: g.blocks,
                len: CODEWORD - self.have,
            }),
            None => Fill::Read(Request {
                offset: g.base + self.src + self.have as u64,
                len: self.fill_total() - self.have,
            }),
        }
    }

    pub(super) fn feed(&mut self, offset: u64, input: &[u8]) -> Result<(), StreamError> {
        let (want, len) = match self.fill() {
            Fill::Read(r) => (r.offset, r.len),
            Fill::Gather(g) => (g.offset, g.len),
        };
        if offset != want {
            return Err(StreamError::WrongOffset {
                expected: want,
                actual: offset,
            });
        }
        if input.is_empty() || input.len() > len {
            return Err(StreamError::InvalidFeed {
                maximum: len,
                actual: input.len(),
            });
        }
        self.buf[self.have..self.have + input.len()].copy_from_slice(input);
        self.have += input.len();
        if self.have == self.fill_total() {
            self.complete()?;
        }
        Ok(())
    }

    /// A codeword or chunk is whole: check it and offer its stored bytes.
    fn complete(&mut self) -> Result<(), StreamError> {
        let g = self.geom;
        let m = match g.code {
            Some(code) => {
                // The emitting pass re-reads what the verifying pass checked.
                if self.pass != Pass::Emit && !codeword_intact(code, &self.buf[..CODEWORD]) {
                    return Err(StreamError::R2007Parity {
                        at: g.base,
                        codeword: self.cw,
                    });
                }
                let k = code.message() as u64;
                let carried = g.stored.saturating_sub(self.cw * k).min(k) as usize;
                self.cw += 1;
                carried
            }
            None => {
                let n = self.have;
                self.src += n as u64;
                n
            }
        };
        self.have = 0;
        self.msg = m;
        self.take = 0;
        self.fed += m as u64;
        if self.pass == Pass::Verify {
            self.crc_s.update(&self.buf[..m]);
        }
        Ok(())
    }

    /// Decode up to `max` bytes into `stage`, from the stored bytes in hand.
    /// A verify pass folds them into the checks; they are handed out only by
    /// the other passes.
    pub(super) fn pump(&mut self, max: usize) -> Result<usize, StreamError> {
        let g = self.geom;
        let max = max.min(STAGE);
        let n = if g.lz {
            let p = self
                .lz
                .decode(&self.buf[self.take..self.msg], &mut self.stage[..max])?;
            self.take += p.consumed;
            p.produced
        } else {
            let n = max
                .min(self.msg - self.take)
                .min(usize::try_from(g.plain - self.produced).unwrap_or(usize::MAX));
            self.stage[..n].copy_from_slice(&self.buf[self.take..self.take + n]);
            self.take += n;
            n
        };
        self.produced += n as u64;
        if self.pass == Pass::Verify {
            match g.check {
                Check::Crc { .. } => self.crc_p.update(&self.stage[..n]),
                Check::Sum { .. } => self.sum.update(&self.stage[..n]),
                Check::Nothing => (),
            }
        }
        if self.produced == g.plain {
            self.take = self.msg;
        }
        Ok(n)
    }

    /// The checks that need the whole page, after a complete verify pass.
    pub(super) fn verdict(&self) -> Result<(), StreamError> {
        let g = &self.geom;
        let actual = self.crc_s.finish();
        let which = match g.check {
            Check::Sum { .. } => 7,
            _ => 5,
        };
        if actual != g.stored_crc {
            return Err(StreamError::R2007Crc64 {
                at: g.base,
                check: which,
                expected: g.stored_crc,
                actual,
            });
        }
        match g.check {
            Check::Crc { expected, .. } => {
                let actual = self.crc_p.finish();
                if actual != expected {
                    return Err(StreamError::R2007Crc64 {
                        at: g.base,
                        check: 6,
                        expected,
                        actual,
                    });
                }
            }
            Check::Sum { expected } => {
                let actual = self.sum.finish();
                if actual != expected {
                    return Err(StreamError::R2007Checksum {
                        at: g.base,
                        expected,
                        actual,
                    });
                }
            }
            Check::Nothing => (),
        }
        Ok(())
    }

    /// Restore-time check that the progress fields form a state the reader
    /// can reach over its geometry.
    pub(super) fn progress_ok(&self) -> bool {
        let g = &self.geom;
        let lz_ok = if g.lz {
            self.lz.limit == g.plain && self.lz.produced == self.produced && self.lz.consistent()
        } else {
            self.lz.mode == super::r2007_lz::Mode::Start
                && self.lz.limit == 0
                && self.lz.consistent()
        };
        let fill_ok = match g.code {
            Some(code) => {
                let k = code.message() as u64;
                self.cw <= g.blocks
                    && self.src == 0
                    && self.have < CODEWORD
                    && self.msg <= code.message()
                    && self.take <= self.msg
                    && self.fed == g.stored.min(self.cw.saturating_mul(k))
                    && (self.cw == 0
                        || self.msg == g.stored.saturating_sub((self.cw - 1) * k).min(k) as usize)
                    && (self.cw != 0 || self.msg == 0)
            }
            None => {
                self.cw == 0
                    && self.src <= g.stored
                    && self.have as u64 <= g.stored - self.src
                    && self.have < STAGE
                    && self.msg <= STAGE
                    && self.take <= self.msg
                    && self.fed == self.src
                    && self.msg as u64 <= self.src
            }
        };
        let crc_p_seed = match g.check {
            Check::Crc { seed, .. } => seed,
            _ => 0,
        };
        let held_s = (self.fed % 8) as u8;
        let held_p = (self.produced % 8) as u8;
        let checks_ok = if self.pass == Pass::Verify {
            self.crc_s.held == held_s
                && self.crc_s.mirrored
                && match g.check {
                    Check::Crc { .. } => {
                        self.crc_p.held == held_p
                            && self.crc_p.mirrored
                            && self.sum == Sum32::new(g.plain)
                    }
                    Check::Sum { .. } => {
                        self.sum.held == held_p
                            && self.sum.chunk == (self.produced / 8 * 8 % 0x15b0) as u16
                            && self.crc_p == Crc64::new(crc_p_seed, true)
                    }
                    Check::Nothing => {
                        self.crc_p == Crc64::new(0, true) && self.sum == Sum32::new(g.plain)
                    }
                }
        } else {
            self.crc_s == Crc64::new(g.stored_seed, true)
                && self.crc_p == Crc64::new(crc_p_seed, true)
                && self.sum == Sum32::new(g.plain)
        };
        self.produced <= g.plain
            && self.fed <= g.stored
            && (self.have == 0 || self.take == self.msg)
            && (self.produced < g.plain || self.take == self.msg)
            && lz_ok
            && fill_ok
            && checks_ok
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Stage {
    /// The first 0x480 bytes: the meta data and the file header page.
    Head = 0,
    /// The first pass over the page map: verify, then walk its entries.
    Map = 1,
    /// The section map: verify, then walk its records.
    Sections = 2,
    /// Scanning the page map for the page a reference names.
    Lookup = 3,
    /// Walking a data page.
    Page = 4,
    Done = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pending {
    None,
    Hole { offset: u64, len: u64 },
    SectionEnd { index: u32, covered: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Rec {
    Descriptor = 0,
    Name = 1,
    Reference = 2,
}

/// Page map facts gathered while its entries are walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct MapFacts {
    pub(super) entries: u64,
    pub(super) max_id: u64,
    /// Offset the next entry starts at.
    pub(super) acc: u64,
    pub(super) self_seen: bool,
    pub(super) sm_found: bool,
    pub(super) sm_address: u64,
}

/// The section being walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Cur {
    pub(super) index: u32,
    pub(super) raw_name: u8,
    pub(super) name_len: u8,
    pub(super) name: [u8; MAX_NAME],
    pub(super) size: u64,
    pub(super) max: u64,
    pub(super) encryption: u64,
    pub(super) hash: u64,
    pub(super) encoding: u64,
    pub(super) pages: u64,
    pub(super) done: u64,
    /// End, in the section, of the last page walked.
    pub(super) end: u64,
}

impl Cur {
    pub(super) const NONE: Cur = Cur {
        index: 0,
        raw_name: 0,
        name_len: 0,
        name: [0; MAX_NAME],
        size: 0,
        max: 0,
        encryption: 0,
        hash: 0,
        encoding: 0,
        pages: 0,
        done: 0,
        end: 0,
    };
}

/// The page reference being walked, and what the page map said of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Ref {
    pub(super) data_offset: u64,
    pub(super) size: u64,
    pub(super) id: i64,
    pub(super) uncompressed: u64,
    pub(super) compressed: u64,
    pub(super) checksum: u64,
    pub(super) crc: u64,
    pub(super) address: u64,
    pub(super) entry_size: u64,
}

/// Fixed-size state for the whole walk.
#[derive(Debug, Clone)]
pub struct R2007Walker {
    pub(super) source: SourceIdentity,
    pub(super) stage: Stage,
    pub(super) failure: Option<StreamError>,
    pub(super) head: [u8; HEAD_BYTES],
    pub(super) have: usize,
    pub(super) hdr: [u64; 34],
    pub(super) pending: Pending,
    pub(super) facts: MapFacts,
    pub(super) aux: Reader,
    pub(super) sec: Reader,
    pub(super) entry: [u8; ENTRY],
    pub(super) entry_have: usize,
    pub(super) rec: [u8; DESCRIPTOR],
    pub(super) rec_have: usize,
    pub(super) rec_kind: Rec,
    pub(super) name: [u8; MAX_NAME],
    pub(super) cur: Cur,
    pub(super) refd: Ref,
    /// Descriptors read, named or not.
    pub(super) described: u32,
    /// Named sections read.
    pub(super) sections: u32,
    /// Data pages walked.
    pub(super) pages: u32,
    /// Bytes of the current page handed out so far.
    pub(super) emitted: u64,
    pub(super) out_len: usize,
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut b = [0; 8];
    b.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(b)
}

impl R2007Walker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self {
            source,
            stage: Stage::Head,
            failure: None,
            head: [0; HEAD_BYTES],
            have: 0,
            hdr: [0; 34],
            pending: Pending::None,
            facts: MapFacts::default(),
            aux: Reader::blank(),
            sec: Reader::blank(),
            entry: [0; ENTRY],
            entry_have: 0,
            rec: [0; DESCRIPTOR],
            rec_have: 0,
            rec_kind: Rec::Descriptor,
            name: [0; MAX_NAME],
            cur: Cur::NONE,
            refd: Ref::default(),
            described: 0,
            sections: 0,
            pages: 0,
            emitted: 0,
            out_len: 0,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.source
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<R2007Header> {
        (self.stage != Stage::Head).then_some(R2007Header { fields: self.hdr })
    }

    /// Section bytes the last `Data` step produced; empty after any other
    /// step.
    pub fn data(&self) -> &[u8] {
        &self.aux.stage[..self.out_len]
    }

    /// The next need. Pure: it changes nothing.
    pub fn demand(&self) -> Result<R2007Demand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.pending != Pending::None {
            return Ok(R2007Demand::Pump);
        }
        match self.stage {
            Stage::Head => {
                if HEAD_BYTES as u64 > self.source.length {
                    return Err(StreamError::Truncated {
                        offset: self.have as u64,
                        needed: HEAD_BYTES - self.have,
                    });
                }
                Ok(R2007Demand::Read(Request {
                    offset: self.have as u64,
                    len: HEAD_BYTES - self.have,
                }))
            }
            Stage::Map | Stage::Lookup | Stage::Page => Self::reader_demand(&self.aux),
            Stage::Sections => Self::reader_demand(&self.sec),
            Stage::Done => Ok(R2007Demand::Done),
        }
    }

    fn reader_demand(reader: &Reader) -> Result<R2007Demand, StreamError> {
        Ok(match reader.status()? {
            Status::Work | Status::Finished => R2007Demand::Pump,
            Status::Need => match reader.fill() {
                Fill::Read(r) => R2007Demand::Read(r),
                Fill::Gather(g) => R2007Demand::Gather(g),
            },
        })
    }

    /// Consume one nonempty prefix of a `Read` or `Gather` demand. Invalid
    /// delivery leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<R2007Step, StreamError> {
        let (want, len) = match self.demand()? {
            R2007Demand::Read(r) => (r.offset, r.len),
            R2007Demand::Gather(g) => (g.offset, g.len),
            _ => return Err(StreamError::Finished),
        };
        if offset != want {
            return Err(StreamError::WrongOffset {
                expected: want,
                actual: offset,
            });
        }
        if input.is_empty() || input.len() > len {
            return Err(StreamError::InvalidFeed {
                maximum: len,
                actual: input.len(),
            });
        }
        self.out_len = 0;
        let result = match self.stage {
            Stage::Head => self.take_head(input),
            Stage::Map | Stage::Lookup | Stage::Page => {
                self.aux.feed(offset, input).map(|()| R2007Step::Framing)
            }
            Stage::Sections => self.sec.feed(offset, input).map(|()| R2007Step::Framing),
            Stage::Done => Err(StreamError::Finished),
        };
        self.settle(result)
    }

    /// Advance a `Pump` demand: no source bytes are involved.
    pub fn pump(&mut self) -> Result<R2007Step, StreamError> {
        if self.demand()? != R2007Demand::Pump {
            return Err(StreamError::Finished);
        }
        self.out_len = 0;
        let result = self.run_pump();
        self.settle(result)
    }

    /// Reject the end of the source while a `Read` or `Gather` is
    /// outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            R2007Demand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            R2007Demand::Gather(g) => Err(StreamError::Truncated {
                offset: g.offset,
                needed: g.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(&mut self, result: Result<R2007Step, StreamError>) -> Result<R2007Step, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn run_pump(&mut self) -> Result<R2007Step, StreamError> {
        match self.pending {
            Pending::Hole { offset, len } => {
                self.pending = Pending::None;
                return Ok(R2007Step::Hole { offset, len });
            }
            Pending::SectionEnd { index, covered } => {
                self.pending = Pending::None;
                self.cur = Cur::NONE;
                return Ok(R2007Step::SectionEnd { index, covered });
            }
            Pending::None => (),
        }
        match self.stage {
            Stage::Map | Stage::Lookup | Stage::Page => self.pump_aux(),
            Stage::Sections => self.pump_sec(),
            Stage::Head | Stage::Done => Err(StreamError::Finished),
        }
    }

    fn at_source(&self, span: u64, base: u64) -> Result<(), StreamError> {
        match base.checked_add(span) {
            Some(end) if end <= self.source.length => Ok(()),
            _ => Err(StreamError::Truncated {
                offset: base,
                needed: usize::try_from(span).unwrap_or(usize::MAX),
            }),
        }
    }

    /// A system page: `factor` copies of its stored bytes, repeated and
    /// Reed-Solomon coded, `blocks` codewords in all (§5.3).
    fn system_geom(
        &self,
        base: u64,
        sizes: (u64, u64, u64),
        crcs: (u64, u64),
        seed: u64,
    ) -> Result<Geom, StreamError> {
        let (comp, unc, factor) = sizes;
        let stored = comp.min(unc);
        if factor == 0 || stored == 0 || unc > u64::from(u32::MAX) {
            return Err(StreamError::R2007Page(base));
        }
        let padded = u128::from((stored + 7) & !7);
        let blocks = u64::try_from((u128::from(factor) * padded).div_ceil(239))
            .map_err(|_| StreamError::R2007Page(base))?;
        let span = blocks
            .checked_mul(CODEWORD as u64)
            .ok_or(StreamError::R2007Page(base))?;
        self.at_source(span, base)?;
        Ok(Geom {
            base,
            code: Some(Code::System),
            blocks,
            stored,
            plain: unc,
            lz: comp < unc,
            stored_crc: crcs.0,
            stored_seed: seed1(seed, stored),
            check: Check::Crc {
                expected: crcs.1,
                seed: seed1(seed, unc),
            },
        })
    }

    fn page_map_geom(&self) -> Result<Geom, StreamError> {
        let h = &self.hdr;
        let base = PAGE_BASE
            .checked_add(h[7])
            .ok_or(StreamError::R2007Page(h[7]))?;
        self.system_geom(base, (h[10], h[11], h[3]), (h[2], h[16]), decode_seed(h[4]))
    }

    fn section_map_geom(&self) -> Result<Geom, StreamError> {
        let h = &self.hdr;
        self.system_geom(
            self.facts.sm_address,
            (h[22], h[25], h[27]),
            (h[26], h[21]),
            decode_seed(h[28]),
        )
    }

    /// A data page of the section being walked, from its reference and the
    /// address the page map gave it.
    fn data_geom(&self) -> Result<Geom, StreamError> {
        let (r, base) = (&self.refd, self.refd.address);
        let rs = self.cur.encoding == 4;
        let comp = r.compressed;
        if comp > r.uncompressed
            || r.uncompressed > u64::from(u32::MAX)
            || r.checksum > u64::from(u32::MAX)
        {
            return Err(StreamError::SectionEntry(self.cur.index));
        }
        let blocks = if rs { comp.div_ceil(251).max(1) } else { 0 };
        let span = if rs { blocks * CODEWORD as u64 } else { comp };
        if span > r.entry_size {
            return Err(StreamError::R2007Page(base));
        }
        self.at_source(span, base)?;
        Ok(Geom {
            base,
            code: rs.then_some(Code::Data),
            blocks,
            stored: comp,
            plain: r.uncompressed,
            lz: comp < r.uncompressed,
            stored_crc: r.crc,
            stored_seed: seed1(self.hdr[30], comp),
            check: Check::Sum {
                expected: r.checksum as u32,
            },
        })
    }

    /// The first 0x480 bytes: the meta data, and the file header page (§5.2)
    /// whose every check is made before any field of it is used.
    fn take_head(&mut self, input: &[u8]) -> Result<R2007Step, StreamError> {
        self.head[self.have..self.have + input.len()].copy_from_slice(input);
        self.have += input.len();
        if self.have < HEAD_BYTES {
            return Ok(R2007Step::Framing);
        }
        let head = self.head;
        let fields = self.verify_head(&head)?;
        self.head = [0; HEAD_BYTES];
        self.have = 0;
        self.hdr = fields;
        let geom = self.page_map_geom()?;
        self.aux.begin(geom, Pass::Verify);
        self.stage = Stage::Map;
        self.facts = MapFacts::default();
        Ok(R2007Step::Header(R2007Header { fields }))
    }

    fn verify_head(&mut self, head: &[u8; HEAD_BYTES]) -> Result<[u64; 34], StreamError> {
        let magic = [head[0], head[1], head[2], head[3], head[4], head[5]];
        match &magic {
            b"AC1021" => (),
            b"AC1012" | b"AC1014" | b"AC1015" | b"AC1018" | b"AC1024" | b"AC1027" | b"AC1032" => {
                return Err(StreamError::UnsupportedLayout(magic));
            }
            _ => return Err(StreamError::InvalidMagic(magic)),
        }
        if head[0x28..0x2c] != 0x80u32.to_le_bytes() {
            return Err(StreamError::R2007Header(0x28));
        }
        let block = &head[BLOCK_AT..];
        let (normal, mirrored) = (u64_at(block, 0x3d8), u64_at(block, 0x3e0));
        let (r1, r2) = (u64_at(block, 0x3e8), u64_at(block, 0x3f0));
        // The three interleaved codewords, then the check data.
        let mut decoded = [0u8; DECODED];
        let mut codeword = [0u8; CODEWORD];
        for j in 0..3 {
            for (i, byte) in codeword.iter_mut().enumerate() {
                *byte = block[3 * i + j];
            }
            if !codeword_intact(Code::System, &codeword) {
                return Err(StreamError::R2007Parity {
                    at: BLOCK_AT as u64,
                    codeword: j as u64,
                });
            }
            decoded[j * 239..(j + 1) * 239].copy_from_slice(&codeword[..239]);
        }
        if let Err((check, expected, actual)) = check_data(normal, mirrored, r1, r2) {
            return Err(StreamError::R2007Crc64 {
                at: BLOCK_AT as u64,
                check,
                expected,
                actual,
            });
        }
        let key = u64_at(&decoded, 8);
        let at = BLOCK_AT as u64;
        let expected = u64_at(&decoded, 0);
        let actual = sequence_crc(key);
        if actual != expected {
            return Err(StreamError::R2007Crc64 {
                at,
                check: 0,
                expected,
                actual,
            });
        }
        let length =
            i32::from_le_bytes([decoded[0x18], decoded[0x19], decoded[0x1a], decoded[0x1b]]);
        let mut plain = [0u8; PLAIN];
        let stored = length.unsigned_abs() as usize;
        if stored == 0 || 0x20 + stored > DECODED {
            return Err(StreamError::R2007Header(AT_BLOCK + 0x18));
        }
        let data = &decoded[0x20..0x20 + stored];
        if length < 0 {
            if stored < PLAIN {
                return Err(StreamError::R2007Header(AT_BLOCK + 0x18));
            }
            plain.copy_from_slice(&data[..PLAIN]);
        } else {
            let expected = u64_at(&decoded, 0x10);
            let mut crc = Crc64::new(seed2(0, stored as u64), false);
            crc.update(data);
            let actual = crc.finish();
            if actual != expected {
                return Err(StreamError::R2007Crc64 {
                    at,
                    check: 1,
                    expected,
                    actual,
                });
            }
            let lz = &mut self.aux.lz;
            lz.reset(PLAIN as u64);
            let progress = lz.decode(data, &mut plain)?;
            if progress.produced != PLAIN {
                return Err(StreamError::Lz77Truncated {
                    consumed: lz.consumed(),
                });
            }
            lz.reset(0);
        }
        check_plain_header(&plain, self.source.length)
    }

    fn pump_aux(&mut self) -> Result<R2007Step, StreamError> {
        let status = self.aux.status()?;
        match (self.stage, self.aux.pass, status) {
            (Stage::Map, Pass::Verify, Status::Finished) => {
                self.aux.verdict()?;
                self.aux.restart(Pass::Emit);
                self.entry_have = 0;
                Ok(R2007Step::Framing)
            }
            (Stage::Page, Pass::Verify, Status::Finished) => {
                self.aux.verdict()?;
                self.page_verified()
            }
            (_, Pass::Verify, Status::Work) => {
                self.aux.pump(STAGE)?;
                Ok(R2007Step::Framing)
            }
            (Stage::Map, Pass::Emit, Status::Finished) => self.finish_map(),
            (Stage::Lookup, Pass::Scan, Status::Finished) => {
                Err(StreamError::R2007PageMissing(self.refd.id))
            }
            (Stage::Map | Stage::Lookup, _, Status::Work) => self.pump_entry(),
            (Stage::Page, Pass::Emit, Status::Finished) => self.page_done(),
            (Stage::Page, Pass::Emit, Status::Work) => self.pump_data(),
            _ => Err(StreamError::Finished),
        }
    }

    /// The next bytes of the page map's stream, a whole 16-byte entry at a time.
    fn pump_entry(&mut self) -> Result<R2007Step, StreamError> {
        let left = self.aux.geom.plain - self.aux.produced;
        let need = ((ENTRY - self.entry_have) as u64).min(left) as usize;
        let n = self.aux.pump(need)?;
        self.entry[self.entry_have..self.entry_have + n].copy_from_slice(&self.aux.stage[..n]);
        self.entry_have += n;
        if self.entry_have < ENTRY {
            if self.aux.produced == self.aux.geom.plain {
                return Err(StreamError::MapLength(self.aux.geom.base));
            }
            return Ok(R2007Step::Framing);
        }
        self.entry_have = 0;
        let entry = R2007Entry {
            id: u64_at(&self.entry, 8) as i64,
            size: u64_at(&self.entry, 0) as i64,
            offset: self.facts.acc,
        };
        if self.stage == Stage::Map {
            return self.on_entry(entry);
        }
        let next = self
            .facts
            .acc
            .checked_add(entry.size.unsigned_abs())
            .ok_or(StreamError::MapLength(self.aux.geom.base))?;
        self.facts.acc = next;
        if entry.id.unsigned_abs() != self.refd.id.unsigned_abs() {
            return Ok(R2007Step::Framing);
        }
        self.refd.address = PAGE_BASE
            .checked_add(entry.offset)
            .ok_or(StreamError::R2007Page(entry.offset))?;
        self.refd.entry_size = entry.size.unsigned_abs();
        let geom = self.data_geom()?;
        self.aux.begin(geom, Pass::Verify);
        self.stage = Stage::Page;
        Ok(R2007Step::Framing)
    }

    /// One entry of the page map's first pass (§5.2): the accumulated offsets
    /// are the pages' places.
    fn on_entry(&mut self, entry: R2007Entry) -> Result<R2007Step, StreamError> {
        let base = self.aux.geom.base;
        let n = self.facts.entries;
        let index = u32::try_from(n).unwrap_or(u32::MAX);
        let size = entry.size.unsigned_abs();
        if entry.id == 0 || size % 0x20 != 0 {
            return Err(StreamError::PageEntry(index));
        }
        let next = self
            .facts
            .acc
            .checked_add(size)
            .ok_or(StreamError::PageEntry(index))?;
        self.facts.acc = next;
        self.facts.entries = n + 1;
        let id = entry.id.unsigned_abs();
        self.facts.max_id = self.facts.max_id.max(id);
        if id == self.hdr[8] && !self.facts.self_seen {
            if entry.offset != self.hdr[7] {
                return Err(StreamError::PageMapSelf);
            }
            self.facts.self_seen = true;
        }
        if id == self.hdr[24] && !self.facts.sm_found {
            self.facts.sm_address = PAGE_BASE
                .checked_add(entry.offset)
                .ok_or(StreamError::R2007Page(base))?;
            self.facts.sm_found = true;
        }
        Ok(R2007Step::Entry(entry))
    }

    /// The page map's stream ended: every count the header gave must hold.
    fn finish_map(&mut self) -> Result<R2007Step, StreamError> {
        let f = self.facts;
        if self.entry_have != 0 {
            return Err(StreamError::MapLength(self.aux.geom.base));
        }
        if f.entries != self.hdr[12] || f.max_id != self.hdr[13] {
            return Err(StreamError::PageEntry(
                u32::try_from(f.entries).unwrap_or(u32::MAX),
            ));
        }
        if !f.self_seen {
            return Err(StreamError::PageMapSelf);
        }
        if !f.sm_found {
            return Err(StreamError::SectionMapMissing);
        }
        let geom = self.section_map_geom()?;
        self.aux.clear();
        self.sec.begin(geom, Pass::Verify);
        self.rec_have = 0;
        self.rec_kind = Rec::Descriptor;
        self.stage = Stage::Sections;
        Ok(R2007Step::Framing)
    }

    fn pump_sec(&mut self) -> Result<R2007Step, StreamError> {
        let status = self.sec.status()?;
        match (self.sec.pass, status) {
            (Pass::Verify, Status::Finished) => {
                self.sec.verdict()?;
                self.sec.restart(Pass::Emit);
                Ok(R2007Step::Framing)
            }
            (Pass::Verify, Status::Work) => {
                self.sec.pump(STAGE)?;
                Ok(R2007Step::Framing)
            }
            (_, Status::Finished) => self.finish_walk(),
            (_, Status::Work) => self.pump_record(),
            _ => Err(StreamError::Finished),
        }
    }

    fn record_size(&self) -> usize {
        match self.rec_kind {
            Rec::Descriptor => DESCRIPTOR,
            Rec::Name => usize::from(self.cur.raw_name),
            Rec::Reference => REFERENCE,
        }
    }

    fn pump_record(&mut self) -> Result<R2007Step, StreamError> {
        let total = self.record_size();
        let left = self.sec.geom.plain - self.sec.produced;
        let need = ((total - self.rec_have) as u64).min(left) as usize;
        let n = self.sec.pump(need)?;
        match self.rec_kind {
            Rec::Name => {
                self.name[self.rec_have..self.rec_have + n].copy_from_slice(&self.sec.stage[..n])
            }
            _ => self.rec[self.rec_have..self.rec_have + n].copy_from_slice(&self.sec.stage[..n]),
        }
        self.rec_have += n;
        if self.rec_have < total {
            if self.sec.produced == self.sec.geom.plain {
                return Err(StreamError::MapLength(self.sec.geom.base));
            }
            return Ok(R2007Step::Framing);
        }
        self.rec_have = 0;
        match self.rec_kind {
            Rec::Descriptor => self.on_descriptor(),
            Rec::Name => self.on_name(),
            Rec::Reference => self.on_reference(),
        }
    }

    fn on_descriptor(&mut self) -> Result<R2007Step, StreamError> {
        let r = &self.rec;
        let index = self.sections;
        let (size, max, encryption, hash) =
            (u64_at(r, 0), u64_at(r, 8), u64_at(r, 16), u64_at(r, 24));
        let (raw_name, encoding, pages) = (u64_at(r, 32), u64_at(r, 48), u64_at(r, 56));
        self.described = self
            .described
            .checked_add(1)
            .ok_or(StreamError::CounterOverflow)?;
        if u64::from(self.described) > self.hdr[20] {
            return Err(StreamError::SectionEntry(index));
        }
        if raw_name == 0 {
            // The unnamed descriptor that makes `SectionsAmount` one more
            // than the sections; it describes no page.
            if pages != 0 {
                return Err(StreamError::SectionEntry(index));
            }
            return Ok(R2007Step::Framing);
        }
        if raw_name as usize > MAX_NAME || raw_name % 2 != 0 {
            return Err(StreamError::SectionEntry(index));
        }
        if !matches!(encoding, 1 | 4) {
            return Err(StreamError::SectionEntry(index));
        }
        if !matches!(encryption, 0 | 2) {
            return Err(StreamError::SectionEncrypted(index));
        }
        self.cur = Cur {
            index,
            raw_name: raw_name as u8,
            name_len: 0,
            name: [0; MAX_NAME],
            size,
            max,
            encryption,
            hash,
            encoding,
            pages,
            done: 0,
            end: 0,
        };
        self.name = [0; MAX_NAME];
        self.rec_kind = Rec::Name;
        Ok(R2007Step::Framing)
    }

    fn on_name(&mut self) -> Result<R2007Step, StreamError> {
        let index = self.cur.index;
        let raw = usize::from(self.cur.raw_name);
        let mut units = [0u16; MAX_NAME / 2];
        let count = raw / 2;
        for (k, unit) in units[..count].iter_mut().enumerate() {
            *unit = u16::from_le_bytes([self.name[2 * k], self.name[2 * k + 1]]);
        }
        if core::char::decode_utf16(units[..count].iter().copied()).any(|c| c.is_err()) {
            return Err(StreamError::SectionEntry(index));
        }
        let mut len = count;
        while len > 0 && units[len - 1] == 0 {
            len -= 1;
        }
        if len == 0 {
            // The legacy reader drops a section whose name is empty; it may
            // have no pages.
            if self.cur.pages != 0 {
                return Err(StreamError::SectionEntry(index));
            }
            self.cur = Cur::NONE;
            self.name = [0; MAX_NAME];
            self.rec_kind = Rec::Descriptor;
            return Ok(R2007Step::Framing);
        }
        self.cur.name_len = (2 * len) as u8;
        self.cur.name[..2 * len].copy_from_slice(&self.name[..2 * len]);
        self.name = [0; MAX_NAME];
        self.sections += 1;
        let c = &self.cur;
        let section = R2007Section {
            index: c.index,
            size: c.size,
            max_size: c.max,
            encryption: c.encryption,
            hash_code: c.hash,
            encoding: c.encoding,
            page_count: c.pages,
            name: c.name,
            name_len: c.name_len,
        };
        if c.pages == 0 {
            self.pending = Pending::SectionEnd {
                index: c.index,
                covered: 0,
            };
            self.rec_kind = Rec::Descriptor;
        } else {
            self.rec_kind = Rec::Reference;
        }
        Ok(R2007Step::Section(section))
    }

    fn on_reference(&mut self) -> Result<R2007Step, StreamError> {
        let r = &self.rec;
        let index = self.cur.index;
        let reference = Ref {
            data_offset: u64_at(r, 0),
            size: u64_at(r, 8),
            id: u64_at(r, 16) as i64,
            uncompressed: u64_at(r, 24),
            compressed: u64_at(r, 32),
            checksum: u64_at(r, 40),
            crc: u64_at(r, 48),
            address: 0,
            entry_size: 0,
        };
        if reference.id == 0
            || reference.compressed > reference.uncompressed
            || reference.data_offset < self.cur.end
            || reference
                .data_offset
                .checked_add(reference.uncompressed)
                .is_none()
        {
            return Err(StreamError::SectionEntry(index));
        }
        self.refd = reference;
        self.rec = [0; DESCRIPTOR];
        let geom = self.page_map_geom()?;
        self.aux.begin(geom, Pass::Scan);
        self.facts.acc = 0;
        self.entry_have = 0;
        self.stage = Stage::Lookup;
        Ok(R2007Step::Framing)
    }

    /// The page's checks all passed: say so, then walk it again for its bytes.
    fn page_verified(&mut self) -> Result<R2007Step, StreamError> {
        let (c, r, g) = (&self.cur, &self.refd, &self.aux.geom);
        let page = R2007Page {
            section: c.index,
            id: r.id,
            address: r.address,
            data_offset: r.data_offset,
            size: r.size,
            uncompressed: r.uncompressed,
            compressed: r.compressed,
            checksum: r.checksum,
            crc: r.crc,
            codewords: g.blocks,
        };
        let from = c.end.min(c.size);
        let to = r.data_offset.min(c.size);
        if to > from {
            self.pending = Pending::Hole {
                offset: from,
                len: to - from,
            };
        }
        self.emitted = 0;
        self.aux.restart(Pass::Emit);
        Ok(R2007Step::Page(page))
    }

    fn pump_data(&mut self) -> Result<R2007Step, StreamError> {
        let before = self.aux.produced;
        let n = self.aux.pump(STAGE)?;
        let offset = self.refd.data_offset + before;
        let size = self.cur.size;
        let keep = if offset >= size {
            0
        } else {
            (n as u64).min(size - offset) as usize
        };
        if keep == 0 {
            return Ok(R2007Step::Framing);
        }
        self.out_len = keep;
        self.emitted += keep as u64;
        Ok(R2007Step::Data { offset, len: keep })
    }

    fn page_done(&mut self) -> Result<R2007Step, StreamError> {
        let plain = self.refd.uncompressed;
        let end = self
            .refd
            .data_offset
            .checked_add(plain)
            .ok_or(StreamError::SectionEntry(self.cur.index))?;
        self.cur.end = end;
        self.cur.done += 1;
        self.pages = self
            .pages
            .checked_add(1)
            .ok_or(StreamError::CounterOverflow)?;
        let emitted = self.emitted;
        self.emitted = 0;
        self.refd = Ref::default();
        self.aux.clear();
        self.stage = Stage::Sections;
        if self.cur.done == self.cur.pages {
            self.pending = Pending::SectionEnd {
                index: self.cur.index,
                covered: self.cur.end.min(self.cur.size),
            };
            self.rec_kind = Rec::Descriptor;
        }
        Ok(R2007Step::PageEnd { plain, emitted })
    }

    /// The section map's stream ended on a record boundary.
    fn finish_walk(&mut self) -> Result<R2007Step, StreamError> {
        if self.rec_have != 0 || self.rec_kind != Rec::Descriptor {
            return Err(StreamError::MapLength(self.sec.geom.base));
        }
        if u64::from(self.described) != self.hdr[20] {
            return Err(StreamError::SectionEntry(self.described));
        }
        self.sec.clear();
        self.cur = Cur::NONE;
        self.stage = Stage::Done;
        Ok(R2007Step::Finished {
            sections: self.sections,
            pages: self.pages,
        })
    }
}

/// §5.2: the 0x110-byte header, checked on its own: its CRC-64 and the
/// fields the spec fixes. Used by the walk and by checkpoint restore.
pub(super) fn check_plain_header(
    plain: &[u8; PLAIN],
    length: u64,
) -> Result<[u64; 34], StreamError> {
    let mut fields = [0u64; 34];
    for (k, field) in fields.iter_mut().enumerate() {
        *field = u64_at(plain, k * 8);
    }
    let mut zeroed = *plain;
    zeroed[0x108..].fill(0);
    let mut crc = Crc64::new(seed2(0, PLAIN as u64), false);
    crc.update(&zeroed);
    let (expected, actual) = (fields[33], crc.finish());
    if actual != expected {
        return Err(StreamError::R2007Crc64 {
            at: BLOCK_AT as u64,
            check: 2,
            expected,
            actual,
        });
    }
    for (k, value) in [
        (0, 0x70),
        (14, 0x20),
        (15, 0x40),
        (17, 0xf800),
        (18, 4),
        (19, 1),
        (29, 0x60100),
    ] {
        if fields[k] != value {
            return Err(StreamError::R2007Header(AT_HEADER + (k * 8) as u16));
        }
    }
    if fields[1] > length {
        return Err(StreamError::R2007Header(AT_HEADER + 0x08));
    }
    if fields[3] == 0 {
        return Err(StreamError::R2007Header(AT_HEADER + 0x18));
    }
    let second = fields[9]
        .checked_add(PAGE_BASE + BLOCK_BYTES as u64)
        .filter(|&end| end <= length);
    if second.is_none() {
        return Err(StreamError::R2007Header(AT_HEADER + 0x48));
    }
    if fields[27] == 0 {
        return Err(StreamError::R2007Header(AT_HEADER + 0xd8));
    }
    Ok(fields)
}

impl Pass {
    pub(super) fn from_u8(byte: u8) -> Option<Pass> {
        match byte {
            0 => Some(Pass::Verify),
            1 => Some(Pass::Emit),
            2 => Some(Pass::Scan),
            _ => None,
        }
    }
}

impl Stage {
    pub(super) fn from_u8(byte: u8) -> Option<Stage> {
        match byte {
            0 => Some(Stage::Head),
            1 => Some(Stage::Map),
            2 => Some(Stage::Sections),
            3 => Some(Stage::Lookup),
            4 => Some(Stage::Page),
            5 => Some(Stage::Done),
            _ => None,
        }
    }
}

impl Rec {
    pub(super) fn from_u8(byte: u8) -> Option<Rec> {
        match byte {
            0 => Some(Rec::Descriptor),
            1 => Some(Rec::Name),
            2 => Some(Rec::Reference),
            _ => None,
        }
    }
}

impl R2007Walker {
    /// Set the geometry of each reader a restored stage uses, from the
    /// verified header, the page map's facts and the page reference.
    pub(super) fn derive_geometry(&mut self, stage: Stage) -> Result<(), StreamError> {
        match stage {
            Stage::Head | Stage::Done => (),
            Stage::Map => self.aux.geom = self.page_map_geom()?,
            Stage::Sections => self.sec.geom = self.section_map_geom()?,
            Stage::Lookup => {
                self.sec.geom = self.section_map_geom()?;
                self.aux.geom = self.page_map_geom()?;
            }
            Stage::Page => {
                self.sec.geom = self.section_map_geom()?;
                self.aux.geom = self.data_geom()?;
            }
        }
        Ok(())
    }

    /// The section being described or walked is one the parser can be in.
    fn section_ok(&self) -> bool {
        let c = &self.cur;
        let named = c.raw_name >= 2
            && usize::from(c.raw_name) <= MAX_NAME
            && c.raw_name % 2 == 0
            && matches!(c.encoding, 1 | 4)
            && matches!(c.encryption, 0 | 2);
        let trimmed = usize::from(c.name_len) <= usize::from(c.raw_name)
            && usize::from(c.name_len) <= MAX_NAME
            && c.name_len % 2 == 0
            && c.name[usize::from(c.name_len)..] == [0; MAX_NAME][usize::from(c.name_len)..];
        match self.rec_kind {
            Rec::Descriptor => match self.pending {
                Pending::None => *c == Cur::NONE && self.rec_have < DESCRIPTOR,
                Pending::SectionEnd { index, covered } => {
                    named
                        && trimmed
                        && c.name_len > 0
                        && index == c.index
                        && c.done == c.pages
                        && covered == c.end.min(c.size)
                        && self.rec_have == 0
                        && c.index + 1 == self.sections
                }
                Pending::Hole { .. } => false,
            },
            Rec::Name => {
                named
                    && c.name_len == 0
                    && c.name == [0; MAX_NAME]
                    && c.done == 0
                    && c.end == 0
                    && c.index == self.sections
                    && self.rec_have < usize::from(c.raw_name)
                    && self.pending == Pending::None
            }
            Rec::Reference => {
                named
                    && trimmed
                    && c.name_len > 0
                    && c.index + 1 == self.sections
                    && c.done < c.pages
                    && self.rec_have < REFERENCE
                    && matches!(self.pending, Pending::None | Pending::Hole { .. })
            }
        }
    }

    fn reference_ok(&self) -> bool {
        let r = &self.refd;
        r.id != 0
            && r.compressed <= r.uncompressed
            && r.data_offset >= self.cur.end
            && r.data_offset.checked_add(r.uncompressed).is_some()
    }

    /// Restore-time check that a decoded state is one the walk can reach.
    /// The header has been re-verified and each reader's geometry derived;
    /// this ties the readers, the cursors and the stage to each other.
    pub(super) fn consistent(&self) -> bool {
        if self.failure.is_some() || self.out_len != 0 {
            return false;
        }
        let h = &self.hdr;
        let blank_parser = self.rec_have == 0
            && self.rec_kind == Rec::Descriptor
            && self.cur == Cur::NONE
            && self.pending == Pending::None
            && self.refd == Ref::default()
            && self.described == 0
            && self.sections == 0
            && self.pages == 0
            && self.emitted == 0;
        if self.stage == Stage::Head {
            return self.have < HEAD_BYTES
                && self.head[self.have..] == [0; HEAD_BYTES][self.have..]
                && *h == [0; 34]
                && self.aux.is_blank()
                && self.sec.is_blank()
                && self.facts == MapFacts::default()
                && self.entry_have == 0
                && blank_parser;
        }
        if self.have != 0
            || self.head != [0; HEAD_BYTES]
            || *h == [0; 34]
            || u64::from(self.described) > h[20]
            || self.sections > self.described
        {
            return false;
        }
        let map_done = self.facts.sm_found
            && self.facts.self_seen
            && self.facts.sm_address >= PAGE_BASE
            && self.facts.entries == h[12]
            && self.facts.max_id == h[13];
        let a = &self.aux;
        let s = &self.sec;
        match self.stage {
            Stage::Map => {
                let walked = if a.pass == Pass::Verify {
                    self.facts == MapFacts::default() && self.entry_have == 0
                } else {
                    a.pass == Pass::Emit
                        && self.entry_have as u64 == a.produced % ENTRY as u64
                        && self.facts.entries == a.produced / ENTRY as u64
                        && (!self.facts.sm_found || self.facts.sm_address >= PAGE_BASE)
                };
                matches!(a.pass, Pass::Verify | Pass::Emit)
                    && a.progress_ok()
                    && s.is_blank()
                    && walked
                    && blank_parser
            }
            Stage::Sections => {
                s.progress_ok()
                    && a.is_blank()
                    && map_done
                    && self.entry_have == 0
                    && self.emitted == 0
                    && match s.pass {
                        Pass::Verify => blank_parser,
                        Pass::Emit => {
                            u64::from(self.described) >= u64::from(self.sections)
                                && u64::from(self.described) <= h[20]
                                && self.refd == Ref::default()
                                && self.section_ok()
                        }
                        Pass::Scan => false,
                    }
            }
            Stage::Lookup => {
                s.pass == Pass::Emit
                    && s.progress_ok()
                    && a.pass == Pass::Scan
                    && a.progress_ok()
                    && map_done
                    && self.rec_kind == Rec::Reference
                    && self.rec_have == 0
                    && self.section_ok()
                    && self.reference_ok()
                    && self.refd.address == 0
                    && self.refd.entry_size == 0
                    && self.entry_have as u64 == a.produced % ENTRY as u64
                    && self.emitted == 0
                    && self.pending == Pending::None
            }
            Stage::Page => {
                let hole = match self.pending {
                    Pending::None => true,
                    Pending::Hole { offset, len } => {
                        let from = self.cur.end.min(self.cur.size);
                        let to = self.refd.data_offset.min(self.cur.size);
                        a.pass == Pass::Emit
                            && a.produced == 0
                            && offset == from
                            && to > from
                            && len == to - from
                    }
                    Pending::SectionEnd { .. } => false,
                };
                s.pass == Pass::Emit
                    && s.progress_ok()
                    && matches!(a.pass, Pass::Verify | Pass::Emit)
                    && a.progress_ok()
                    && map_done
                    && self.rec_kind == Rec::Reference
                    && self.rec_have == 0
                    && self.section_ok()
                    && self.reference_ok()
                    && self.refd.address >= PAGE_BASE
                    && self.refd.entry_size > 0
                    && self.entry_have == 0
                    && hole
                    && self.emitted <= a.produced
                    && (a.pass == Pass::Emit || self.emitted == 0)
            }
            Stage::Done => {
                a.is_blank()
                    && s.is_blank()
                    && map_done
                    && self.entry_have == 0
                    && self.rec_have == 0
                    && self.rec_kind == Rec::Descriptor
                    && self.cur == Cur::NONE
                    && self.pending == Pending::None
                    && self.refd == Ref::default()
                    && self.emitted == 0
                    && u64::from(self.described) == h[20]
                    && self.sections <= self.described
            }
            Stage::Head => false,
        }
    }
}
