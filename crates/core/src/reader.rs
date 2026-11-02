//! Proportional page layout and rendering for articles, portrait 528×792.
//!
//! Lines break at spaces and soft hyphens (U+00AD); the companion inserts
//! soft hyphens, so the device carries no hyphenation dictionaries. Lines are
//! justified except the last of a paragraph and headings ("# " at the start
//! of a paragraph). A no-break space (U+00A0) is drawn as a space but never
//! breaks or stretches. Positions are UTF-8 byte offsets, so a saved place
//! survives re-layout.
use crate::{
    Error,
    font::{Font, Glyph},
    panel,
};

pub const WIDTH: usize = 528;
pub const HEIGHT: usize = 792;
pub const STRIDE: usize = WIDTH / 8;
pub const FRAME_LEN: usize = STRIDE * HEIGHT;

/// A portrait 1-bit frame, row-major, MSB first, set bit = white (the panel
/// convention).
pub struct Frame<'b>(pub &'b mut [u8; FRAME_LEN]);

impl Frame<'_> {
    pub fn clear(&mut self) {
        self.0.fill(0xFF);
    }

    pub fn ink(&mut self, x: i32, y: i32) {
        if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
            let (x, y) = (x as usize, y as usize);
            self.0[y * STRIDE + x / 8] &= !(0x80 >> (x % 8));
        }
    }

    pub fn is_ink(&self, x: usize, y: usize) -> bool {
        self.0[y * STRIDE + x / 8] & (0x80 >> (x % 8)) == 0
    }

    fn glyph(&mut self, g: &Glyph<'_>, x: i32, baseline: i32) {
        let x0 = x + g.left as i32;
        let y0 = baseline - g.top as i32;
        for gy in 0..g.height as usize {
            for gx in 0..g.width as usize {
                if g.ink(gx, gy) {
                    self.ink(x0 + gx as i32, y0 + gy as i32);
                }
            }
        }
    }

    /// One raw panel row (HARDWARE.md, Phase 2.2): raw row `r` is portrait
    /// column `r`, and raw x runs down the portrait rows.
    pub fn raw_row(&self, r: usize, out: &mut [u8; panel::ROW_BYTES]) {
        out.fill(0xFF);
        if r >= WIDTH {
            return;
        }
        for y in 0..HEIGHT {
            if self.is_ink(r, y) {
                out[y / 8] &= !(0x80 >> (y % 8));
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Style<'a> {
    pub body: Font<'a>,
    pub heading: Font<'a>,
    pub margin_x: u16,
    pub margin_top: u16,
    pub margin_bottom: u16,
}

impl Style<'_> {
    fn text_width(&self) -> i32 {
        WIDTH as i32 - 2 * self.margin_x as i32
    }
}

/// One laid-out line, as byte ranges into the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    /// First byte drawn (after a heading's "# ").
    pub content: usize,
    /// End of the drawn bytes (exclusive).
    pub end: usize,
    /// Where the next line starts.
    pub next: usize,
    /// Natural width in pixels, including a trailing hyphen.
    pub width: i32,
    pub hyphen: bool,
    pub heading: bool,
    /// Last line of its paragraph (not justified).
    pub last: bool,
}

const SOFT_HYPHEN: char = '\u{AD}';
const NBSP: char = '\u{A0}';

fn breaks(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Lays out the line starting at `offset`.
pub fn line(text: &str, style: &Style<'_>, offset: usize) -> Line {
    let paragraph_start = offset == 0 || text.as_bytes()[offset - 1] == b'\n';
    // A heading too long for one line stays a heading on the lines after.
    let start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let heading = text[start..].starts_with("# ");
    let content = offset + if heading && paragraph_start { 2 } else { 0 };
    let font = if heading { &style.heading } else { &style.body };
    let max = style.text_width();
    let hyphen_w = font.advance('-') as i32;
    let mut width = 0;
    // (end, next, width at end, hyphen)
    let mut candidate: Option<(usize, usize, i32, bool)> = None;
    for (rel, c) in text[content..].char_indices() {
        let i = content + rel;
        if c == '\n' {
            return Line {
                content,
                end: i,
                next: i + 1,
                width,
                hyphen: false,
                heading,
                last: true,
            };
        }
        if c == SOFT_HYPHEN {
            if width + hyphen_w <= max && i > content {
                candidate = Some((i, i + c.len_utf8(), width + hyphen_w, true));
            }
            continue;
        }
        if breaks(c) && i > content {
            candidate = Some((i, i + c.len_utf8(), width, false));
        }
        let adv = font.advance(if c == '\t' { ' ' } else { c }) as i32;
        if width + adv > max && !breaks(c) {
            let (end, next, w, hyphen) = match candidate {
                Some(cand) => cand,
                // One word wider than the line: break inside it, but always
                // take at least one character.
                None if i > content => (i, i, width, false),
                None => (i + c.len_utf8(), i + c.len_utf8(), width + adv, false),
            };
            let mut next = next;
            while text[next..].starts_with([' ', '\t']) {
                next += 1;
            }
            return Line {
                content,
                end,
                next,
                width: w,
                hyphen,
                heading,
                last: false,
            };
        }
        width += adv;
    }
    Line {
        content,
        end: text.len(),
        next: text.len(),
        width,
        hyphen: false,
        heading,
        last: true,
    }
}

fn draw_line(frame: &mut Frame<'_>, text: &str, style: &Style<'_>, l: &Line, baseline: i32) {
    let font = if l.heading {
        &style.heading
    } else {
        &style.body
    };
    let body = &text[l.content..l.end];
    // Trailing spaces neither show nor stretch.
    let body = body.trim_end_matches([' ', '\t']);
    let gaps = body.chars().filter(|&c| breaks(c)).count() as i32;
    let extra = style.text_width() - l.width;
    let justify = !l.last && !l.heading && gaps > 0 && extra > 0;
    let mut x = style.margin_x as i32;
    let mut gap = 0;
    for c in body.chars() {
        match c {
            SOFT_HYPHEN => continue,
            ' ' | '\t' => {
                x += font.advance(' ') as i32;
                if justify {
                    // Spread the remainder evenly, the leftover pixels first.
                    x += extra / gaps + i32::from(gap < extra % gaps);
                }
                gap += 1;
            }
            _ => {
                let g = font.glyph_or_fallback(if c == NBSP { ' ' } else { c });
                frame.glyph(&g, x, baseline);
                x += g.advance as i32;
            }
        }
    }
    if l.hyphen {
        frame.glyph(&font.glyph_or_fallback('-'), x, baseline);
    }
}

/// Rejects text the layout cannot show: too long, or control characters.
pub fn validate(text: &str) -> Result<(), Error> {
    if text.len() > crate::text::MAX_TEXT {
        return Err(Error::TooLarge);
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Lays out the page starting at `offset`, calling `visit` with each line
/// and its baseline, and returns where the next page starts.
fn page(
    text: &str,
    style: &Style<'_>,
    offset: usize,
    mut visit: impl FnMut(&Line, i32),
) -> Result<usize, Error> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err(Error::Invalid);
    }
    let bottom = HEIGHT as i32 - style.margin_bottom as i32;
    let mut top = style.margin_top as i32;
    let mut at = offset;
    while at < text.len() {
        let l = line(text, style, at);
        let font = if l.heading {
            &style.heading
        } else {
            &style.body
        };
        let lh = font.line_height as i32;
        if top + lh > bottom && at != offset {
            break;
        }
        // Centre the glyphs' ascent and descent in the line box.
        let pad = (lh - (font.ascent as i32 - font.descent as i32)) / 2;
        visit(&l, top + pad + font.ascent as i32);
        top += lh;
        if l.next <= at {
            break; // cannot happen; never loop forever on bad input
        }
        at = l.next;
    }
    Ok(at)
}

/// Draws the page starting at `offset` (cleared first) and returns where the
/// next page starts. `offset` must be a character boundary in `text`.
pub fn render_page(
    frame: &mut Frame<'_>,
    text: &str,
    style: &Style<'_>,
    offset: usize,
) -> Result<usize, Error> {
    frame.clear();
    page(text, style, offset, |l, baseline| {
        draw_line(frame, text, style, l, baseline)
    })
}

/// Where the page after `offset` starts, without drawing.
pub fn next_page(text: &str, style: &Style<'_>, offset: usize) -> Result<usize, Error> {
    page(text, style, offset, |_, _| {})
}

/// Where the page before `offset` starts: pages are laid out from the start
/// of the text (layout only, no drawing) until one reaches `offset`.
pub fn previous_page(text: &str, style: &Style<'_>, offset: usize) -> Result<usize, Error> {
    let mut start = 0;
    loop {
        let next = next_page(text, style, start)?;
        if next >= offset || next <= start {
            return Ok(start);
        }
        start = next;
    }
}
