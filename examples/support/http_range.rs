//! A plain-`http` range fetcher for the streaming examples: one worker
//! thread, one keep-alive HTTP/1.1 connection, one `Range` request per
//! posted [`Request`]. It exists to drive `byte_source::AsyncSource` with
//! a transport that really answers later; it is not a general HTTP client
//! (no TLS, redirects or chunked bodies).

use dwg::streaming::Request;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;

/// What the server said about one request. The offset is the one the
/// server's `Content-Range` claims; the adapter checks it.
#[derive(Debug)]
pub enum Answer {
    Bytes(u64, Vec<u8>),
    Eof(u64),
    Failed(io::Error),
}

pub struct HttpWorker {
    requests: mpsc::Sender<Request>,
    answers: mpsc::Receiver<Answer>,
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn get(conn: &mut BufReader<TcpStream>, host: &str, path: &str, request: Request) -> Answer {
    let last = request.offset + request.len as u64 - 1;
    let head = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nRange: bytes={}-{last}\r\n\r\n",
        request.offset
    );
    let result = (|| -> io::Result<Answer> {
        conn.get_mut().write_all(head.as_bytes())?;
        let mut line = String::new();
        conn.read_line(&mut line)?;
        let status: u16 = line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(format!("bad status line {line:?}")))?;
        let (mut length, mut range) = (None, None);
        loop {
            line.clear();
            if conn.read_line(&mut line)? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let header = line.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':').unwrap_or((header, ""));
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse::<usize>().ok(),
                "content-range" => range = Some(value.trim().to_string()),
                _ => (),
            }
        }
        let length = length.ok_or_else(|| invalid("no Content-Length".into()))?;
        // Decide from the status and length before reading or allocating a
        // body: a server that ignores Range (200, the whole file) or claims
        // more than was asked for is refused, not buffered.
        match status {
            416 => {
                if length > 1024 {
                    return Err(invalid(format!("416 with a {length}-byte body")));
                }
                io::copy(&mut conn.by_ref().take(length as u64), &mut io::sink())?;
                Ok(Answer::Eof(request.offset))
            }
            206 => {
                if length > request.len {
                    return Err(invalid(format!(
                        "206 body of {length} bytes for a {}-byte request",
                        request.len
                    )));
                }
                let range = range.ok_or_else(|| invalid("206 without Content-Range".into()))?;
                let first = range
                    .strip_prefix("bytes ")
                    .and_then(|r| r.split('-').next())
                    .and_then(|r| r.parse::<u64>().ok())
                    .ok_or_else(|| invalid(format!("bad Content-Range {range:?}")))?;
                let mut body = vec![0; length];
                conn.read_exact(&mut body)?;
                Ok(Answer::Bytes(first, body))
            }
            other => Err(invalid(format!("HTTP status {other}"))),
        }
    })();
    result.unwrap_or_else(Answer::Failed)
}

impl HttpWorker {
    /// Connect to `http://host:port/path` and start the worker thread.
    pub fn spawn(url: &str) -> io::Result<Self> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| invalid("only http:// URLs".into()))?;
        let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let (host, path) = (host.to_string(), path.to_string());
        let stream = TcpStream::connect(&host)?;
        // One small request per demand: without this, Nagle's algorithm and
        // delayed ACKs add tens of milliseconds to every round trip.
        stream.set_nodelay(true)?;
        let mut conn = BufReader::new(stream);
        let (requests, inbox) = mpsc::channel::<Request>();
        let (outbox, answers) = mpsc::channel();
        thread::spawn(move || {
            for request in inbox {
                if outbox.send(get(&mut conn, &host, &path, request)).is_err() {
                    break;
                }
            }
        });
        Ok(Self { requests, answers })
    }

    /// Post one request to the worker thread.
    pub fn fetch(&self, request: Request) -> io::Result<()> {
        self.requests
            .send(request)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker stopped"))
    }

    /// Block until the worker answers the oldest posted request.
    pub fn wait(&self) -> Answer {
        self.answers.recv().unwrap_or_else(|_| {
            Answer::Failed(io::Error::new(io::ErrorKind::BrokenPipe, "worker stopped"))
        })
    }
}
