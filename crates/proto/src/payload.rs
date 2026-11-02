//! Fixed headers and payload validation. CRC can be updated as bytes arrive.
use crate::{ARTICLE_CAP, Error, MAX_TIME, Result, text};
#[derive(Debug, Clone, Copy)]
pub struct Crc32(u32);
impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}
impl Crc32 {
    pub const fn new() -> Self {
        Self(!0)
    }
    pub fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u32::from(b);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(self.0 & 1));
            }
        }
    }
    pub fn finish(self) -> u32 {
        !self.0
    }
}
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(bytes);
    c.finish()
}
fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenHeader {
    /// 1 (black/white) or 2 (four greys, ADR 018).
    pub bpp: u8,
    pub width: u16,
    pub height: u16,
    pub crc: u32,
}
impl TokenHeader {
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() < 16 {
            return Err(Error::Truncated);
        }
        if &b[..4] != b"X3TK" || b[4] != 1 || u16_at(b, 10) != 0 {
            return Err(Error::Invalid);
        }
        let h = Self {
            bpp: b[5],
            width: u16_at(b, 6),
            height: u16_at(b, 8),
            crc: u32_at(b, 12),
        };
        h.validate()?;
        Ok(h)
    }
    fn validate(&self) -> Result<()> {
        if !matches!(self.bpp, 1 | 2)
            || !matches!((self.width, self.height), (792, 528) | (528, 792))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        self.validate()?;
        let b = out.get_mut(..16).ok_or(Error::BufferTooSmall)?;
        b[..5].copy_from_slice(b"X3TK\x01");
        b[5] = self.bpp;
        b[6..8].copy_from_slice(&self.width.to_le_bytes());
        b[8..10].copy_from_slice(&self.height.to_le_bytes());
        b[10..12].fill(0);
        b[12..16].copy_from_slice(&self.crc.to_le_bytes());
        Ok(16)
    }
    /// Whole payload length: header plus 792 × 528 pixels at `bpp`.
    pub fn payload_len(&self) -> usize {
        16 + 792 * 528 * usize::from(self.bpp) / 8
    }
    pub fn payload(b: &[u8]) -> Result<Self> {
        let h = Self::decode(b)?;
        if b.len() != h.payload_len() {
            return Err(Error::Invalid);
        }
        if crc32(&b[16..]) != h.crc {
            return Err(Error::Checksum);
        }
        Ok(h)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArticleHeader {
    pub summary: bool,
    pub title_len: u16,
    pub url_len: u16,
    pub text_len: u32,
    pub fetched_at: u64,
    pub crc: u32,
}
impl ArticleHeader {
    pub fn len(&self) -> usize {
        28 + usize::from(self.title_len) + usize::from(self.url_len) + self.text_len as usize
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    fn validate(&self) -> Result<()> {
        if !(1..=256).contains(&self.title_len)
            || self.url_len > 2048
            || self.text_len > 262144
            || self.fetched_at > MAX_TIME
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub fn decode(b: &[u8]) -> Result<Self> {
        if b.len() < 28 {
            return Err(Error::Truncated);
        }
        if &b[..4] != b"X3AR" || b[4] != 1 || b[5] > 1 || u16_at(b, 6) != 0 {
            return Err(Error::Invalid);
        }
        let fetched_at = u64::from_le_bytes(b[16..24].try_into().map_err(|_| Error::Truncated)?);
        let h = Self {
            summary: b[5] == 1,
            title_len: u16_at(b, 8),
            url_len: u16_at(b, 10),
            text_len: u32_at(b, 12),
            fetched_at,
            crc: u32_at(b, 24),
        };
        h.validate()?;
        Ok(h)
    }
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        self.validate()?;
        let b = out.get_mut(..28).ok_or(Error::BufferTooSmall)?;
        b[..4].copy_from_slice(b"X3AR");
        b[4] = 1;
        b[5] = u8::from(self.summary);
        b[6..8].fill(0);
        b[8..10].copy_from_slice(&self.title_len.to_le_bytes());
        b[10..12].copy_from_slice(&self.url_len.to_le_bytes());
        b[12..16].copy_from_slice(&self.text_len.to_le_bytes());
        b[16..24].copy_from_slice(&self.fetched_at.to_le_bytes());
        b[24..28].copy_from_slice(&self.crc.to_le_bytes());
        Ok(28)
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct Article<'a> {
    pub header: ArticleHeader,
    pub title: &'a str,
    pub url: &'a str,
    pub text: &'a str,
}
pub fn valid_url(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty()
        && !authority.starts_with(':')
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}
impl<'a> Article<'a> {
    pub fn decode(b: &'a [u8]) -> Result<Self> {
        if b.len() > ARTICLE_CAP {
            return Err(Error::TooLarge);
        }
        let h = ArticleHeader::decode(b)?;
        if b.len() != h.len() {
            return Err(Error::Invalid);
        }
        if crc32(&b[28..]) != h.crc {
            return Err(Error::Checksum);
        }
        let title_end = 28 + usize::from(h.title_len);
        let url_end = title_end + usize::from(h.url_len);
        let title = text(&b[28..title_end])?;
        let url = text(&b[title_end..url_end])?;
        let content = text(&b[url_end..])?;
        validate_text(title, url, content)?;
        Ok(Self {
            header: h,
            title,
            url,
            text: content,
        })
    }
}
pub fn validate_text(title: &str, url: &str, content: &str) -> Result<()> {
    if title.is_empty() || title.len() > 256 || url.len() > 2048 || content.len() > 262144 {
        return Err(Error::TooLarge);
    }
    if title.chars().any(char::is_control)
        || url.chars().any(char::is_control)
        || !valid_url(url)
        || content
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
pub fn encode_article(
    title: &str,
    url: &str,
    content: &str,
    summary: bool,
    fetched_at: u64,
    out: &mut [u8],
) -> Result<usize> {
    validate_text(title, url, content)?;
    let mut crc = Crc32::new();
    for s in [title, url, content] {
        crc.update(s.as_bytes());
    }
    let h = ArticleHeader {
        summary,
        title_len: title.len() as u16,
        url_len: url.len() as u16,
        text_len: content.len() as u32,
        fetched_at,
        crc: crc.finish(),
    };
    if out.len() < h.len() {
        return Err(Error::BufferTooSmall);
    }
    h.encode(out)?;
    let mut n = 28;
    for s in [title, url, content] {
        out[n..n + s.len()].copy_from_slice(s.as_bytes());
        n += s.len();
    }
    Ok(n)
}
