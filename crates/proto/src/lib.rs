//! Bounded protocol v2 codecs shared by the device and host. No allocation.
#![no_std]
#![forbid(unsafe_code)]

pub mod config;
pub mod http;
pub mod lines;
pub mod payload;
pub mod sign;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    TooLarge,
    Truncated,
    BufferTooSmall,
    Checksum,
}
pub type Result<T> = core::result::Result<T, Error>;
pub const LINE_CAP: usize = 192;
pub const JOBS_CAP: usize = 4096;
/// Largest `tk1`: a four-grey token (ADR 018).
pub const TOKEN_CAP: usize = 104_560;
pub const ARTICLE_CAP: usize = 264_476;
pub const MAX_TIME: u64 = 253_402_300_799;

pub fn item_id(s: &str) -> Result<&str> {
    if s.is_empty()
        || s.len() > 48
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::Invalid);
    }
    Ok(s)
}
pub fn job_id(s: &str) -> Result<&str> {
    if s.len() != 26
        || s.as_bytes()[0] > b'7'
        || !s
            .bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
    {
        return Err(Error::Invalid);
    }
    Ok(s)
}
pub fn hash(s: &str) -> Result<&str> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid);
    }
    Ok(s)
}
pub(crate) fn number(s: &str) -> Result<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid);
    }
    s.parse().map_err(|_| Error::Invalid)
}
pub(crate) fn text(bytes: &[u8]) -> Result<&str> {
    core::str::from_utf8(bytes).map_err(|_| Error::Invalid)
}

/// A bounded formatting sink. Discard output on error (it may be partial).
pub(crate) struct Writer<'a> {
    pub bytes: &'a mut [u8],
    pub len: usize,
}
impl core::fmt::Write for Writer<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
        let dst = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
pub(crate) fn formatted(output: &mut [u8], args: core::fmt::Arguments<'_>) -> Result<usize> {
    use core::fmt::Write;
    let mut w = Writer {
        bytes: output,
        len: 0,
    };
    w.write_fmt(args).map_err(|_| Error::BufferTooSmall)?;
    Ok(w.len)
}
