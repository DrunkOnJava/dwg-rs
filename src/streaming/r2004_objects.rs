//! The object records of an R2004-family file (ODA v5.4.1 §20.1) streamed out
//! of the whole-file walk. [`R2004ObjectWalker`] is the file walker of
//! [`super::r2004_walk`] with one section singled out: the bytes of
//! `AcDb:AcDbObjects` do not leave it as section data but are framed, in
//! section order, into records by [`super::records`]. Every other section's
//! data passes through as the file walker reports it. A record's payload
//! (everything between its `MS` and its CRC, an R2010 `MC` included, the
//! bytes the legacy `RawObject::raw` holds) is handed out as it arrives and
//! is provisional until the step that commits the record after its CRC, type
//! and handle checks. Records come out in stream order, each with the handle
//! it carries; the `AcDb:Handles` map is not read.
//!
//! The file walker visits data pages in page-map order and the framer needs
//! the section's bytes in section order, so a page of the object section that
//! the page map lists out of order, or a hole in it, is refused with
//! `ObjectsOrder` rather than buffered or reordered: the state is fixed and
//! no page index is kept. Every object section of the pinned corpus is in
//! order.

use super::flat::ObjectRecord;
use super::r2004_sections::STAGE;
use super::r2004_walk::{R2004Walker, WalkDemand, WalkStep};
use super::records::{Frame, ObjectFramer, RecordFormat};
use super::{Request, StreamError};

/// The section the records live in.
pub const OBJECTS_SECTION: &[u8] = b"AcDb:AcDbObjects";

/// What one feed or pump produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStep {
    /// A step of the file walk. The object section's own `Data` steps are
    /// consumed here and reported as `Walk(Framing)`.
    Walk(WalkStep),
    /// `len` bytes of the open record's payload, provisional, in
    /// [`R2004ObjectWalker::data`] until the next call.
    Payload { len: usize },
    /// A record whose CRC, type and handle passed; the payload steps since
    /// the last `Object` were its bytes.
    Object(ObjectRecord),
    /// The stream ended on a record boundary at the section's size.
    Objects { records: u64, bytes: u64 },
}

/// The walker's next need: source bytes, a pump with no source bytes, or
/// nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectDemand {
    Read(Request),
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
pub struct R2004ObjectWalker {
    pub(super) inner: R2004Walker,
    pub(super) found: bool,
    pub(super) section_id: u32,
    pub(super) framer: ObjectFramer,
    pub(super) pending: [u8; STAGE],
    pub(super) pending_len: usize,
    pub(super) pending_pos: usize,
    emit: Emit,
    pub(super) failure: Option<StreamError>,
}

fn is_objects(name: &[u8; 64]) -> bool {
    name.starts_with(OBJECTS_SECTION) && name[OBJECTS_SECTION.len()] == 0
}

impl R2004ObjectWalker {
    /// Start at byte 0 without opening or allocating the source.
    pub fn new(source: super::SourceIdentity) -> Self {
        Self::from_walker(R2004Walker::new(source))
    }

    pub(super) fn from_walker(inner: R2004Walker) -> Self {
        Self {
            inner,
            found: false,
            section_id: 0,
            framer: ObjectFramer::idle(),
            pending: [0; STAGE],
            pending_len: 0,
            pending_pos: 0,
            emit: Emit::None,
            failure: None,
        }
    }

    /// Source identity is part of the state and checkpoint contract.
    pub fn source(&self) -> super::SourceIdentity {
        self.inner.source()
    }

    /// The verified file header, once the walk has passed it.
    pub fn header(&self) -> Option<super::R2004Header> {
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

    /// The record layout of the file's release, once its header is verified.
    pub fn format(&self) -> RecordFormat {
        match self.inner.header() {
            Some(header) if header.magic == *b"AC1018" => RecordFormat::R2000,
            _ => RecordFormat::R2010,
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
    pub fn demand(&self) -> Result<ObjectDemand, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if self.buffered() {
            return Ok(ObjectDemand::Pump);
        }
        Ok(match self.inner.demand()? {
            WalkDemand::Read(request) => ObjectDemand::Read(request),
            WalkDemand::Pump => ObjectDemand::Pump,
            WalkDemand::Done => ObjectDemand::Done,
        })
    }

    /// Consume one nonempty prefix of a `Read` demand. Invalid delivery
    /// leaves state unchanged; a grammar failure is terminal.
    pub fn feed(&mut self, offset: u64, input: &[u8]) -> Result<ObjectStep, StreamError> {
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
    pub fn pump(&mut self) -> Result<ObjectStep, StreamError> {
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

    /// Reject the end of the source while a `Read` is outstanding.
    pub fn end_of_input(&self) -> Result<(), StreamError> {
        match self.demand()? {
            ObjectDemand::Read(r) => Err(StreamError::Truncated {
                offset: r.offset,
                needed: r.len,
            }),
            _ => Ok(()),
        }
    }

    fn settle(
        &mut self,
        result: Result<ObjectStep, StreamError>,
    ) -> Result<ObjectStep, StreamError> {
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn on_walk(&mut self, step: WalkStep) -> Result<ObjectStep, StreamError> {
        match step {
            WalkStep::Section(section) if is_objects(&section.name) => {
                if self.found {
                    return Err(StreamError::SectionEntry(section.index));
                }
                self.framer = ObjectFramer::begin(self.format(), section.size)?;
                self.found = true;
                self.section_id = section.section_id;
                Ok(ObjectStep::Walk(step))
            }
            WalkStep::Data {
                section_id, len, ..
            } if self.found && section_id == self.section_id => {
                let WalkStep::Data { offset, .. } = step else {
                    return Err(StreamError::Finished);
                };
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
                Ok(ObjectStep::Walk(WalkStep::Framing))
            }
            WalkStep::Data { .. } => {
                self.emit = Emit::Inner;
                Ok(ObjectStep::Walk(step))
            }
            WalkStep::Finished { .. } => {
                if !self.found {
                    return Err(StreamError::ObjectsMissing);
                }
                if self.framer.stage != Frame::Ended {
                    return Err(StreamError::ObjectsIncomplete(self.framer.at));
                }
                Ok(ObjectStep::Walk(step))
            }
            other => Ok(ObjectStep::Walk(other)),
        }
    }

    /// Frame the bytes held, up to the next thing the framer reports.
    fn frame(&mut self) -> Result<ObjectStep, StreamError> {
        if self.framer.stage == Frame::Done {
            self.framer.stage = Frame::Ended;
            return Ok(ObjectStep::Objects {
                records: self.framer.records,
                bytes: self.framer.total,
            });
        }
        let start = self.pending_pos;
        let (n, event) = self.framer.feed(&self.pending[start..self.pending_len])?;
        self.pending_pos += n;
        Ok(match event {
            super::records::Event::Framing => ObjectStep::Walk(WalkStep::Framing),
            super::records::Event::Payload => {
                self.emit = Emit::Pending { start, len: n };
                ObjectStep::Payload { len: n }
            }
            super::records::Event::Object(record) => ObjectStep::Object(record),
        })
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
        if !self.found {
            return self.framer == ObjectFramer::idle()
                && self.section_id == 0
                && self.pending_len == 0
                && self.pending_pos == 0;
        }
        let slot = self.inner.slots[..self.inner.slot_count]
            .iter()
            .find(|slot| slot.id == self.section_id);
        let tied = remaining == 0
            || (self.inner.stage == super::r2004_walk::Stage::Walk
                && self.inner.page.section_id() == self.section_id
                && self.framer.at + remaining as u64 == self.inner.page.cursor);
        slot.is_some_and(|slot| slot.size == self.framer.total)
            && self.framer.format == self.format()
            && self.framer.consistent()
            && self.framer.at + remaining as u64 <= self.framer.total
            && (remaining == 0 || !matches!(self.framer.stage, Frame::Done | Frame::Ended))
            && tied
    }
}
