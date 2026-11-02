//! Rasterise a TTF into an `x3f` bitmap font (format in `x3_core::font`).
use x3_core::font::{GLYPH_LEN, HEADER_LEN, MAGIC};

/// Latin with Polish, Latin-1, Greek, the punctuation web articles use and
/// common maths signs. Ascending order; glyphs the font lacks are skipped.
pub fn charset() -> impl Iterator<Item = char> {
    let ranges: [(u32, u32); 13] = [
        (0x20, 0x7E),     // ASCII
        (0xA0, 0xFF),     // Latin-1: NBSP, soft hyphen, accents
        (0x100, 0x17F),   // Latin Extended-A: Polish and Central European
        (0x370, 0x3FF),   // Greek: maths and science articles
        (0x2010, 0x2015), // hyphens and dashes
        (0x2018, 0x201F), // quotes, „
        (0x2020, 0x2026), // dagger, bullet, ellipsis
        (0x2032, 0x203A), // primes, ‹ ›
        (0x20AC, 0x20AC), // euro
        (0x2122, 0x2122), // trade mark
        (0x2190, 0x2193), // arrows
        (0x2200, 0x22FF), // maths operators: − √ ∞ ≈ ≠ ≤ ≥
        (0xFFFD, 0xFFFD), // replacement
    ];
    ranges
        .into_iter()
        .flat_map(|(a, b)| a..=b)
        .filter_map(char::from_u32)
}

/// Coverage at or above this (0–255) becomes ink.
pub const THRESHOLD: u8 = 110;

pub fn rasterise(ttf: &[u8], px: f32) -> Result<Vec<u8>, String> {
    if !(8.0..=96.0).contains(&px) {
        return Err("size must be 8 to 96 px".into());
    }
    let font = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
        .map_err(|e| e.to_string())?;
    let lm = font
        .horizontal_line_metrics(px)
        .ok_or("font has no horizontal metrics")?;
    let mut table = Vec::new();
    let mut bitmaps = Vec::new();
    let mut count: u16 = 0;
    for c in charset() {
        // Index 0 is .notdef: skip characters the font does not have.
        if font.lookup_glyph_index(c) == 0 && c != ' ' {
            continue;
        }
        let (m, coverage) = font.rasterize(c, px);
        let (w, h) = (m.width, m.height);
        if w > 255 || h > 255 {
            return Err(format!("glyph {c:?} too large"));
        }
        let top = m.ymin + h as i32;
        let advance = m.advance_width.round();
        if !(-128..=127).contains(&m.xmin) || !(-128..=127).contains(&top) || advance < 0.0 {
            return Err(format!("glyph {c:?} metrics out of range"));
        }
        let stride = w.div_ceil(8);
        let offset = bitmaps.len() as u32;
        for y in 0..h {
            let mut row = vec![0u8; stride];
            for x in 0..w {
                if coverage[y * w + x] >= THRESHOLD {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
            bitmaps.extend_from_slice(&row);
        }
        table.extend_from_slice(&(c as u32).to_le_bytes());
        table.extend_from_slice(&(advance as u16).to_le_bytes());
        table.push(m.xmin as i8 as u8);
        table.push(top as i8 as u8);
        table.push(w as u8);
        table.push(h as u8);
        table.extend_from_slice(&0u16.to_le_bytes());
        table.extend_from_slice(&offset.to_le_bytes());
        count += 1;
    }
    let ascent = lm.ascent.round() as i16;
    let descent = lm.descent.round() as i16;
    // 1.25× the em for comfortable reading on e-ink.
    let line_height = (px * 1.25).round() as u16;
    let mut out = Vec::with_capacity(HEADER_LEN + table.len() + bitmaps.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(px.round() as u16).to_le_bytes());
    out.extend_from_slice(&ascent.to_le_bytes());
    out.extend_from_slice(&descent.min(0).to_le_bytes());
    out.extend_from_slice(&line_height.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    debug_assert_eq!(table.len(), count as usize * GLYPH_LEN);
    out.extend_from_slice(&table);
    out.extend_from_slice(&bitmaps);
    x3_core::font::Font::parse(&out).map_err(|e| format!("self-check failed: {e:?}"))?;
    Ok(out)
}
