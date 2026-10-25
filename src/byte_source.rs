//! Pausable byte delivery. Grammar declares ranges; only adapters perform I/O.

use crate::streaming::{CHUNK_BYTES, FlatHeaderParser, Request, StreamError};
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

/// Perform at most one transport poll, using caller-owned bounded scratch.
/// Completion refers to the file header, never to the whole model.
pub fn poll_header<S: ByteSource>(
    parser: &mut FlatHeaderParser,
    source: &mut S,
    scratch: &mut [u8],
) -> Result<HeaderPoll, PumpError<S::Error>> {
    let Some(request) = parser.request().map_err(PumpError::Grammar)? else {
        return Ok(HeaderPoll::Complete);
    };
    let maximum = request.len.min(scratch.len()).min(CHUNK_BYTES);
    if maximum == 0 {
        return Err(PumpError::InvalidRead { maximum, actual: 0 });
    }
    match source
        .poll_read(request, &mut scratch[..maximum])
        .map_err(PumpError::Source)?
    {
        SourcePoll::Pending => Ok(HeaderPoll::Pending),
        SourcePoll::Eof => {
            parser.end_of_input().map_err(PumpError::Grammar)?;
            Ok(HeaderPoll::Complete)
        }
        SourcePoll::Ready(actual) => {
            if actual == 0 || actual > maximum {
                return Err(PumpError::InvalidRead { maximum, actual });
            }
            parser
                .feed(request.offset, &scratch[..actual])
                .map_err(PumpError::Grammar)?;
            Ok(HeaderPoll::Progress(actual))
        }
    }
}

/// Adapter for an existing seekable reader. It never probes EOF by seeking.
/// A resumed parser starts directly at its outstanding request's offset.
#[derive(Debug)]
pub struct FileSource<R> {
    inner: R,
    position: Option<u64>,
}

impl<R> FileSource<R> {
    /// Take custody of the reader without opening or pre-reading it.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            position: None,
        }
    }

    /// Return custody to the caller.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> ByteSource for FileSource<R> {
    type Error = io::Error;
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
