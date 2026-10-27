//! The object records of an R2007 (`AC1021`) file (ODA v5.4.1 §20.1) streamed
//! out of the whole-file walk. [`R2007ObjectWalker`] is the file walker of
//! [`super::r2007`] with one section singled out, as [`super::r2004_objects`]
//! does for the R2004 family: the bytes of `AcDb:AcDbObjects` do not leave it
//! as section data but are framed, in section order, into records by
//! [`super::records`]. Every other section's data passes through as the file
//! walker reports it. A record is `MS` size, `MS` bytes of payload and a CRC,
//! the layout of R2000 (`RecordFormat::R2000`): on every object section of the
//! pinned corpus (ARCHITECTURE.md §7am) the records tile the section exactly
//! after its four-byte prologue and every CRC and every handle agrees with the
//! `AcDb:Handles` map. A record's payload (everything between its `MS` and its
//! CRC, the bytes the legacy `RawObject::raw` holds) is handed out as it
//! arrives, and only after the page it lies in passed every Reed-Solomon,
//! CRC-64 and checksum test; it is provisional until the step that commits the
//! record after its own CRC, type and handle checks. Records come out in stream
//! order with the handle each carries; the `AcDb:Handles` map is not read.
//!
//! The file walker hands out each section's pages in the order the section map
//! lists them, which is the order of the section's bytes, so a page of the
//! object section never arrives out of order. A hole in it (a stretch no page
//! covers) is refused with `ObjectsOrder` rather than framed as zeros: the
//! records tile the section, so a gap is a fault, and the state is fixed and
//! keeps no page index.

use super::flat::ObjectRecord;
use super::r2004_objects::OBJECTS_SECTION;
use super::r2007::{
    Cur, Gather, MAX_NAME, Pass, Pending, R2007Demand, R2007Header, R2007Section, R2007Step,
    R2007Walker, STAGE, Stage,
};
use super::records::{Event, Frame, ObjectFramer, RecordFormat};
use super::{Request, SourceIdentity, StreamError};

/// What one feed or pump produced. A step is returned by value once per call,
/// and the walk step that makes the enum large (the verified 272-byte file
/// header) comes once per file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum R2007ObjectStep {
    /// A step of the file walk. The object section's own `Data` steps are
    /// consumed here and reported as `Walk(Framing)`.
    Walk(R2007Step),
    /// `len` bytes of the open record's payload, provisional, in
    /// [`R2007ObjectWalker::data`] until the next call.
    Payload { len: usize },
    /// A record whose CRC, type and handle passed; the payload steps since
    /// the last `Object` were its bytes.
    Object(ObjectRecord),
    /// The stream ended on a record boundary at the section's size.
    Objects { records: u64, bytes: u64 },
}

/// The walker's next need: source bytes read contiguously, the bytes of a
/// codeword gathered across a page, a pump with no source bytes, or nothing
/// more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R2007ObjectDemand {
    Read(Request),
    Gather(Gather),
    Pump,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Emit {
    None,
    Inner,
    Pending { start: usize, len: usize },
}

/// Fixed-size state for the whole walk and the open record.
#[derive(Debug, Clone)]
pub struct R2007ObjectWalker {
    pub(super) inner: R2007Walker,
    pub(super) found: bool,
    pub(super) section: u32,
    pub(super) framer: ObjectFramer,
    pub(super) pending: [u8; STAGE],
    pub(super) pending_len: usize,
    pub(super) pending_pos: usize,
    emit: Emit,
    pub(super) failure: Option<StreamError>,
}

/// Whether a UTF-16LE section name, `len` bytes of it, is the object section's.
fn is_objects_name(name: &[u8; MAX_NAME], len: u8) -> bool {
    let n = usize::from(len);
    n == 2 * OBJECTS_SECTION.len()
        && name[..n]
            .chunks_exact(2)
            .zip(OBJECTS_SECTION)
            .all(|(unit, &byte)| unit[0] == byte && unit[1] == 0)
}

fn is_objects(section: &R2007Section) -> bool {
    is_objects_name(&section.name, section.name_len)
}

impl R2007ObjectWalker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: SourceIdentity) -> Self {
        Self::from_walker(R2007Walker::new(source))
    }

    pub(super) fn from_walker(inner: R2007Walker) -> Self {
        Self {
            inner,
            found: false,
            section: 0,
            framer: ObjectFramer::idle(),
            pending: [0; STAGE],
            pending_len: 0,
            pending_pos: 0,
            emit: Emit::None,
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> SourceIdentity {
        self.inner.source()
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<R2007Header> {
        self.inner.header()
    }

    /// The bytes the last `Payload` step carried, or the section bytes the
    /// last pass-through `Data` step did; empty after any other step.
    pub fn data(&self) -> &[u8] {
        match self.emit {
            Emit::None => &[],
            Emit::Inner => self.inner.data(),
            Emit::Pending { start, len } => &self.pending[start..start + len],
        }
    }

    /// Records committed so far.
    pub fn records(&self) -> u64 {
        self.framer.records
    }

    fn buffered(&self) -> bool {
        self.pending_pos < self.pending_len || self.framer.stage == Frame::Done
    }

    /// The next need. Pure: it changes nothing.
    pub fn demand(&self) -> Result<R2007ObjectDemand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.buffered() {
            return Ok(R2007ObjectDemand::Pump);
        }
        Ok(match self.inner.demand()? {
            R2007Demand::Read(request) => R2007ObjectDemand::Read(request),
            R2007Demand::Gather(gather) => R2007ObjectDemand::Gather(gather),
            R2007Demand::Pump => R2007ObjectDemand::Pump,
            R2007Demand::Done => R2007ObjectDemand::Done,
        })
    }

    /// Consume one nonempty prefix of a `Read` or `Gather` demand. Invalid
    /// delivery leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<R2007ObjectStep, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.buffered() {
            return Err(StreamError::Finished);
        }
        self.emit = Emit::None;
        let step = self.inner.feed(offset, input)?;
        let result = self.on_walk(step);
        self.settle(result)
    }

    /// Advance a `Pump` demand: no source bytes are involved.
    pub fn pump(&mut self) -> Result<R2007ObjectStep, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.emit = Emit::None;
        let result = if self.buffered() {
            self.frame()
        } else {
            let step = self.inner.pump()?;
            self.on_walk(step)
        };
        self.settle(result)
    }

    /// Reject the end of the source while a `Read` or `Gather` is outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            R2007ObjectDemand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            R2007ObjectDemand::Gather(g) => Err(StreamError::Truncated {
                offset: g.offset,
                needed: g.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(
        &mut self,
        result: Result<R2007ObjectStep, StreamError>,
    ) -> Result<R2007ObjectStep, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn in_objects(&self) -> bool {
        self.found && self.inner.cur.raw_name != 0 && self.inner.cur.index == self.section
    }

    fn on_walk(&mut self, step: R2007Step) -> Result<R2007ObjectStep, StreamError> {
        match step {
            R2007Step::Section(section) if is_objects(&section) => {
                if self.found {
                    return Err(StreamError::SectionEntry(section.index));
                }
                self.framer = ObjectFramer::begin(RecordFormat::R2000, section.size)?;
                self.found = true;
                self.section = section.index;
                Ok(R2007ObjectStep::Walk(step))
            }
            R2007Step::Data { offset, len } if self.in_objects() => {
                if offset != self.framer.at {
                    return Err(StreamError::ObjectsOrder {
                        expected: self.framer.at,
                        actual: offset,
                    });
                }
                let bytes = self.inner.data();
                if bytes.len() != len || len > STAGE {
                    return Err(StreamError::InvalidFeed {
                        maximum: STAGE,
                        actual: len,
                    });
                }
                self.pending[..len].copy_from_slice(bytes);
                self.pending_len = len;
                self.pending_pos = 0;
                Ok(R2007ObjectStep::Walk(R2007Step::Framing))
            }
            R2007Step::Hole { offset, len } if self.in_objects() => {
                Err(StreamError::ObjectsOrder {
                    expected: self.framer.at,
                    actual: offset.saturating_add(len),
                })
            }
            R2007Step::Data { .. } => {
                self.emit = Emit::Inner;
                Ok(R2007ObjectStep::Walk(step))
            }
            R2007Step::Finished { .. } => {
                if !self.found {
                    return Err(StreamError::ObjectsMissing);
                }
                if self.framer.stage != Frame::Ended {
                    return Err(StreamError::ObjectsIncomplete(self.framer.at));
                }
                Ok(R2007ObjectStep::Walk(step))
            }
            other => Ok(R2007ObjectStep::Walk(other)),
        }
    }

    /// Frame the bytes held, up to the next thing the framer reports.
    fn frame(&mut self) -> Result<R2007ObjectStep, StreamError> {
        if self.framer.stage == Frame::Done {
            self.framer.stage = Frame::Ended;
            return Ok(R2007ObjectStep::Objects {
                records: self.framer.records,
                bytes: self.framer.total,
            });
        }
        let start = self.pending_pos;
        let (n, event) = self.framer.feed(&self.pending[start..self.pending_len])?;
        self.pending_pos += n;
        Ok(match event {
            Event::Framing => R2007ObjectStep::Walk(R2007Step::Framing),
            Event::Payload => {
                self.emit = Emit::Pending { start, len: n };
                R2007ObjectStep::Payload { len: n }
            }
            Event::Object(record) => R2007ObjectStep::Object(record),
        })
    }

    /// The section offset of the next byte the file walk hands out of the
    /// object section, when it is inside that section and nothing is held.
    fn next_offset(&self) -> u64 {
        let (w, c) = (&self.inner, &self.inner.cur);
        let from = c.end.min(c.size);
        if w.stage == Stage::Page && w.aux.pass == Pass::Emit && w.pending == Pending::None {
            w.refd
                .data_offset
                .saturating_add(w.aux.produced)
                .min(c.size)
        } else {
            from
        }
    }

    /// Restore-time check that a decoded state is one the walk can reach.
    /// The embedded file walker has been validated by its own restore; this
    /// ties the framer to it.
    pub(super) fn consistent(&self) -> bool {
        let remaining = self.pending_len.saturating_sub(self.pending_pos);
        if self.failure.is_some()
            || self.pending_len > STAGE
            || self.pending_pos > self.pending_len
            || self.pending[self.pending_len..].iter().any(|&b| b != 0)
        {
            return false;
        }
        let c: &Cur = &self.inner.cur;
        let named = c.raw_name != 0;
        if !self.found {
            return self.framer == ObjectFramer::idle()
                && self.section == 0
                && self.pending_len == 0
                && self.pending_pos == 0
                && !(named && is_objects_name(&c.name, c.name_len));
        }
        if !self.framer.consistent()
            || self.framer.format != RecordFormat::R2000
            || self.inner.sections <= self.section
        {
            return false;
        }
        if named && c.index == self.section {
            is_objects_name(&c.name, c.name_len)
                && c.size == self.framer.total
                && self.framer.at + remaining as u64 == self.next_offset()
                && (remaining == 0 || !matches!(self.framer.stage, Frame::Done | Frame::Ended))
        } else {
            (!named || c.index > self.section) && remaining == 0 && self.framer.stage != Frame::Done
        }
    }
}
