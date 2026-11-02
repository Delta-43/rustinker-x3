//! Bounded cache metadata. Payloads are immutable until atomic replacement.
use crate::Error;
pub const MAX_ID: usize = 48;
pub const MAX_ENTRIES: usize = 500;
const RECORD: usize = 48 + 1 + 8 + 4 + 4 + 1;

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
/// Returns a safe ASCII name, substituting unsupported characters. Storage APIs
/// still reject unvalidated external IDs rather than silently rewriting them.
pub fn sanitise_filename(input: &str, output: &mut [u8]) -> Result<usize, Error> {
    if input.is_empty()
        || input.len() > 256
        || input.contains(['/', '\\'])
        || input == "."
        || input == ".."
    {
        return Err(Error::Invalid);
    }
    let mut n = 0;
    for c in input.chars().take(MAX_ID) {
        if n >= output.len() {
            return Err(Error::BufferTooSmall);
        }
        output[n] = if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            c as u8
        } else {
            b'_'
        };
        n += 1;
    }
    if n == 0 {
        return Err(Error::Invalid);
    }
    Ok(n)
}

/// `rename` must atomically replace a file. `flush` must persist the temporary
/// data before rename. Hardware FAT guarantees require power-loss testing.
pub trait Storage {
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), Error>;
    fn flush(&mut self, name: &str) -> Result<(), Error>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), Error>;
    fn read(&self, name: &str, output: &mut [u8]) -> Result<usize, Error>;
    fn remove(&mut self, name: &str) -> Result<(), Error>;
}
pub fn atomic_write(storage: &mut impl Storage, id: &str, bytes: &[u8]) -> Result<(), Error> {
    if !valid_id(id) {
        return Err(Error::Invalid);
    }
    if bytes.len() > 264_476 {
        return Err(Error::TooLarge);
    }
    let mut name = [0u8; MAX_ID + 4];
    name[..id.len()].copy_from_slice(id.as_bytes());
    name[id.len()..id.len() + 4].copy_from_slice(b".tmp");
    let temp = core::str::from_utf8(&name[..id.len() + 4]).map_err(|_| Error::Invalid)?;
    storage.write(temp, bytes)?;
    storage.flush(temp)?;
    storage.rename(temp, id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    id: [u8; MAX_ID],
    len: u8,
    pub created: u64,
    pub size: u32,
    pub position: u32,
    pub read: bool,
}
impl Entry {
    pub fn new(id: &str, created: u64, size: u32) -> Result<Self, Error> {
        if !valid_id(id) || size > 264_476 {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; MAX_ID];
        bytes[..id.len()].copy_from_slice(id.as_bytes());
        Ok(Self {
            id: bytes,
            len: id.len() as u8,
            created,
            size,
            position: 0,
            read: false,
        })
    }
    pub fn id(&self) -> &str {
        core::str::from_utf8(&self.id[..self.len as usize]).unwrap_or("")
    }
    pub fn set_position(&mut self, text: &str, position: usize) -> Result<(), Error> {
        if text.len() > 262_144 || position > text.len() || !text.is_char_boundary(position) {
            return Err(Error::Invalid);
        }
        self.position = position as u32;
        Ok(())
    }
}
/// Caller owns the memory. A 500-entry index belongs in SD-backed working
/// storage on device; a small active window avoids a large firmware stack.
pub struct Index<const N: usize> {
    entries: [Option<Entry>; N],
    cap: u64,
}
impl<const N: usize> Index<N> {
    pub fn new(cap: u64) -> Result<Self, Error> {
        if N == 0 || N > MAX_ENTRIES || cap == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            entries: [None; N],
            cap,
        })
    }
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().flatten()
    }
    pub fn used(&self) -> u64 {
        self.entries().map(|e| u64::from(e.size)).sum()
    }
    /// Plans and applies evictions. Caller persists the index atomically before
    /// deleting orphan payloads. Payload failures must not mutate this index.
    pub fn insert(&mut self, entry: Entry, mut evicted: impl FnMut(Entry)) -> Result<(), Error> {
        if u64::from(entry.size) > self.cap || entry.position > 262_144 {
            return Err(Error::TooLarge);
        }
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|s| s.is_some_and(|e| e.id() == entry.id()))
        {
            *slot = None;
        }
        while self.used() + u64::from(entry.size) > self.cap
            || self.entries.iter().all(Option::is_some)
        {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.map(|e| (i, !e.read, e.created)))
                .min_by_key(|&(i, unread, date)| (unread, date, i))
                .map(|v| v.0)
                .ok_or(Error::Invalid)?;
            if let Some(e) = self.entries[victim].take() {
                evicted(e);
            }
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Error::Invalid)?;
        *slot = Some(entry);
        Ok(())
    }
    pub fn remove(&mut self, id: &str) -> Result<Entry, Error> {
        if !valid_id(id) {
            return Err(Error::Invalid);
        }
        self.entries
            .iter_mut()
            .find(|s| s.is_some_and(|e| e.id() == id))
            .and_then(Option::take)
            .ok_or(Error::Invalid)
    }
    pub fn encode(&self, output: &mut [u8]) -> Result<usize, Error> {
        let count = self.entries().count();
        let len = 6 + count * RECORD + 4;
        if output.len() < len {
            return Err(Error::BufferTooSmall);
        }
        output[..4].copy_from_slice(b"X3I1");
        output[4..6].copy_from_slice(&(count as u16).to_le_bytes());
        for (i, e) in self.entries().enumerate() {
            let r = &mut output[6 + i * RECORD..6 + (i + 1) * RECORD];
            r[..48].copy_from_slice(&e.id);
            r[48] = e.len;
            r[49..57].copy_from_slice(&e.created.to_le_bytes());
            r[57..61].copy_from_slice(&e.size.to_le_bytes());
            r[61..65].copy_from_slice(&e.position.to_le_bytes());
            r[65] = u8::from(e.read);
        }
        let crc = crc32(&output[..len - 4]);
        output[len - 4..len].copy_from_slice(&crc.to_le_bytes());
        Ok(len)
    }
    /// Corrupt snapshots are rejected wholesale; caller may rebuild from fully
    /// validated payloads. Never treats partially decoded entries as committed.
    /// Reopens a damaged snapshot as an empty index. Orphan payloads are left
    /// untouched for later validation; no damaged entry is made visible.
    /// The boolean reports whether recovery discarded corrupt metadata.
    pub fn recover(bytes: &[u8], cap: u64) -> Result<(Self, bool), Error> {
        let empty = Self::new(cap)?;
        match Self::decode(bytes, cap) {
            Ok(index) => Ok((index, false)),
            Err(_) => Ok((empty, true)),
        }
    }
    pub fn decode(bytes: &[u8], cap: u64) -> Result<Self, Error> {
        let mut index = Self::new(cap)?;
        if bytes.len() < 10 || &bytes[..4] != b"X3I1" {
            return Err(Error::Corrupt);
        }
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        if count > N || bytes.len() != 6 + count * RECORD + 4 {
            return Err(Error::Corrupt);
        }
        let split = bytes.len() - 4;
        if crc32(&bytes[..split]).to_le_bytes() != bytes[split..] {
            return Err(Error::Corrupt);
        }
        for r in bytes[6..split].as_chunks::<RECORD>().0 {
            let len = r[48] as usize;
            if len == 0 || len > 48 || r[len..48].iter().any(|&b| b != 0) || r[65] > 1 {
                return Err(Error::Corrupt);
            }
            let id = core::str::from_utf8(&r[..len]).map_err(|_| Error::Corrupt)?;
            let date = u64::from_le_bytes(r[49..57].try_into().map_err(|_| Error::Corrupt)?);
            let size = u32::from_le_bytes(r[57..61].try_into().map_err(|_| Error::Corrupt)?);
            let mut e = Entry::new(id, date, size).map_err(|_| Error::Corrupt)?;
            e.position = u32::from_le_bytes(r[61..65].try_into().map_err(|_| Error::Corrupt)?);
            e.read = r[65] == 1;
            if e.position > 262_144
                || index.entries().any(|old| old.id() == id)
                || index.used() + u64::from(size) > cap
            {
                return Err(Error::Corrupt);
            }
            index.insert(e, |_| {}).map_err(|_| Error::Corrupt)?;
        }
        Ok(index)
    }
}
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
