//! Pausable byte delivery. Grammar declares ranges; only adapters perform I/O.

use crate::streaming::{
    CHUNK_BYTES, FlatHeaderParser, FlatWalker, MapDemand, MapStep, R2004HeaderParser,
    R2004MapWalker, R2004SectionWalker, R2004Walker, R2007Demand, R2007Step, R2007Walker, Request,
    SectionDemand, SectionStep, Step, StreamError, WalkDemand, WalkStep,
};
use std::io::{self, Read, Seek, SeekFrom};

/// `Pending` consumes nothing; EOF is an explicit terminal source observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePoll {
    Ready(usize),
    Pending,
    Eof,
}

/// A local file, range fetcher or worker may satisfy the same exact demand.
/// Implementations must not write or report more than `min(request.len,
/// output.len())` bytes. Short reads are allowed. Pending callbacks should
/// wake the caller; grammar never blocks or owns a runtime.
pub trait ByteSource {
    type Error;
    /// Poll one bounded read at an absolute source offset.
    fn poll_read(&mut self, request: Request, output: &mut [u8])
    -> Result<SourcePoll, Self::Error>;

    /// Poll the bytes at `offset`, `offset + stride`, ... into `output`, as
    /// many as are available (at least one on `Ready`). The R2007 walk
    /// gathers each Reed-Solomon codeword this way. The default issues one
    /// single-byte read per byte; a source that can serve the span at once
    /// should override it. `Pending` and `Eof` refer to the first byte.
    fn poll_gather(
        &mut self,
        offset: u64,
        stride: u64,
        output: &mut [u8],
    ) -> Result<SourcePoll, Self::Error>
    where
        Self: Sized,
    {
        gather_by_reads(self, offset, stride, output)
    }
}

/// Gather by single-byte reads: the default of [`ByteSource::poll_gather`].
pub fn gather_by_reads<S: ByteSource>(
    source: &mut S,
    offset: u64,
    stride: u64,
    output: &mut [u8],
) -> Result<SourcePoll, S::Error> {
    let mut got = 0;
    while got < output.len() {
        let request = Request {
            offset: offset + got as u64 * stride,
            len: 1,
        };
        match source.poll_read(request, &mut output[got..=got])? {
            SourcePoll::Ready(_) => got += 1,
            other if got == 0 => return Ok(other),
            _ => break,
        }
    }
    Ok(SourcePoll::Ready(got))
}

/// Result of a single scheduler-controlled poll, with no hidden read loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderPoll {
    Progress(usize),
    Pending,
    Complete,
}

/// Transport errors and grammar errors remain distinguishable.
#[derive(Debug)]
pub enum PumpError<E> {
    Source(E),
    Grammar(StreamError),
    InvalidRead { maximum: usize, actual: usize },
}

impl<E: core::fmt::Display> core::fmt::Display for PumpError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Source(e) => write!(f, "byte source: {e}"),
            Self::Grammar(e) => write!(f, "{e}"),
            Self::InvalidRead { maximum, actual } => write!(
                f,
                "source reported {actual} bytes for {maximum}-byte buffer"
            ),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PumpError<E> {}

/// Result of a single walk poll. `Fed` bytes are `scratch[..len]`, read
/// from `offset`; a `Payload` step leaves them with the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkPoll {
    Fed { step: Step, offset: u64, len: usize },
    Pending,
    Complete,
}

trait Demand {
    type Output;
    fn request(&self) -> Result<Option<Request>, StreamError>;
    fn feed(&mut self, offset: u64, input: &[u8]) -> Result<Self::Output, StreamError>;
    fn end_of_input(&self) -> Result<(), StreamError>;
}

impl Demand for FlatHeaderParser {
    type Output = ();
    fn request(&self) -> Result<Option<Request>, StreamError> {
        self.request()
    }
    fn feed(&mut self, offset: u64, input: &[u8]) -> Result<(), StreamError> {
        self.feed(offset, input)
    }
    fn end_of_input(&self) -> Result<(), StreamError> {
        self.end_of_input()
    }
}

impl Demand for R2004HeaderParser {
    type Output = ();
    fn request(&self) -> Result<Option<Request>, StreamError> {
        self.request()
    }
    fn feed(&mut self, offset: u64, input: &[u8]) -> Result<(), StreamError> {
        self.feed(offset, input)
    }
    fn end_of_input(&self) -> Result<(), StreamError> {
        self.end_of_input()
    }
}

impl Demand for FlatWalker {
    type Output = Step;
    fn request(&self) -> Result<Option<Request>, StreamError> {
        self.request()
    }
    fn feed(&mut self, offset: u64, input: &[u8]) -> Result<Step, StreamError> {
        self.feed(offset, input)
    }
    fn end_of_input(&self) -> Result<(), StreamError> {
        self.end_of_input()
    }
}

enum Polled<T> {
    Fed(Request, usize, T),
    Pending,
    Complete,
}

fn poll_once<G: Demand, S: ByteSource>(
    grammar: &mut G,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<Polled<G::Output>, PumpError<S::Error>> {
    let Some(request) = grammar.request().map_err(PumpError::Grammar)? else {
        return Ok(Polled::Complete);
    };
    let maximum = request.len.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    match source
        .poll_read(request, &mut scratch[..maximum])
        .map_err(PumpError::Source)?
    {
        SourcePoll::Pending => Ok(Polled::Pending),
        SourcePoll::Eof => {
            grammar.end_of_input().map_err(PumpError::Grammar)?;
            Ok(Polled::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            let output = grammar
                .feed(request.offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(Polled::Fed(request, actual, output))
        }
    }
}

/// Perform at most one transport poll, using caller-owned bounded scratch.
/// Completion refers to the file header, never to the whole model.
pub fn poll_header<S: ByteSource>(
    parser: &mut FlatHeaderParser,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<HeaderPoll, PumpError<S::Error>> {
    Ok(match poll_once(parser, source, scratch)? {
        Polled::Fed(_, actual, ()) => HeaderPoll::Progress(actual),
        Polled::Pending => HeaderPoll::Pending,
        Polled::Complete => HeaderPoll::Complete,
    })
}

/// One transport poll for the R2004-family file header (§4.1). Completion
/// refers to the header, never to the page maps or objects.
pub fn poll_r2004_header<S: ByteSource>(
    parser: &mut R2004HeaderParser,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<HeaderPoll, PumpError<S::Error>> {
    Ok(match poll_once(parser, source, scratch)? {
        Polled::Fed(_, actual, ()) => HeaderPoll::Progress(actual),
        Polled::Pending => HeaderPoll::Pending,
        Polled::Complete => HeaderPoll::Complete,
    })
}

/// Result of one R2004 map-walk poll: a step from source bytes or from a
/// pump (no source bytes), a pending source, or the end of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapPoll {
    Fed {
        step: MapStep,
        offset: u64,
        len: usize,
    },
    Pumped(MapStep),
    Pending,
    Complete,
}

/// One poll for the R2004 page map and section map walk (§4.3–§4.5): a
/// pump when the walker needs no source bytes, otherwise at most one read
/// into caller-owned bounded scratch.
pub fn poll_r2004_maps<S: ByteSource>(
    walker: &mut R2004MapWalker,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<MapPoll, PumpError<S::Error>> {
    let request = match walker.demand().map_err(PumpError::Grammar)? {
        MapDemand::Done => return Ok(MapPoll::Complete),
        MapDemand::Pump => return Ok(MapPoll::Pumped(walker.pump().map_err(PumpError::Grammar)?)),
        MapDemand::Read(request) => request,
    };
    let maximum = request.len.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    match source
        .poll_read(request, &mut scratch[..maximum])
        .map_err(PumpError::Source)?
    {
        SourcePoll::Pending => Ok(MapPoll::Pending),
        SourcePoll::Eof => {
            walker.end_of_input().map_err(PumpError::Grammar)?;
            Ok(MapPoll::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            let step = walker
                .feed(request.offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(MapPoll::Fed {
                step,
                offset: request.offset,
                len: actual,
            })
        }
    }
}

/// Result of one section-walk poll: a step from source bytes or from a
/// pump, the walker asking the caller for its next page, a pending source,
/// or the end of the section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionPoll {
    Fed {
        step: SectionStep,
        offset: u64,
        len: usize,
    },
    Pumped(SectionStep),
    /// Call `begin_page` with the next page of the section.
    NextPage,
    Pending,
    Complete,
}

/// One poll for a section's data-page walk (§4.6): a pump when the walker
/// needs no source bytes, otherwise at most one read into caller-owned
/// bounded scratch. `NextPage` consumes nothing.
pub fn poll_r2004_section<S: ByteSource>(
    walker: &mut R2004SectionWalker,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<SectionPoll, PumpError<S::Error>> {
    let request = match walker.demand().map_err(PumpError::Grammar)? {
        SectionDemand::Done => return Ok(SectionPoll::Complete),
        SectionDemand::NextPage => return Ok(SectionPoll::NextPage),
        SectionDemand::Pump => {
            return Ok(SectionPoll::Pumped(
                walker.pump().map_err(PumpError::Grammar)?,
            ));
        }
        SectionDemand::Read(request) => request,
    };
    let maximum = request.len.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    match source
        .poll_read(request, &mut scratch[..maximum])
        .map_err(PumpError::Source)?
    {
        SourcePoll::Pending => Ok(SectionPoll::Pending),
        SourcePoll::Eof => {
            walker.end_of_input().map_err(PumpError::Grammar)?;
            Ok(SectionPoll::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            let step = walker
                .feed(request.offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(SectionPoll::Fed {
                step,
                offset: request.offset,
                len: actual,
            })
        }
    }
}

/// Result of one whole-file walk poll: a step from source bytes or from a
/// pump, a pending source, or the end of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePoll {
    Fed {
        step: WalkStep,
        offset: u64,
        len: usize,
    },
    Pumped(WalkStep),
    Pending,
    Complete,
}

/// One poll for the whole R2004-family walk: a pump when the walker needs
/// no source bytes, otherwise at most one read into caller-owned bounded
/// scratch.
pub fn poll_r2004_walk<S: ByteSource>(
    walker: &mut R2004Walker,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<FilePoll, PumpError<S::Error>> {
    let request = match walker.demand().map_err(PumpError::Grammar)? {
        WalkDemand::Done => return Ok(FilePoll::Complete),
        WalkDemand::Pump => {
            return Ok(FilePoll::Pumped(walker.pump().map_err(PumpError::Grammar)?));
        }
        WalkDemand::Read(request) => request,
    };
    let maximum = request.len.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    match source
        .poll_read(request, &mut scratch[..maximum])
        .map_err(PumpError::Source)?
    {
        SourcePoll::Pending => Ok(FilePoll::Pending),
        SourcePoll::Eof => {
            walker.end_of_input().map_err(PumpError::Grammar)?;
            Ok(FilePoll::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            let step = walker
                .feed(request.offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(FilePoll::Fed {
                step,
                offset: request.offset,
                len: actual,
            })
        }
    }
}

/// Result of one whole-file R2007 walk poll: a step from source bytes (read
/// contiguously, or gathered across a page) or from a pump, a pending source,
/// or the end of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum R2007Poll {
    Fed {
        step: R2007Step,
        offset: u64,
        /// Bytes between the fed bytes in the source: 1 for a contiguous read.
        stride: u64,
        len: usize,
    },
    Pumped(R2007Step),
    Pending,
    Complete,
}

/// One poll for the whole R2007 walk: a pump when the walker needs no source
/// bytes, otherwise at most one read, or one gather of the codeword in
/// progress, into caller-owned bounded scratch.
pub fn poll_r2007_walk<S: ByteSource>(
    walker: &mut R2007Walker,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<R2007Poll, PumpError<S::Error>> {
    let (offset, stride, wanted) = match walker.demand().map_err(PumpError::Grammar)? {
        R2007Demand::Done => return Ok(R2007Poll::Complete),
        R2007Demand::Pump => {
            return Ok(R2007Poll::Pumped(walker.pump().map_err(PumpError::Grammar)?));
        }
        R2007Demand::Read(r) => (r.offset, 1, r.len),
        R2007Demand::Gather(g) => (g.offset, g.stride, g.len),
    };
    let maximum = wanted.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    let polled = if stride == 1 {
        source.poll_read(Request { offset, len: maximum }, &mut scratch[..maximum])
    } else {
        source.poll_gather(offset, stride, &mut scratch[..maximum])
    };
    match polled.map_err(PumpError::Source)? {
        SourcePoll::Pending => Ok(R2007Poll::Pending),
        SourcePoll::Eof => {
            walker.end_of_input().map_err(PumpError::Grammar)?;
            Ok(R2007Poll::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            let step = walker
                .feed(offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(R2007Poll::Fed {
                step,
                offset,
                stride,
                len: actual,
            })
        }
    }
}

/// One transport poll for the R13–R2000 container walk. Completion means
/// the object map's final chunk verified, not that bodies were decoded.
pub fn poll_walk<S: ByteSource>(
    walker: &mut FlatWalker,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<WalkPoll, PumpError<S::Error>> {
    Ok(match poll_once(walker, source, scratch)? {
        Polled::Fed(request, len, step) => WalkPoll::Fed {
            step,
            offset: request.offset,
            len,
        },
        Polled::Pending => WalkPoll::Pending,
        Polled::Complete => WalkPoll::Complete,
    })
}

/// Adapter for an existing seekable reader. It never probes EOF by seeking.
/// A resumed parser starts directly at its outstanding request's offset.
#[derive(Debug)]
pub struct FileSource<R> {
    inner: R,
    position: Option<u64>,
    window: Vec<u8>,
    window_at: u64,
}

/// Bytes a gathering [`FileSource`] reads at once and serves strided reads
/// from: a page's whole span (the R2007 walk gathers one byte of each codeword
/// from every row of it), not a byte at a time.
const GATHER_WINDOW: usize = 1 << 20;

impl<R> FileSource<R> {
    /// Take custody of the reader without opening or pre-reading it.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            position: None,
            window: Vec::new(),
            window_at: 0,
        }
    }

    /// Return custody to the caller.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> ByteSource for FileSource<R> {
    type Error = io::Error;
    fn poll_gather(
        &mut self,
        offset: u64,
        stride: u64,
        output: &mut [u8],
    ) -> io::Result<SourcePoll> {
        if output.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty gather request",
            ));
        }
        let span = (output.len() as u64 - 1).checked_mul(stride);
        let last = span.and_then(|span| offset.checked_add(span));
        let Some(last) = last.filter(|last| last - offset < GATHER_WINDOW as u64) else {
            return gather_by_reads(self, offset, stride, output);
        };
        let covered = |s: &Self| {
            offset >= s.window_at && last < s.window_at + s.window.len() as u64
        };
        if !covered(self) {
            self.window.resize(GATHER_WINDOW, 0);
            self.window_at = offset;
            let mut filled = 0;
            let seek = self.inner.seek(SeekFrom::Start(offset));
            let outcome = match seek {
                Ok(_) => loop {
                    match self.inner.read(&mut self.window[filled..]) {
                        Ok(0) => break Ok(()),
                        Ok(n) => {
                            filled += n;
                            if filled == GATHER_WINDOW {
                                break Ok(());
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => break Err(e),
                    }
                },
                Err(e) => Err(e),
            };
            self.window.truncate(filled);
            self.position = None;
            if let Err(e) = outcome {
                self.window.clear();
                return if e.kind() == io::ErrorKind::WouldBlock {
                    Ok(SourcePoll::Pending)
                } else {
                    Err(e)
                };
            }
        }
        if !covered(self) {
            return gather_by_reads(self, offset, stride, output);
        }
        for (k, slot) in output.iter_mut().enumerate() {
            *slot = self.window[(offset + k as u64 * stride - self.window_at) as usize];
        }
        Ok(SourcePoll::Ready(output.len()))
    }

    fn poll_read(&mut self, request: Request, output: &mut [u8]) -> io::Result<SourcePoll> {
        if self.position != Some(request.offset) {
            match self.inner.seek(SeekFrom::Start(request.offset)) {
                Ok(actual) if actual == request.offset => self.position = Some(actual),
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "seek returned wrong offset",
                    ));
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Ok(SourcePoll::Pending);
                }
                Err(e) => return Err(e),
            }
        }
        let maximum = request.len.min(output.len()).min(CHUNK_BYTES);
        if maximum == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty read request",
            ));
        }
        match self.inner.read(&mut output[..maximum]) {
            Ok(0) => Ok(SourcePoll::Eof),
            Ok(actual) => {
                self.position = request.offset.checked_add(actual as u64);
                Ok(SourcePoll::Ready(actual))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                Ok(SourcePoll::Pending)
            }
            Err(e) => Err(e),
        }
    }
}

/// Borrowed input adapter; owns no copy and allocates no source-sized buffer.
#[derive(Debug)]
pub struct SliceSource<'a> {
    bytes: &'a [u8],
}

impl<'a> SliceSource<'a> {
    /// Borrow a stable immutable source for caller-controlled polling.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl ByteSource for SliceSource<'_> {
    type Error = core::convert::Infallible;
    fn poll_gather(
        &mut self,
        offset: u64,
        stride: u64,
        output: &mut [u8],
    ) -> Result<SourcePoll, Self::Error> {
        let mut got = 0;
        while got < output.len() {
            let at = usize::try_from(offset + got as u64 * stride)
                .ok()
                .and_then(|at| self.bytes.get(at));
            let Some(&byte) = at else { break };
            output[got] = byte;
            got += 1;
        }
        Ok(if got == 0 {
            SourcePoll::Eof
        } else {
            SourcePoll::Ready(got)
        })
    }

    fn poll_read(
        &mut self,
        request: Request,
        output: &mut [u8],
    ) -> Result<SourcePoll, Self::Error> {
        let Some(input) = usize::try_from(request.offset)
            .ok()
            .and_then(|offset| self.bytes.get(offset..))
        else {
            return Ok(SourcePoll::Eof);
        };
        let n = request
            .len
            .min(output.len())
            .min(input.len())
            .min(CHUNK_BYTES);
        if n == 0 {
            return Ok(SourcePoll::Eof);
        }
        output[..n].copy_from_slice(&input[..n]);
        Ok(SourcePoll::Ready(n))
    }
}

/// Asynchronous adapter for a transport that answers later: a worker
/// thread, a browser worker, an HTTP range fetcher or an event loop. It
/// owns no thread, runtime or connection. A poll with nothing in flight
/// posts one bounded request and returns `Pending`; the host collects it
/// with [`AsyncSource::take_request`], fetches it however it likes and
/// hands back the answer with [`AsyncSource::complete`],
/// [`AsyncSource::complete_eof`] or [`AsyncSource::fail`], then polls
/// again. An answer must be for the posted offset and no longer than the
/// posted length, or it is rejected; bytes the caller's next scratch
/// cannot take are held (at most one chunk) for the polls that follow,
/// so no source byte is fetched twice.
#[derive(Debug)]
pub struct AsyncSource {
    posted: Option<Request>,
    taken: bool,
    held: [u8; CHUNK_BYTES],
    held_offset: u64,
    held_len: usize,
    eof_at: Option<u64>,
    failure: Option<io::Error>,
}

impl Default for AsyncSource {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncSource {
    /// An idle adapter with nothing posted.
    pub fn new() -> Self {
        Self {
            posted: None,
            taken: false,
            held: [0; CHUNK_BYTES],
            held_offset: 0,
            held_len: 0,
            eof_at: None,
            failure: None,
        }
    }

    /// The request posted by the last `Pending` poll, once, for the host
    /// to fetch. `None` if nothing is waiting to be sent.
    pub fn take_request(&mut self) -> Option<Request> {
        if self.taken {
            return None;
        }
        self.taken = self.posted.is_some();
        self.posted
    }

    /// Whether a posted request still awaits its answer.
    pub fn in_flight(&self) -> bool {
        self.posted.is_some()
    }

    /// Bytes fetched but not yet delivered, and the source offset of the
    /// first. They are the caller's to save with a grammar checkpoint, so a
    /// resumed process does not fetch them again.
    pub fn held(&self) -> (u64, &[u8]) {
        (self.held_offset, &self.held[..self.held_len])
    }

    /// An idle adapter that already holds `bytes` fetched at `offset`, as
    /// saved by [`AsyncSource::held`]. At most one chunk.
    pub fn with_held(offset: u64, bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > CHUNK_BYTES || offset.checked_add(bytes.len() as u64).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "held bytes exceed one chunk or the source range",
            ));
        }
        let mut source = Self::new();
        source.held[..bytes.len()].copy_from_slice(bytes);
        source.held_offset = offset;
        source.held_len = bytes.len();
        Ok(source)
    }

    fn answer(&mut self, offset: u64) -> io::Result<Request> {
        match self.posted {
            Some(posted) if self.taken && posted.offset == offset => Ok(posted),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "answer is not for the posted request",
            )),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no request is in flight",
            )),
        }
    }

    /// Deliver the bytes fetched for the posted request: at its offset,
    /// one byte up to its length. Anything else is rejected and leaves the
    /// request in flight.
    pub fn complete(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        let posted = self.answer(offset)?;
        if bytes.is_empty()
            || bytes.len() > posted.len
            || offset.checked_add(bytes.len() as u64).is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "answer is empty or longer than the posted request",
            ));
        }
        self.held[..bytes.len()].copy_from_slice(bytes);
        self.held_offset = offset;
        self.held_len = bytes.len();
        self.posted = None;
        self.taken = false;
        Ok(())
    }

    /// The source has no byte at the posted offset.
    pub fn complete_eof(&mut self, offset: u64) -> io::Result<()> {
        self.answer(offset)?;
        self.eof_at = Some(offset);
        self.posted = None;
        self.taken = false;
        Ok(())
    }

    /// The transport failed; the next poll returns this error.
    pub fn fail(&mut self, error: io::Error) {
        self.failure = Some(error);
        self.posted = None;
        self.taken = false;
    }
}

impl ByteSource for AsyncSource {
    type Error = io::Error;
    fn poll_read(&mut self, request: Request, output: &mut [u8]) -> io::Result<SourcePoll> {
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        let maximum = request.len.min(output.len()).min(CHUNK_BYTES);
        if maximum == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty read request",
            ));
        }
        if self.held_len > 0 {
            if request.offset != self.held_offset {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "request does not continue the bytes already fetched",
                ));
            }
            let n = maximum.min(self.held_len);
            output[..n].copy_from_slice(&self.held[..n]);
            self.held.copy_within(n..self.held_len, 0);
            self.held_len -= n;
            self.held_offset += n as u64;
            return Ok(SourcePoll::Ready(n));
        }
        if let Some(eof) = self.eof_at {
            return if request.offset == eof {
                Ok(SourcePoll::Eof)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "request after end of source",
                ))
            };
        }
        match self.posted {
            Some(posted) if posted.offset == request.offset => Ok(SourcePoll::Pending),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "request changed while a read is in flight",
            )),
            None => {
                self.posted = Some(Request {
                    offset: request.offset,
                    len: maximum,
                });
                self.taken = false;
                Ok(SourcePoll::Pending)
            }
        }
    }
}
