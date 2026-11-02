//! `x3f` bitmap fonts: rasterised on the host (`x3-tools font`), read in place
//! on the device. Little-endian, no padding:
//!
//! ```text
//! header  16 B  "X3F1", em_px u16, ascent i16, descent i16 (≤ 0),
//!               line_height u16, glyph_count u16, reserved u16 = 0
//! glyphs  16 B each, codepoints strictly increasing:
//!               codepoint u32, advance u16, left i8, top i8 (rows above the
//!               baseline), width u8, height u8, reserved u16 = 0,
//!               bitmap offset u32 (from the start of the bitmap area)
//! bitmaps rows of ceil(width / 8) bytes, MSB first, 1 = ink
//! ```
use crate::Error;

pub const MAGIC: &[u8; 4] = b"X3F1";
pub const HEADER_LEN: usize = 16;
pub const GLYPH_LEN: usize = 16;
/// Largest font file accepted.
pub const MAX_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph<'a> {
    pub codepoint: u32,
    pub advance: u16,
    pub left: i8,
    pub top: i8,
    pub width: u8,
    pub height: u8,
    /// `height` rows of `stride()` bytes.
    pub bitmap: &'a [u8],
}

impl Glyph<'_> {
    pub fn stride(&self) -> usize {
        (self.width as usize).div_ceil(8)
    }

    pub fn ink(&self, x: usize, y: usize) -> bool {
        x < self.width as usize
            && y < self.height as usize
            && self.bitmap[y * self.stride() + x / 8] & (0x80 >> (x % 8)) != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Font<'a> {
    pub em_px: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_height: u16,
    table: &'a [u8],
    bitmaps: &'a [u8],
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl<'a> Font<'a> {
    /// Parses and fully validates a font, so later lookups cannot fail.
    pub fn parse(b: &'a [u8]) -> Result<Self, Error> {
        if b.len() > MAX_LEN {
            return Err(Error::TooLarge);
        }
        if b.len() < HEADER_LEN || &b[0..4] != MAGIC {
            return Err(Error::Invalid);
        }
        let em_px = u16_at(b, 4);
        let ascent = u16_at(b, 6) as i16;
        let descent = u16_at(b, 8) as i16;
        let line_height = u16_at(b, 10);
        let count = u16_at(b, 12) as usize;
        if u16_at(b, 14) != 0
            || em_px == 0
            || em_px > 200
            || ascent <= 0
            || descent > 0
            || line_height == 0
            || line_height > 400
            || count == 0
        {
            return Err(Error::Invalid);
        }
        let table_end = HEADER_LEN + count * GLYPH_LEN;
        if b.len() < table_end {
            return Err(Error::Corrupt);
        }
        let table = &b[HEADER_LEN..table_end];
        let bitmaps = &b[table_end..];
        let mut previous: Option<u32> = None;
        for g in table.as_chunks::<GLYPH_LEN>().0 {
            let cp = u32_at(g, 0);
            if char::from_u32(cp).is_none() || previous.is_some_and(|p| cp <= p) {
                return Err(Error::Corrupt);
            }
            previous = Some(cp);
            let size = (g[8] as usize).div_ceil(8) * g[9] as usize;
            let offset = u32_at(g, 12) as usize;
            if u16_at(g, 4) > 400
                || u16_at(g, 10) != 0
                || offset
                    .checked_add(size)
                    .is_none_or(|end| end > bitmaps.len())
            {
                return Err(Error::Corrupt);
            }
        }
        Ok(Self {
            em_px,
            ascent,
            descent,
            line_height,
            table,
            bitmaps,
        })
    }

    pub fn len(&self) -> usize {
        self.table.len() / GLYPH_LEN
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    fn at(&self, i: usize) -> Glyph<'a> {
        let g = &self.table[i * GLYPH_LEN..(i + 1) * GLYPH_LEN];
        let (width, height) = (g[8], g[9]);
        let offset = u32_at(g, 12) as usize;
        let size = (width as usize).div_ceil(8) * height as usize;
        Glyph {
            codepoint: u32_at(g, 0),
            advance: u16_at(g, 4),
            left: g[6] as i8,
            top: g[7] as i8,
            width,
            height,
            bitmap: &self.bitmaps[offset..offset + size],
        }
    }

    /// The glyph for `c`, if the font has it.
    pub fn glyph(&self, c: char) -> Option<Glyph<'a>> {
        let (mut lo, mut hi) = (0, self.len());
        let want = c as u32;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let cp = u32_at(&self.table[mid * GLYPH_LEN..], 0);
            match cp.cmp(&want) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(self.at(mid)),
            }
        }
        None
    }

    /// The glyph drawn for `c`: itself, else U+FFFD, else '?', else the first.
    pub fn glyph_or_fallback(&self, c: char) -> Glyph<'a> {
        self.glyph(c)
            .or_else(|| self.glyph('\u{FFFD}'))
            .or_else(|| self.glyph('?'))
            .unwrap_or_else(|| self.at(0))
    }

    pub fn advance(&self, c: char) -> u16 {
        self.glyph_or_fallback(c).advance
    }
}
