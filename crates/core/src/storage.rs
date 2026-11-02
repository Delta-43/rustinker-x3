//! Immutable payloads and metadata generations. No rename atomicity assumption.
//! The adapter must synchronise writes before `sync` returns. `create` is
//! exclusive: a failed generation must never overwrite an older file.
use crate::Error;
use sha2::{Digest, Sha256};
use x3_proto::{
    self as proto,
    payload::{ArticleHeader, Crc32, TokenHeader},
};
pub const RECORD_LEN: usize = 112;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Payload,
    Metadata,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileKey {
    pub generation: u32,
    pub kind: FileKind,
}
impl FileKey {
    pub fn name(&self) -> [u8; 12] {
        let mut n = *b"00000000.DAT";
        for i in 0..8 {
            n[7 - i] = b"0123456789ABCDEF"[((self.generation >> (i * 4)) & 15) as usize];
        }
        if self.kind == FileKind::Metadata {
            n[9..].copy_from_slice(b"MET");
        }
        n
    }
}
pub trait Store {
    fn create(&mut self, key: FileKey) -> Result<(), Error>;
    fn append(&mut self, key: FileKey, bytes: &[u8]) -> Result<(), Error>;
    fn sync(&mut self, key: FileKey) -> Result<(), Error>;
    fn len(&mut self, key: FileKey) -> Result<usize, Error>;
    fn read(&mut self, key: FileKey, offset: usize, out: &mut [u8]) -> Result<usize, Error>;
}
/// A store that can list its own generations.
pub trait Catalog: Store {
    /// The next unused generation, including abandoned payload files.
    fn next_generation(&mut self) -> Result<u32, Error>;
    /// Metadata generations above `after`, ascending, into caller-owned
    /// scratch. Repeat with the last one returned to keep RAM bounded.
    fn generations(&mut self, after: u32, out: &mut [u32]) -> Result<usize, Error>;
    /// Payload generations above `after`, ascending, like `generations`.
    fn payload_generations(&mut self, after: u32, out: &mut [u32]) -> Result<usize, Error>;
    /// Deletes one file. A file that is already gone is not an error.
    fn remove(&mut self, key: FileKey) -> Result<(), Error>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    item: [u8; 48],
    item_len: u8,
    pub generation: u32,
    pub payload_generation: u32,
    pub len: u32,
    pub sha256: [u8; 32],
    pub position: u32,
    pub token: bool,
    pub deleted: bool,
}
impl Record {
    pub fn new(
        item: &str,
        generation: u32,
        len: u32,
        sha256: [u8; 32],
        token: bool,
    ) -> Result<Self, Error> {
        proto::item_id(item).map_err(|_| Error::Invalid)?;
        if generation == 0
            || len as usize
                > if token {
                    proto::TOKEN_CAP
                } else {
                    proto::ARTICLE_CAP
                }
        {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; 48];
        bytes[..item.len()].copy_from_slice(item.as_bytes());
        Ok(Self {
            item: bytes,
            item_len: item.len() as u8,
            generation,
            payload_generation: generation,
            len,
            sha256,
            position: 0,
            token,
            deleted: false,
        })
    }
    pub fn item(&self) -> &str {
        core::str::from_utf8(&self.item[..self.item_len as usize]).unwrap_or("")
    }
    pub fn payload_key(&self) -> FileKey {
        FileKey {
            generation: self.payload_generation,
            kind: FileKind::Payload,
        }
    }
    pub fn metadata_key(&self) -> FileKey {
        FileKey {
            generation: self.generation,
            kind: FileKind::Metadata,
        }
    }
    pub fn encode(&self) -> Result<[u8; RECORD_LEN], Error> {
        if self.generation == 0
            || self.payload_generation == 0
            || self.payload_generation > self.generation
            || self.position > 262144
        {
            return Err(Error::Invalid);
        }
        let mut b = [0; RECORD_LEN];
        b[..4].copy_from_slice(b"X3M1");
        b[4..8].copy_from_slice(&self.generation.to_le_bytes());
        b[8..12].copy_from_slice(&self.payload_generation.to_le_bytes());
        b[12..16].copy_from_slice(&self.len.to_le_bytes());
        b[16..20].copy_from_slice(&self.position.to_le_bytes());
        b[20] = self.item_len;
        b[21] = u8::from(self.token);
        b[22] = u8::from(self.deleted);
        b[24..72].copy_from_slice(&self.item);
        b[72..104].copy_from_slice(&self.sha256);
        let crc = proto::payload::crc32(&b[..108]);
        b[108..].copy_from_slice(&crc.to_le_bytes());
        Ok(b)
    }
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != RECORD_LEN
            || &b[..4] != b"X3M1"
            || b[21] > 1
            || b[22] > 1
            || b[23] != 0
            || b[104..108] != [0; 4]
            || proto::payload::crc32(&b[..108]).to_le_bytes() != b[108..]
        {
            return Err(Error::Corrupt);
        }
        let il = usize::from(b[20]);
        if il == 0 || il > 48 || b[24 + il..72].iter().any(|&v| v != 0) {
            return Err(Error::Corrupt);
        }
        let u = |i| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut r = Self::new(
            core::str::from_utf8(&b[24..24 + il]).map_err(|_| Error::Corrupt)?,
            u(4),
            u(12),
            b[72..104].try_into().map_err(|_| Error::Corrupt)?,
            b[21] == 1,
        )?;
        r.payload_generation = u(8);
        r.position = u(16);
        r.deleted = b[22] == 1;
        r.encode()?;
        Ok(r)
    }
}
fn read_exact(
    s: &mut impl Store,
    key: FileKey,
    offset: usize,
    out: &mut [u8],
) -> Result<(), Error> {
    let mut n = 0;
    while n < out.len() {
        let got = s.read(key, offset + n, &mut out[n..])?;
        if got == 0 || got > out.len() - n {
            return Err(Error::Corrupt);
        }
        n += got;
    }
    Ok(())
}
/// UTF-8 and control validation across arbitrary chunk boundaries.
struct Utf8 {
    bytes: [u8; 4],
    used: usize,
    need: usize,
}
impl Utf8 {
    fn new() -> Self {
        Self {
            bytes: [0; 4],
            used: 0,
            need: 0,
        }
    }
    fn push(&mut self, b: u8) -> Result<(), Error> {
        if self.used == 0 {
            self.need = match b {
                0..=127 => 1,
                194..=223 => 2,
                224..=239 => 3,
                240..=244 => 4,
                _ => return Err(Error::Corrupt),
            };
        }
        self.bytes[self.used] = b;
        self.used += 1;
        if self.used == self.need {
            let s = core::str::from_utf8(&self.bytes[..self.used]).map_err(|_| Error::Corrupt)?;
            if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err(Error::Corrupt);
            }
            self.used = 0;
        }
        Ok(())
    }
}
/// Reread the persisted payload before exposing metadata. Scratch is 512 bytes;
/// URL validation uses a bounded 2 KiB stack buffer, never a full article copy.
pub fn verify(s: &mut impl Store, r: &Record) -> Result<(), Error> {
    let key = r.payload_key();
    if s.len(key)? != r.len as usize {
        return Err(Error::Corrupt);
    }
    let mut header = [0; 28];
    let header_len = if r.token { 16 } else { 28 };
    read_exact(s, key, 0, &mut header[..header_len])?;
    let (expected_crc, text_start) = if r.token {
        let h = TokenHeader::decode(&header).map_err(|_| Error::Corrupt)?;
        if r.len as usize != h.payload_len() {
            return Err(Error::Corrupt);
        }
        (h.crc, usize::MAX)
    } else {
        let h = ArticleHeader::decode(&header).map_err(|_| Error::Corrupt)?;
        if h.len() != r.len as usize || r.position > h.text_len {
            return Err(Error::Corrupt);
        }
        let mut title = [0; 256];
        read_exact(s, key, 28, &mut title[..h.title_len as usize])?;
        let title =
            core::str::from_utf8(&title[..h.title_len as usize]).map_err(|_| Error::Corrupt)?;
        let mut url = [0; 2048];
        read_exact(
            s,
            key,
            28 + h.title_len as usize,
            &mut url[..h.url_len as usize],
        )?;
        let url = core::str::from_utf8(&url[..h.url_len as usize]).map_err(|_| Error::Corrupt)?;
        proto::payload::validate_text(title, url, "").map_err(|_| Error::Corrupt)?;
        (h.crc, 28 + h.title_len as usize + h.url_len as usize)
    };
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    let mut utf = Utf8::new();
    let mut buf = [0; 512];
    let mut offset = 0;
    while offset < r.len as usize {
        let n = buf.len().min(r.len as usize - offset);
        read_exact(s, key, offset, &mut buf[..n])?;
        sha.update(&buf[..n]);
        for (i, &b) in buf[..n].iter().enumerate() {
            let at = offset + i;
            if at >= header_len {
                crc.update(&[b]);
            }
            if at >= text_start {
                if at - text_start == r.position as usize && utf.used != 0 {
                    return Err(Error::Corrupt);
                }
                utf.push(b)?;
            }
        }
        offset += n;
    }
    let digest: [u8; 32] = sha.finalize().into();
    if digest != r.sha256 || crc.finish() != expected_crc || utf.used != 0 {
        return Err(Error::Corrupt);
    }
    Ok(())
}
/// True when an article's saved position is at (or past) the end of its text.
/// Tokens are never "finished". Reads only the 28-byte header.
pub fn finished(s: &mut impl Store, r: &Record) -> Result<bool, Error> {
    if r.token || r.deleted {
        return Ok(false);
    }
    let mut header = [0; 28];
    read_exact(s, r.payload_key(), 0, &mut header)?;
    let h = ArticleHeader::decode(&header).map_err(|_| Error::Corrupt)?;
    Ok(r.position >= h.text_len)
}
/// A download owns no payload buffer. Any error requires abandoning this
/// generation; it can be garbage-collected only after recovery.
pub struct Stage {
    record: Record,
    written: usize,
    sha: Sha256,
    failed: bool,
}
impl Stage {
    pub fn begin(s: &mut impl Store, record: Record) -> Result<Self, Error> {
        if record.deleted || record.position != 0 || record.generation != record.payload_generation
        {
            return Err(Error::Invalid);
        }
        record.encode()?;
        s.create(record.payload_key())?;
        Ok(Self {
            record,
            written: 0,
            sha: Sha256::new(),
            failed: false,
        })
    }
    pub fn write(&mut self, s: &mut impl Store, bytes: &[u8]) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Storage);
        }
        if bytes.len() > self.record.len as usize - self.written {
            self.failed = true;
            return Err(Error::TooLarge);
        }
        if let Err(e) = s.append(self.record.payload_key(), bytes) {
            self.failed = true;
            return Err(e);
        }
        self.sha.update(bytes);
        self.written += bytes.len();
        Ok(())
    }
    pub fn commit(self, s: &mut impl Store) -> Result<Record, Error> {
        let hash: [u8; 32] = self.sha.finalize().into();
        if self.failed || self.written != self.record.len as usize || hash != self.record.sha256 {
            return Err(Error::Corrupt);
        }
        s.sync(self.record.payload_key())?;
        verify(s, &self.record)?;
        persist(s, &self.record)?;
        Ok(self.record)
    }
}
fn persist(s: &mut impl Store, r: &Record) -> Result<(), Error> {
    let bytes = r.encode()?;
    let key = r.metadata_key();
    s.create(key)?;
    s.append(key, &bytes)?;
    s.sync(key)?;
    let mut check = [0; RECORD_LEN];
    read_exact(s, key, 0, &mut check)?;
    if Record::decode(&check)? != *r {
        return Err(Error::Corrupt);
    }
    Ok(())
}
/// Rebuild one logical item's latest state by scanning the adapter's metadata
/// filenames. A tombstone prevents resurrection of an older generation.
/// Missing/corrupt candidates are ignored; I/O errors propagate from the scan
/// adapter itself. Callers must not garbage-collect history until recovery ends.
pub fn recover(
    s: &mut impl Store,
    item: &str,
    generations: impl IntoIterator<Item = u32>,
) -> Result<Option<Record>, Error> {
    proto::item_id(item).map_err(|_| Error::Invalid)?;
    let mut latest: Option<Record> = None;
    for generation in generations {
        let key = FileKey {
            generation,
            kind: FileKind::Metadata,
        };
        let mut bytes = [0; RECORD_LEN];
        if s.len(key) != Ok(RECORD_LEN) || read_exact(s, key, 0, &mut bytes).is_err() {
            continue;
        }
        let Ok(r) = Record::decode(&bytes) else {
            continue;
        };
        if r.generation != generation
            || r.item() != item
            || latest
                .as_ref()
                .is_some_and(|old| old.generation >= generation)
        {
            continue;
        }
        if r.deleted || verify(s, &r).is_ok() {
            latest = Some(r);
        }
    }
    Ok(latest)
}
pub fn position(
    s: &mut impl Store,
    old: &Record,
    generation: u32,
    offset: u32,
) -> Result<Record, Error> {
    if generation <= old.generation || old.deleted || old.token {
        return Err(Error::Invalid);
    }
    let mut new = old.clone();
    new.generation = generation;
    new.position = offset;
    verify(s, &new)?;
    persist(s, &new)?;
    Ok(new)
}
pub fn delete(s: &mut impl Store, old: &Record, generation: u32) -> Result<Record, Error> {
    if generation <= old.generation {
        return Err(Error::Invalid);
    }
    let mut new = old.clone();
    new.generation = generation;
    new.deleted = true;
    persist(s, &new)?;
    Ok(new)
}
/// Select oldest committed token, excluding the item currently on screen.
/// Caller passes only latest non-deleted records, one per logical item.
pub fn token_victim<'a>(
    records: &'a [Record],
    on_screen: &str,
    limit: usize,
) -> Result<Option<&'a Record>, Error> {
    if limit == 0 {
        return Err(Error::Invalid);
    }
    let count = records.iter().filter(|r| r.token && !r.deleted).count();
    if count <= limit {
        return Ok(None);
    }
    records
        .iter()
        .filter(|r| r.token && !r.deleted && r.item() != on_screen)
        .min_by_key(|r| r.payload_generation)
        .map(Some)
        .ok_or(Error::Storage)
}
