//! The R2007 walk's checkpoint (`DWGR`): the walk's cursors, the verified
//! 0x110-byte file header and the progress of each reader that is in use,
//! under a CRC-32, little endian, with no pointer or heap state. The
//! geometry of every page (where it is, how many codewords, what its checks
//! must be) is derived from the header, the page map's facts and the page
//! reference on restore and never stored, so a checkpoint cannot contradict
//! the file it is restored against. Restore replays the header's own checks
//! on the stored bytes, derives each reader's geometry, and requires the
//! progress to be a state the walk can reach; no source bytes are read.
//! CRC detects accidental corruption and gives no authenticity.

use super::r2007::{
    Cur, MapFacts, Pass, Pending, R2007_WINDOW, R2007Walker, Reader, Rec, Ref, Stage,
    check_plain_header,
};
use super::r2007_codes::{Crc64, Sum32};
use super::r2007_lz::{Mode, R2007Lz, WINDOW};
use super::{SourceIdentity, StreamError};

const VERSION: u8 = 1;
const TRAILER: usize = 4;
pub(super) const MAX: usize = 2 * R2007_WINDOW + 8192;
const HEAD: usize = 0x480;
const PLAIN: usize = 0x110;

/// Serialized R2007 walk: at most `2 * R2007_WINDOW + 8192` bytes, which is
/// each reader's retained window (the last `min(produced, window)` output
/// bytes) and a few kilobytes of cursors. Where the output went, and the
/// bytes of the last `Data` step, are the caller's to save alongside.
#[derive(Debug, Clone)]
pub struct R2007Checkpoint {
    bytes: [u8; MAX],
    len: usize,
}

impl R2007Checkpoint {
    /// The canonical wire bytes, ready for storage or cross-runtime transfer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct Put<'a> {
    b: &'a mut [u8],
    at: usize,
}

impl Put<'_> {
    fn bytes(&mut self, v: &[u8]) {
        self.b[self.at..self.at + v.len()].copy_from_slice(v);
        self.at += v.len();
    }
    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
}

struct Get<'a> {
    b: &'a [u8],
    at: usize,
}

type Res<T> = Result<T, StreamError>;

impl<'a> Get<'a> {
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(StreamError::InvalidCheckpoint)?;
        let s = self
            .b
            .get(self.at..end)
            .ok_or(StreamError::InvalidCheckpoint)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Res<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Res<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Res<u64> {
        let s = self.take(8)?;
        let mut b = [0; 8];
        b.copy_from_slice(s);
        Ok(u64::from_le_bytes(b))
    }
}

fn put_crc(w: &mut Put<'_>, c: &Crc64) {
    w.u64(c.crc);
    w.u8(c.held);
    w.bytes(&c.buf[..usize::from(c.held)]);
}

fn get_crc(r: &mut Get<'_>, mirrored: bool) -> Res<Crc64> {
    let crc = r.u64()?;
    let held = usize::from(r.u8()?);
    if held >= 8 {
        return Err(StreamError::InvalidCheckpoint);
    }
    let mut buf = [0; 8];
    buf[..held].copy_from_slice(r.take(held)?);
    let held = held as u8;
    Ok(Crc64 {
        crc,
        buf,
        held,
        mirrored,
    })
}

fn put_lz(w: &mut Put<'_>, z: &R2007Lz) {
    w.u8(z.mode as u8);
    w.u8(z.op);
    w.u8(z.skip);
    w.u8(z.lo);
    w.bytes(&z.ins);
    w.u8(z.ins_have);
    w.u8(z.ins_need);
    w.u64(z.count);
    w.u64(z.acc);
    w.u32(z.dist);
    w.bytes(&z.blk);
    w.u8(z.blk_have);
    w.u8(z.blk_need);
    w.bytes(&z.pend);
    w.u8(z.pend_len);
    w.u8(z.pend_pos);
    w.u64(z.produced);
    w.u64(z.consumed);
    let kept = z.produced.min(WINDOW as u64) as usize;
    let oldest = z.produced - kept as u64;
    for j in 0..kept {
        w.u8(z.window[((oldest + j as u64) as usize) & (WINDOW - 1)]);
    }
}

fn get_lz(r: &mut Get<'_>, z: &mut R2007Lz, limit: u64) -> Res<()> {
    z.mode = Mode::from_u8(r.u8()?).ok_or(StreamError::InvalidCheckpoint)?;
    z.op = r.u8()?;
    z.skip = r.u8()?;
    z.lo = r.u8()?;
    z.ins.copy_from_slice(r.take(4)?);
    z.ins_have = r.u8()?;
    z.ins_need = r.u8()?;
    z.count = r.u64()?;
    z.acc = r.u64()?;
    z.dist = r.u32()?;
    z.blk.copy_from_slice(r.take(32)?);
    z.blk_have = r.u8()?;
    z.blk_need = r.u8()?;
    z.pend.copy_from_slice(r.take(32)?);
    z.pend_len = r.u8()?;
    z.pend_pos = r.u8()?;
    z.produced = r.u64()?;
    z.consumed = r.u64()?;
    z.limit = limit;
    z.failure = None;
    let kept = z.produced.min(WINDOW as u64) as usize;
    let oldest = z.produced - kept as u64;
    let window = r.take(kept)?;
    for (j, &byte) in window.iter().enumerate() {
        z.window[((oldest + j as u64) as usize) & (WINDOW - 1)] = byte;
    }
    Ok(())
}

fn put_reader(w: &mut Put<'_>, r: &Reader) {
    w.u8(r.pass as u8);
    w.u64(r.cw);
    w.u64(r.src);
    w.u16(r.have as u16);
    w.u16(r.msg as u16);
    w.u16(r.take as u16);
    w.u64(r.fed);
    w.u64(r.produced);
    put_crc(w, &r.crc_s);
    put_crc(w, &r.crc_p);
    w.u32(r.sum.s1);
    w.u32(r.sum.s2);
    w.u16(r.sum.chunk);
    w.u8(r.sum.held);
    w.bytes(&r.sum.buf[..usize::from(r.sum.held)]);
    w.bytes(&r.buf[..r.have.max(r.msg)]);
    put_lz(w, &r.lz);
}

/// Read a reader's progress into `r`, whose geometry is already set.
fn get_reader(rd: &mut Get<'_>, r: &mut Reader) -> Res<()> {
    r.pass = Pass::from_u8(rd.u8()?).ok_or(StreamError::InvalidCheckpoint)?;
    r.cw = rd.u64()?;
    r.src = rd.u64()?;
    r.have = usize::from(rd.u16()?);
    r.msg = usize::from(rd.u16()?);
    r.take = usize::from(rd.u16()?);
    r.fed = rd.u64()?;
    r.produced = rd.u64()?;
    r.crc_s = get_crc(rd, true)?;
    r.crc_p = get_crc(rd, true)?;
    let (s1, s2, chunk) = (rd.u32()?, rd.u32()?, rd.u16()?);
    let held = usize::from(rd.u8()?);
    if held >= 8 {
        return Err(StreamError::InvalidCheckpoint);
    }
    let mut buf = [0; 8];
    buf[..held].copy_from_slice(rd.take(held)?);
    r.sum = Sum32 {
        s1,
        s2,
        buf,
        held: held as u8,
        chunk,
    };
    let n = r.have.max(r.msg);
    if n > r.buf.len() {
        return Err(StreamError::InvalidCheckpoint);
    }
    r.buf = [0; 256];
    r.buf[..n].copy_from_slice(rd.take(n)?);
    let limit = if r.geom.lz { r.geom.plain } else { 0 };
    get_lz(rd, &mut r.lz, limit)
}

fn put_pending(w: &mut Put<'_>, p: Pending) {
    match p {
        Pending::None => {
            w.u8(0);
            w.u64(0);
            w.u64(0);
        }
        Pending::Hole { offset, len } => {
            w.u8(1);
            w.u64(offset);
            w.u64(len);
        }
        Pending::SectionEnd { index, covered } => {
            w.u8(2);
            w.u64(u64::from(index));
            w.u64(covered);
        }
    }
}

fn get_pending(r: &mut Get<'_>) -> Res<Pending> {
    let tag = r.u8()?;
    let (a, b) = (r.u64()?, r.u64()?);
    match tag {
        0 if a == 0 && b == 0 => Ok(Pending::None),
        1 => Ok(Pending::Hole { offset: a, len: b }),
        2 => Ok(Pending::SectionEnd {
            index: u32::try_from(a).map_err(|_| StreamError::InvalidCheckpoint)?,
            covered: b,
        }),
        _ => Err(StreamError::InvalidCheckpoint),
    }
}

fn put_cur(w: &mut Put<'_>, c: &Cur) {
    w.u32(c.index);
    w.u8(c.raw_name);
    w.u8(c.name_len);
    w.bytes(&c.name);
    for v in [
        c.size,
        c.max,
        c.encryption,
        c.hash,
        c.encoding,
        c.pages,
        c.done,
        c.end,
    ] {
        w.u64(v);
    }
}

fn get_cur(r: &mut Get<'_>) -> Res<Cur> {
    let index = r.u32()?;
    let (raw_name, name_len) = (r.u8()?, r.u8()?);
    let mut name = [0; super::r2007::MAX_NAME];
    name.copy_from_slice(r.take(super::r2007::MAX_NAME)?);
    Ok(Cur {
        index,
        raw_name,
        name_len,
        name,
        size: r.u64()?,
        max: r.u64()?,
        encryption: r.u64()?,
        hash: r.u64()?,
        encoding: r.u64()?,
        pages: r.u64()?,
        done: r.u64()?,
        end: r.u64()?,
    })
}

impl R2007Walker {
    fn active(&self) -> (bool, bool) {
        match self.stage {
            Stage::Head | Stage::Done => (false, false),
            Stage::Map => (false, true),
            Stage::Sections => (true, false),
            Stage::Lookup | Stage::Page => (true, true),
        }
    }

    /// Serialize the walk between steps. A failed walk is terminal and has
    /// no checkpoint; its error is returned instead. The bytes of the last
    /// `Data` step, and anything derived from the output, are the caller's.
    pub fn checkpoint(&self) -> Result<R2007Checkpoint, StreamError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let mut out = R2007Checkpoint {
            bytes: [0; MAX],
            len: 0,
        };
        let mut w = Put {
            b: &mut out.bytes,
            at: 0,
        };
        w.bytes(b"DWGR");
        w.u8(VERSION);
        w.u8(self.stage as u8);
        w.u8(self.rec_kind as u8);
        w.u64(self.source.length);
        w.bytes(&self.source.digest);
        w.u32(self.described);
        w.u32(self.sections);
        w.u32(self.pages);
        w.u64(self.emitted);
        put_pending(&mut w, self.pending);
        w.u16(self.have as u16);
        w.u8(self.entry_have as u8);
        w.u8(self.rec_have as u8);
        let f = &self.facts;
        w.u64(f.entries);
        w.u64(f.max_id);
        w.u64(f.acc);
        w.u8(u8::from(f.self_seen) | (u8::from(f.sm_found) << 1));
        w.u64(f.sm_address);
        put_cur(&mut w, &self.cur);
        let r = &self.refd;
        for v in [
            r.data_offset,
            r.size,
            r.id as u64,
            r.uncompressed,
            r.compressed,
            r.checksum,
            r.crc,
            r.address,
            r.entry_size,
        ] {
            w.u64(v);
        }
        if self.stage == Stage::Head {
            w.bytes(&self.head[..self.have]);
        } else {
            for v in self.hdr {
                w.u64(v);
            }
        }
        w.bytes(&self.entry[..self.entry_have]);
        match self.rec_kind {
            Rec::Name => w.bytes(&self.name[..self.rec_have]),
            _ => w.bytes(&self.rec[..self.rec_have]),
        }
        let (sec, aux) = self.active();
        if sec {
            put_reader(&mut w, &self.sec);
        }
        if aux {
            put_reader(&mut w, &self.aux);
        }
        let end = w.at;
        let checksum = crate::crc::crc32(0, &out.bytes[..end]);
        out.bytes[end..end + TRAILER].copy_from_slice(&checksum.to_le_bytes());
        out.len = end + TRAILER;
        Ok(out)
    }

    /// Restore against an independently supplied source identity. The
    /// header is re-checked, each reader's geometry derived and its
    /// progress required to be reachable. No source bytes are read or
    /// requested.
    pub fn restore(input: &[u8], expected_source: SourceIdentity) -> Result<Self, StreamError> {
        let bad = StreamError::InvalidCheckpoint;
        if input.len() < 4 + TRAILER || input.len() > MAX || &input[..4] != b"DWGR" {
            return Err(bad);
        }
        let end = input.len() - TRAILER;
        if input[end..] != crate::crc::crc32(0, &input[..end]).to_le_bytes() {
            return Err(bad);
        }
        let mut rd = Get {
            b: &input[..end],
            at: 4,
        };
        if rd.u8()? != VERSION {
            return Err(bad);
        }
        let stage = Stage::from_u8(rd.u8()?).ok_or(bad)?;
        let rec_kind = Rec::from_u8(rd.u8()?).ok_or(bad)?;
        let length = rd.u64()?;
        let mut digest = [0; 32];
        digest.copy_from_slice(rd.take(32)?);
        let source = SourceIdentity { digest, length };
        if source != expected_source {
            return Err(StreamError::SourceMismatch);
        }
        let mut w = Self::new(source);
        w.stage = stage;
        w.rec_kind = rec_kind;
        w.described = rd.u32()?;
        w.sections = rd.u32()?;
        w.pages = rd.u32()?;
        w.emitted = rd.u64()?;
        w.pending = get_pending(&mut rd)?;
        w.have = usize::from(rd.u16()?);
        w.entry_have = usize::from(rd.u8()?);
        w.rec_have = usize::from(rd.u8()?);
        let facts = MapFacts {
            entries: rd.u64()?,
            max_id: rd.u64()?,
            acc: rd.u64()?,
            self_seen: false,
            sm_found: false,
            sm_address: 0,
        };
        let flags = rd.u8()?;
        if flags > 3 {
            return Err(bad);
        }
        w.facts = MapFacts {
            self_seen: flags & 1 != 0,
            sm_found: flags & 2 != 0,
            sm_address: rd.u64()?,
            ..facts
        };
        w.cur = get_cur(&mut rd)?;
        w.refd = Ref {
            data_offset: rd.u64()?,
            size: rd.u64()?,
            id: rd.u64()? as i64,
            uncompressed: rd.u64()?,
            compressed: rd.u64()?,
            checksum: rd.u64()?,
            crc: rd.u64()?,
            address: rd.u64()?,
            entry_size: rd.u64()?,
        };
        if stage == Stage::Head {
            if w.have >= HEAD {
                return Err(bad);
            }
            w.head[..w.have].copy_from_slice(rd.take(w.have)?);
        } else {
            let mut plain = [0u8; PLAIN];
            for chunk in plain.chunks_exact_mut(8) {
                chunk.copy_from_slice(&rd.u64()?.to_le_bytes());
            }
            w.hdr = check_plain_header(&plain, length).map_err(|_| bad)?;
        }
        let held = if rec_kind == Rec::Name {
            super::r2007::MAX_NAME
        } else {
            64
        };
        if w.entry_have > 16 || w.rec_have > held {
            return Err(bad);
        }
        w.entry[..w.entry_have].copy_from_slice(rd.take(w.entry_have)?);
        let rec = rd.take(w.rec_have)?;
        match rec_kind {
            Rec::Name => w.name[..w.rec_have].copy_from_slice(rec),
            _ => w.rec[..w.rec_have].copy_from_slice(rec),
        }
        let (sec, aux) = w.active();
        w.derive_geometry(stage).map_err(|_| bad)?;
        if sec {
            get_reader(&mut rd, &mut w.sec)?;
        }
        if aux {
            get_reader(&mut rd, &mut w.aux)?;
        }
        if rd.at != rd.b.len() || !w.consistent() {
            return Err(bad);
        }
        Ok(w)
    }
}
