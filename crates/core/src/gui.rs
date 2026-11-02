//! The on-device interface: black and white, Literata, laid
//! out like an e-reader home screen: status bar, a "continue reading" card,
//! icon tiles, rows with progress bars. Paper is white, ink is black; the
//! selection is inverted.
//!
//! Every function draws a whole portrait frame from data the caller owns, so
//! the simulator and the firmware run the same code. No allocation.
use crate::{
    font::Font,
    reader::{Frame, HEIGHT, STRIDE, Style, WIDTH},
};

const MARGIN: i32 = 24;
const STATUS_H: i32 = 44;
const HEADER_H: i32 = 112;
const FOOTER_H: i32 = 56;
const ROW_H: i32 = 64;
const ROW_H_BAR: i32 = 88;
const ROW_H_CAPTION: i32 = 104;
const BAR_H: i32 = 20;
/// Centres of the four bottom buttons, left to right: Left, Right, Back,
/// Confirm. Measured on a photo of the device (2026-10-31): two
/// pill-shaped bars with a button at each end, so two pairs; each pair spread
/// 10 px wider, after trying it on the panel.
const KEY_X: [i32; 4] = [88, 217, 308, 436];
/// Vertical centre of the two side keys.
const SIDE_Y: i32 = 199;

fn set(frame: &mut Frame<'_>, x: i32, y: i32, ink: bool) {
    if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
        let (x, y) = (x as usize, y as usize);
        let bit = 0x80 >> (x % 8);
        if ink {
            frame.0[y * STRIDE + x / 8] &= !bit;
        } else {
            frame.0[y * STRIDE + x / 8] |= bit;
        }
    }
}

/// Fills a rectangle with ink (`true`) or paper (`false`).
pub fn fill(frame: &mut Frame<'_>, x: i32, y: i32, w: i32, h: i32, ink: bool) {
    for yy in y..y + h {
        for xx in x..x + w {
            set(frame, xx, yy, ink);
        }
    }
}

/// A filled rectangle with rounded corners of radius `r`.
fn rrect(frame: &mut Frame<'_>, x: i32, y: i32, w: i32, h: i32, r: i32, ink: bool) {
    let r = r.min(w / 2).min(h / 2).max(0);
    for yy in 0..h {
        for xx in 0..w {
            let cx = if xx < r {
                r - 1 - xx
            } else if xx >= w - r {
                xx - (w - r)
            } else {
                -1
            };
            let cy = if yy < r {
                r - 1 - yy
            } else if yy >= h - r {
                yy - (h - r)
            } else {
                -1
            };
            let inside = cx < 0
                || cy < 0
                || (cx * 2 + 1) * (cx * 2 + 1) + (cy * 2 + 1) * (cy * 2 + 1) <= 4 * r * r;
            if inside {
                set(frame, x + xx, y + yy, ink);
            }
        }
    }
}

/// A rounded outline `t` pixels thick; the inside is painted `bg`.
#[allow(clippy::too_many_arguments)] // drawing: frame, style, data, geometry
fn rframe(frame: &mut Frame<'_>, x: i32, y: i32, w: i32, h: i32, r: i32, t: i32, ink: bool) {
    rrect(frame, x, y, w, h, r, ink);
    rrect(
        frame,
        x + t,
        y + t,
        w - 2 * t,
        h - 2 * t,
        (r - t).max(0),
        !ink,
    );
}

fn disc(frame: &mut Frame<'_>, cx: i32, cy: i32, r: i32, ink: bool) {
    for y in -r..=r {
        for x in -r..=r {
            if x * x + y * y <= r * r {
                set(frame, cx + x, cy + y, ink);
            }
        }
    }
}

fn ring(frame: &mut Frame<'_>, cx: i32, cy: i32, r: i32, t: i32, ink: bool) {
    for y in -r..=r {
        for x in -r..=r {
            let d = x * x + y * y;
            if d <= r * r && d >= (r - t) * (r - t) {
                set(frame, cx + x, cy + y, ink);
            }
        }
    }
}

/// The four home icons, 44 × 44, at `(x, y)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Book,
    Token,
    Receive,
    Device,
}

fn icon(frame: &mut Frame<'_>, which: Icon, x: i32, y: i32, ink: bool) {
    let bg = !ink;
    match which {
        Icon::Book => {
            rframe(frame, x + 4, y + 2, 36, 40, 4, 3, ink);
            fill(frame, x + 13, y + 2, 3, 40, ink);
            for n in 0..3 {
                fill(frame, x + 21, y + 12 + n * 8, 13, 2, ink);
            }
        }
        Icon::Token => {
            rframe(frame, x + 12, y + 1, 30, 38, 4, 3, ink);
            rrect(frame, x + 2, y + 7, 30, 36, 4, bg);
            rframe(frame, x + 2, y + 7, 30, 36, 4, 3, ink);
            for dy in -9i32..=9 {
                let half = 9 - dy.abs();
                fill(frame, x + 17 - half, y + 25 + dy, half * 2 + 1, 1, ink);
            }
        }
        Icon::Receive => {
            fill(frame, x + 20, y + 3, 4, 22, ink);
            for k in 0..11 {
                fill(frame, x + 22 - (11 - k), y + 22 + k, (11 - k) * 2, 1, ink);
            }
            fill(frame, x + 4, y + 30, 3, 12, ink);
            fill(frame, x + 37, y + 30, 3, 12, ink);
            fill(frame, x + 4, y + 39, 36, 3, ink);
        }
        Icon::Device => {
            ring(frame, x + 22, y + 22, 20, 3, ink);
            disc(frame, x + 22, y + 13, 3, ink);
            fill(frame, x + 20, y + 20, 4, 14, ink);
        }
    }
}

/// The status strip drawn on every screen.
#[derive(Debug, Clone, Copy)]
pub struct Status<'a> {
    pub clock: &'a str,
    /// Battery charge, 0–100.
    pub battery: u8,
    pub wifi: bool,
}

fn status_bar(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>) {
    let b = centred(&style.body, 0, STATUS_H);
    text(frame, &style.body, MARGIN, b, st.clock, true);
    // Battery: outline, nub and a fill in proportion.
    let (bw, bh) = (38, 18);
    let bx = WIDTH as i32 - MARGIN - bw - 4;
    let by = (STATUS_H - bh) / 2;
    rframe(frame, bx, by, bw, bh, 3, 2, true);
    fill(frame, bx + bw, by + 5, 3, bh - 10, true);
    let level = i32::from(st.battery.min(100));
    fill(frame, bx + 4, by + 4, (bw - 8) * level / 100, bh - 8, true);
    let mut pct = NoteBuf::default();
    let _ = core::fmt::write(&mut pct, format_args!("{}%", st.battery.min(100)));
    let pw = text_width(&style.body, pct.as_str());
    text(frame, &style.body, bx - 12 - pw, b, pct.as_str(), true);
    if st.wifi {
        // Three arcs over a dot, to the left of the percentage.
        let cx = bx - 12 - pw - 34;
        let cy = STATUS_H / 2 + 9;
        disc(frame, cx, cy, 2, true);
        for r in [8i32, 14, 20] {
            for y in -r..0 {
                for x in -r..=r {
                    let d = x * x + y * y;
                    if d <= r * r && d >= (r - 2) * (r - 2) && x.abs() <= -y {
                        set(frame, cx + x, cy + y, true);
                    }
                }
            }
        }
    }
}

/// Width of `s` in pixels.
pub fn text_width(font: &Font<'_>, s: &str) -> i32 {
    s.chars()
        .filter(|&c| c != '\u{ad}')
        .map(|c| font.advance(c) as i32)
        .sum()
}

/// Draws `s` with its baseline at `baseline`; returns the width drawn.
pub fn text(
    frame: &mut Frame<'_>,
    font: &Font<'_>,
    x: i32,
    baseline: i32,
    s: &str,
    ink: bool,
) -> i32 {
    let mut pen = x;
    for c in s.chars().filter(|&c| c != '\u{ad}') {
        let g = font.glyph_or_fallback(c);
        let (x0, y0) = (pen + g.left as i32, baseline - g.top as i32);
        for gy in 0..g.height as usize {
            for gx in 0..g.width as usize {
                if g.ink(gx, gy) {
                    set(frame, x0 + gx as i32, y0 + gy as i32, ink);
                }
            }
        }
        pen += g.advance as i32;
    }
    pen - x
}

/// The longest prefix of `s` that fits in `max` pixels, ending on a character
/// boundary. When it cuts, the result leaves room for an ellipsis.
pub fn fit<'a>(font: &Font<'_>, s: &'a str, max: i32) -> (&'a str, bool) {
    if text_width(font, s) <= max {
        return (s, false);
    }
    let room = max - font.advance('…') as i32;
    let mut used = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        used += font.advance(c) as i32;
        if used > room {
            break;
        }
        end = i + c.len_utf8();
    }
    (s[..end].trim_end(), true)
}

/// Draws `s` cut to `max` pixels, with an ellipsis if it was cut.
fn text_fit(
    frame: &mut Frame<'_>,
    font: &Font<'_>,
    x: i32,
    baseline: i32,
    s: &str,
    max: i32,
    ink: bool,
) {
    let (shown, cut) = fit(font, s, max);
    let w = text(frame, font, x, baseline, shown, ink);
    if cut {
        text(frame, font, x + w, baseline, "…", ink);
    }
}

/// Baseline that centres `font`'s ascent and descent in a box of height `h`.
fn centred(font: &Font<'_>, top: i32, h: i32) -> i32 {
    top + (h - (font.ascent as i32 - font.descent as i32)) / 2 + font.ascent as i32
}

fn rule(frame: &mut Frame<'_>, y: i32, thick: i32) {
    fill(frame, MARGIN, y, WIDTH as i32 - 2 * MARGIN, thick, true);
}

fn header(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>, title: &str, note: &str) {
    frame.clear();
    status_bar(frame, style, st);
    let inner = WIDTH as i32 - 2 * MARGIN;
    let note_w = text_width(&style.body, note);
    let band = HEADER_H - 8 - STATUS_H;
    let base = centred(&style.heading, STATUS_H, band);
    text_fit(
        frame,
        &style.heading,
        MARGIN,
        base,
        title,
        inner - note_w - 16,
        true,
    );
    if !note.is_empty() {
        let b = centred(&style.body, STATUS_H, band);
        text(
            frame,
            &style.body,
            WIDTH as i32 - MARGIN - note_w,
            b,
            note,
            true,
        );
    }
    rule(frame, HEADER_H - 8, 2);
}

/// Which buttons do something on a screen. The bottom bar draws a symbol for
/// each one in the order the keys sit on the X3: Left, Right, Back, Confirm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keys {
    pub left: bool,
    pub right: bool,
    pub back: bool,
    pub confirm: bool,
    /// The two side keys, mid-height on each edge: `<` left, `>` right.
    pub side: bool,
}

impl Keys {
    pub const NONE: Self = Self {
        left: false,
        right: false,
        back: false,
        confirm: false,
        side: false,
    };
    /// Every key: lists and Home.
    pub const ALL: Self = Self {
        left: true,
        right: true,
        back: true,
        confirm: true,
        side: true,
    };
    /// Home: every key. Back refreshes the screen.
    pub const HOME: Self = Self::ALL;
    /// Only Back.
    pub const BACK: Self = Self {
        back: true,
        ..Self::NONE
    };
}

/// A two-pixel line. With `toggle` it flips what is there, so a symbol stays
/// visible over both paper and an inverted row.
fn stroke(frame: &mut Frame<'_>, a: (i32, i32), b: (i32, i32), toggle: bool) {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let mut err = dx + dy;
    loop {
        for (ox, oy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            if toggle {
                let (px, py) = (x + ox, y + oy);
                if (0..WIDTH as i32).contains(&px) && (0..HEIGHT as i32).contains(&py) {
                    let ink = frame.is_ink(px as usize, py as usize);
                    set(frame, px, py, !ink);
                }
            } else {
                set(frame, x + ox, y + oy, true);
            }
        }
        if (x, y) == b {
            return;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// A chevron pointing `dir` ((-1,0) left, (1,0) right, (0,-1) up, (0,1) down).
fn chevron(frame: &mut Frame<'_>, cx: i32, cy: i32, dir: (i32, i32), toggle: bool) {
    let (tip, a, b) = if dir.0 != 0 {
        (
            (cx + dir.0 * 5, cy),
            (cx - dir.0 * 5, cy - 10),
            (cx - dir.0 * 5, cy + 10),
        )
    } else {
        (
            (cx, cy + dir.1 * 5),
            (cx - 10, cy - dir.1 * 5),
            (cx + 10, cy - dir.1 * 5),
        )
    };
    stroke(frame, a, tip, toggle);
    stroke(frame, tip, b, toggle);
}

/// The bottom bar: a hairline and one thin symbol per key that works here,
/// over where its button sits. The side keys get a chevron at each edge.
fn footer(frame: &mut Frame<'_>, keys: Keys) {
    let y = HEIGHT as i32 - FOOTER_H;
    rule(frame, y, 1);
    let cy = y + FOOTER_H / 2;
    if keys.left {
        chevron(frame, KEY_X[0], cy, (-1, 0), false);
    }
    if keys.right {
        chevron(frame, KEY_X[1], cy, (1, 0), false);
    }
    if keys.back {
        let x = KEY_X[2];
        stroke(frame, (x - 13, cy), (x + 13, cy), false);
        stroke(frame, (x - 13, cy), (x - 5, cy - 8), false);
        stroke(frame, (x - 13, cy), (x - 5, cy + 8), false);
    }
    if keys.confirm {
        ring(frame, KEY_X[3], cy, 11, 2, true);
    }
    if keys.side {
        // Each side key is a raised bar about 160 px tall on screen scale,
        // centred near y = 199 (measured on a photo, 2026-10-31):
        // `<` on the left edge, `>` on the right.
        chevron(frame, 12, SIDE_Y, (-1, 0), true);
        chevron(frame, WIDTH as i32 - 13, SIDE_Y, (1, 0), true);
    }
}

/// A thin progress bar: a rounded outline with a rounded fill, `pct` 0–100.
fn bar(frame: &mut Frame<'_>, x: i32, y: i32, w: i32, h: i32, pct: u8, ink: bool) {
    rframe(frame, x, y, w, h, h / 2, 2, ink);
    let inner = w - 6;
    let filled = inner * i32::from(pct.min(100)) / 100;
    if filled > 0 {
        rrect(
            frame,
            x + 3,
            y + 3,
            filled.max(h - 6),
            h - 6,
            (h - 6) / 2,
            ink,
        );
    }
}

/// One line of a list: a title, an optional right-hand label and an optional
/// caption under the title.
#[derive(Debug, Clone, Copy, Default)]
pub struct Row<'a> {
    pub title: &'a str,
    pub right: &'a str,
    pub caption: &'a str,
    /// Reading progress, drawn as a bar under the title.
    pub bar: Option<u8>,
    /// Draw `right` in an outlined pill (for "new").
    pub pill: bool,
    /// Set the title in bold (an unread article).
    pub strong: bool,
}

fn row_height(rows: &[Row<'_>]) -> i32 {
    if rows.iter().any(|r| !r.caption.is_empty()) {
        ROW_H_CAPTION
    } else if rows.iter().any(|r| r.bar.is_some()) {
        ROW_H_BAR
    } else {
        ROW_H
    }
}

/// How many rows one screen shows for `rows`.
pub fn rows_per_screen(rows: &[Row<'_>]) -> usize {
    ((HEIGHT as i32 - HEADER_H - FOOTER_H) / row_height(rows)).max(1) as usize
}

/// A list screen. The selected row is inverted. Long lists page: the screen
/// shows the page that holds `selected`, and the header says which.
#[allow(clippy::too_many_arguments)] // drawing: frame, style, data, geometry
pub fn list(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    title: &str,
    rows: &[Row<'_>],
    selected: usize,
    empty: &str,
    keys: Keys,
) {
    let per = rows_per_screen(rows);
    let pages = rows.len().div_ceil(per).max(1);
    let page = selected.min(rows.len().saturating_sub(1)) / per;
    let mut note_buf = NoteBuf::default();
    if pages > 1 {
        let _ = core::fmt::write(&mut note_buf, format_args!("{}/{}", page + 1, pages));
    }
    header(frame, style, st, title, note_buf.as_str());
    footer(frame, keys);
    let h = row_height(rows);
    let inner = WIDTH as i32 - 2 * MARGIN;
    if rows.is_empty() {
        wrapped(frame, &style.body, MARGIN, HEADER_H + 24, inner, empty);
        return;
    }
    for (n, row) in rows.iter().enumerate().skip(page * per).take(per) {
        let top = HEADER_H + (n - page * per) as i32 * h;
        let on = n != selected;
        if !on {
            fill(frame, 0, top, WIDTH as i32, h, true);
        } else {
            fill(frame, MARGIN, top + h - 1, inner, 1, true);
        }
        let right_w = text_width(&style.body, row.right);
        let pill_pad = if row.pill { 28 } else { 0 };
        let right_room = if right_w > 0 {
            right_w + pill_pad + 16
        } else {
            0
        };
        let title_font = if row.strong || !row.caption.is_empty() {
            &style.heading
        } else {
            &style.body
        };
        // Where the title sits, and the baseline the right label shares.
        let base = if !row.caption.is_empty() {
            top + 14 + style.heading.ascent as i32
        } else if row.bar.is_some() {
            top + 12 + title_font.ascent as i32
        } else {
            centred(title_font, top, h)
        };
        text_fit(
            frame,
            title_font,
            MARGIN,
            base,
            row.title,
            inner - right_room,
            on,
        );
        if !row.caption.is_empty() {
            let b2 = base + 14 + style.body.ascent as i32;
            text_fit(frame, &style.body, MARGIN, b2, row.caption, inner, on);
        }
        if let Some(pct) = row.bar {
            bar(frame, MARGIN, top + h - 30, inner, 14, pct, on);
        }
        if right_w > 0 {
            let x = WIDTH as i32 - MARGIN - right_w - pill_pad / 2;
            if row.pill {
                let (asc, desc) = (style.body.ascent as i32, style.body.descent as i32);
                let ph = asc - desc + 12;
                let py = base - asc - 6;
                rframe(frame, x - 14, py, right_w + pill_pad, ph, ph / 2, 2, on);
            }
            text(frame, &style.body, x, base, row.right, on);
        }
    }
}

/// What the Home screen shows for the article you were last reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hero<'a> {
    pub title: &'a str,
    pub caption: &'a str,
    pub percent: u8,
}

/// One tile of the Home grid.
#[derive(Debug, Clone, Copy)]
pub struct Tile<'a> {
    pub icon: Icon,
    pub label: &'a str,
    pub caption: &'a str,
}

/// The Home screen: status bar, a "continue reading" card and four tiles.
/// `selected` 0 is the card; 1–4 are the tiles in reading order. `hero` is
/// `None` when nothing is on the card yet.
#[allow(clippy::too_many_arguments)] // drawing: frame, style, data, geometry
pub fn home(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    hero: Option<&Hero<'_>>,
    tiles: &[Tile<'_>; 4],
    selected: usize,
    keys: Keys,
) {
    frame.clear();
    status_bar(frame, style, st);
    footer(frame, keys);
    let inner = WIDTH as i32 - 2 * MARGIN;
    // The card.
    let (top, h) = (STATUS_H + 8, 204);
    let on = selected != 0;
    if on {
        rframe(frame, MARGIN, top, inner, h, 18, 3, true);
    } else {
        rrect(frame, MARGIN, top, inner, h, 18, true);
    }
    let pad = 24;
    let x = MARGIN + pad;
    let room = inner - 2 * pad;
    let eyebrow = if hero.is_some() {
        "Continue reading"
    } else {
        "Nothing to read yet"
    };
    let b = top + 18 + style.body.ascent as i32;
    text(frame, &style.body, x, b, eyebrow, on);
    match hero {
        Some(hero) => {
            let tb = b + style.body.line_height as i32 - style.body.ascent as i32
                + 8
                + style.heading.ascent as i32;
            text_fit(frame, &style.heading, x, tb, hero.title, room, on);
            bar(frame, x, top + h - 76, room, 16, hero.percent, on);
            let cb = top + h - 76 + 16 + 10 + style.body.ascent as i32;
            text_fit(frame, &style.body, x, cb, hero.caption, room, on);
        }
        None => {
            let tb = b + style.body.line_height as i32 + 8;
            wrapped_ink(
                frame,
                &style.body,
                x,
                tb - style.body.ascent as i32,
                room,
                "Send an article from your phone, then choose Receive.",
                on,
            );
        }
    }
    // The tiles: two by two.
    let gap = 16;
    let tw = (inner - gap) / 2;
    let th = 176;
    let grid_top = top + h + gap;
    for (i, tile) in tiles.iter().enumerate() {
        let tx = MARGIN + (i as i32 % 2) * (tw + gap);
        let ty = grid_top + (i as i32 / 2) * (th + gap);
        let on = selected != i + 1;
        if on {
            rframe(frame, tx, ty, tw, th, 18, 3, true);
        } else {
            rrect(frame, tx, ty, tw, th, 18, true);
        }
        icon(frame, tile.icon, tx + 22, ty + 22, on);
        let lb = ty + 22 + 44 + 16 + style.heading.ascent as i32;
        text_fit(frame, &style.heading, tx + 22, lb, tile.label, tw - 44, on);
        let cb = lb + 10 + style.body.ascent as i32;
        text_fit(frame, &style.body, tx + 22, cb, tile.caption, tw - 44, on);
    }
}

/// Labelled values, then one action drawn as a selected row (inverse
/// video) under them, so it reads as something to press, not as data: the
/// Storage screen's "Delete everything". Confirm runs it.
pub fn pairs_action(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    title: &str,
    items: &[(&str, &str)],
    action: &str,
    keys: Keys,
) {
    let top = pairs(frame, style, st, title, items, keys) + 24;
    let h = ROW_H;
    if top + h > HEIGHT as i32 - FOOTER_H {
        return;
    }
    fill(frame, 0, top, WIDTH as i32, h, true);
    let b = centred(&style.heading, top, h);
    text_fit(
        frame,
        &style.heading,
        MARGIN,
        b,
        action,
        WIDTH as i32 - 2 * MARGIN,
        false,
    );
}

/// A list of labelled values, such as the Device screen. Returns the y just
/// below the last row drawn.
pub fn pairs(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    title: &str,
    items: &[(&str, &str)],
    keys: Keys,
) -> i32 {
    header(frame, style, st, title, "");
    footer(frame, keys);
    let inner = WIDTH as i32 - 2 * MARGIN;
    // More rows than fit at the full height: close them up, down to a line
    // of text with a little air, so nothing is silently dropped.
    let room = HEIGHT as i32 - HEADER_H - FOOTER_H;
    let tightest = style.body.ascent as i32 - style.body.descent as i32 + 10;
    let h = (room / items.len().max(1) as i32).clamp(tightest.min(ROW_H), ROW_H);
    let fits = (room / h) as usize;
    for (n, (label, value)) in items.iter().take(fits).enumerate() {
        let top = HEADER_H + n as i32 * h;
        let b = centred(&style.body, top, h);
        let lw = text(frame, &style.body, MARGIN, b, label, true);
        let room = inner - lw - 24;
        let (shown, cut) = fit(&style.body, value, room);
        let vw = text_width(&style.body, shown)
            + if cut {
                style.body.advance('…') as i32
            } else {
                0
            };
        let x = WIDTH as i32 - MARGIN - vw;
        let w = text(frame, &style.body, x, b, shown, true);
        if cut {
            text(frame, &style.body, x + w, b, "…", true);
        }
        fill(frame, MARGIN, top + h - 1, inner, 1, true);
    }
    HEADER_H + items.len().min(fits) as i32 * h
}

/// One step of a Receive: what it is and how it went.
#[derive(Debug, Clone, Copy)]
pub struct Step<'a> {
    pub label: &'a str,
    pub state: &'a str,
}

/// The Receive screen: a progress bar (`done` of `total` items) over the
/// steps so far, newest last.
#[allow(clippy::too_many_arguments)] // drawing: frame, style, data, geometry
pub fn receive(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    title: &str,
    done: usize,
    total: usize,
    steps: &[Step<'_>],
    keys: Keys,
) {
    header(frame, style, st, title, "");
    footer(frame, keys);
    let inner = WIDTH as i32 - 2 * MARGIN;
    let top = HEADER_H + 16;
    let pct = (done.min(total) * 100).checked_div(total).unwrap_or(0) as u8;
    bar(frame, MARGIN, top, inner, BAR_H, pct, true);
    let mut caption = NoteBuf::default();
    let _ = core::fmt::write(&mut caption, format_args!("{done} of {total}"));
    let b = top + BAR_H + 12 + style.body.ascent as i32;
    text(frame, &style.body, MARGIN, b, caption.as_str(), true);
    let list_top = top + BAR_H + 12 + style.body.line_height as i32 + 16;
    let h = 56;
    let fits = ((HEIGHT as i32 - FOOTER_H - list_top) / h).max(0) as usize;
    // Show the newest steps when there are more than fit.
    let skip = steps.len().saturating_sub(fits);
    for (n, s) in steps.iter().skip(skip).enumerate() {
        let y = list_top + n as i32 * h;
        let b = centred(&style.body, y, h);
        let sw = text_width(&style.body, s.state);
        text(
            frame,
            &style.body,
            WIDTH as i32 - MARGIN - sw,
            b,
            s.state,
            true,
        );
        text_fit(
            frame,
            &style.body,
            MARGIN,
            b,
            s.label,
            inner - sw - 16,
            true,
        );
        fill(frame, MARGIN, y + h - 1, inner, 1, true);
    }
}

/// Plain wrapped text for messages. Breaks at spaces; very long words are cut.
fn wrapped(frame: &mut Frame<'_>, font: &Font<'_>, x: i32, top: i32, width: i32, s: &str) {
    wrapped_ink(frame, font, x, top, width, s, true);
}

fn wrapped_ink(
    frame: &mut Frame<'_>,
    font: &Font<'_>,
    x: i32,
    top: i32,
    width: i32,
    s: &str,
    ink: bool,
) {
    let mut y = top + font.ascent as i32;
    for para in s.split('\n') {
        let mut start = 0;
        let bytes = para.as_bytes();
        while start < para.len() {
            let mut end = start;
            let mut last_space = None;
            let mut w = 0;
            for (i, c) in para[start..].char_indices() {
                w += font.advance(c) as i32;
                if w > width {
                    break;
                }
                end = start + i + c.len_utf8();
                if bytes[start + i] == b' ' {
                    last_space = Some(start + i);
                }
            }
            let cut = if end >= para.len() {
                para.len()
            } else {
                last_space
                    .filter(|&s| s > start)
                    .unwrap_or(end.max(start + 1))
            };
            text(frame, font, x, y, para[start..cut].trim_end(), ink);
            y += font.line_height as i32;
            start = cut;
            while start < para.len() && bytes[start] == b' ' {
                start += 1;
            }
        }
        if para.is_empty() {
            y += font.line_height as i32;
        }
    }
}

/// A short message under a title, for errors and empty states.
pub fn message(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    title: &str,
    body: &str,
    keys: Keys,
) {
    header(frame, style, st, title, "");
    footer(frame, keys);
    wrapped(
        frame,
        &style.body,
        MARGIN,
        HEADER_H + 24,
        WIDTH as i32 - 2 * MARGIN,
        body,
    );
}

/// Draws `data` as a QR code with its top-left corner at (`x`, `y`), at most
/// `max` pixels square including a two-module quiet border. Returns the size
/// drawn, or `None` if the text does not fit a small code (nothing is drawn).
fn qr(frame: &mut Frame<'_>, x: i32, y: i32, max: i32, data: &str) -> Option<i32> {
    use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
    const BUF: usize = Version::new(6).buffer_len();
    let mut temp = [0u8; BUF];
    let mut out = [0u8; BUF];
    let code = QrCode::encode_text(
        data,
        &mut temp,
        &mut out,
        QrCodeEcc::Low,
        Version::MIN,
        Version::new(6),
        None,
        true,
    )
    .ok()?;
    const QUIET: i32 = 2;
    let modules = code.size();
    let scale = max / (modules + 2 * QUIET);
    if scale < 3 {
        return None;
    }
    let side = (modules + 2 * QUIET) * scale;
    fill(frame, x, y, side, side, false);
    for my in 0..modules {
        for mx in 0..modules {
            if code.get_module(mx, my) {
                fill(
                    frame,
                    x + (mx + QUIET) * scale,
                    y + (my + QUIET) * scale,
                    scale,
                    scale,
                    true,
                );
            }
        }
    }
    Some(side)
}

/// The Wi-Fi setup screen (ADR 025): what to do on the phone, the hotspot name
/// and password, and a QR code (`join_text`) the iPhone camera reads.
pub fn wifi_setup(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    ssid: &str,
    password: &str,
    join_text: &str,
    keys: Keys,
) {
    header(frame, style, st, "Wi-Fi setup", "");
    footer(frame, keys);
    let top = HEADER_H + 12;
    // Centred, so the camera sees a code with a wide margin around it.
    let mut y = top;
    if let Some(side) = qr(frame, (WIDTH as i32 - 252) / 2, top, 252, join_text) {
        y += side + 12;
    }
    wrapped(
        frame,
        &style.body,
        MARGIN,
        y,
        WIDTH as i32 - 2 * MARGIN,
        "Scan the code with the camera, or join the network below. Then open 192.168.4.1 in a browser, choose your network and enter its password.",
    );
    let line = style.body.line_height as i32;
    let y = HEIGHT as i32 - FOOTER_H - 16 - 2 * line;
    text(
        frame,
        &style.body,
        MARGIN,
        y + style.body.ascent as i32,
        "Network",
        true,
    );
    text(
        frame,
        &style.heading,
        MARGIN + 124,
        y + style.body.ascent as i32,
        ssid,
        true,
    );
    let y = y + line;
    text(
        frame,
        &style.body,
        MARGIN,
        y + style.body.ascent as i32,
        "Password",
        true,
    );
    text(
        frame,
        &style.heading,
        MARGIN + 124,
        y + style.body.ascent as i32,
        password,
        true,
    );
}

/// A tiny fixed buffer for the header note and counters.
#[derive(Default)]
struct NoteBuf {
    buf: [u8; 24],
    len: usize,
}

impl core::fmt::Write for NoteBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

impl NoteBuf {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::reader::FRAME_LEN;
    use std::{vec, vec::Vec};

    static BODY: &[u8] = include_bytes!("../../../assets/fonts/literata-regular-24.x3f");
    static HEAD: &[u8] = include_bytes!("../../../assets/fonts/literata-bold-28.x3f");

    fn style() -> Style<'static> {
        Style {
            body: Font::parse(BODY).unwrap(),
            heading: Font::parse(HEAD).unwrap(),
            margin_x: 20,
            margin_top: 24,
            margin_bottom: 48,
        }
    }

    const STAT: Status<'static> = Status {
        clock: "14:07",
        battery: 50,
        wifi: true,
    };

    fn row(title: &str) -> Row<'_> {
        Row {
            title,
            ..Row::default()
        }
    }

    fn ink_in(frame: &Frame<'_>, x: usize, y: usize, w: usize, h: usize) -> usize {
        (y..y + h)
            .flat_map(|yy| (x..x + w).map(move |xx| (xx, yy)))
            .filter(|&(xx, yy)| frame.is_ink(xx, yy))
            .count()
    }

    #[test]
    fn selected_row_is_inverted_and_others_are_not() {
        let st = style();
        let stat = STAT;
        let rows = [row("One"), row("Two"), row("Three")];
        let mut buf = vec![0u8; FRAME_LEN];
        let mut f = Frame(buf.as_mut_slice().try_into().unwrap());
        list(&mut f, &st, &stat, "T", &rows, 1, "", Keys::ALL);
        let top = HEADER_H as usize + ROW_H as usize;
        // The selected row's right margin is solid ink; the others' is paper.
        assert_eq!(ink_in(&f, 480, top + 4, 40, 50), 40 * 50);
        assert_eq!(ink_in(&f, 480, HEADER_H as usize + 4, 40, 50), 0);
        // Its text is drawn in paper, so the row is not solid.
        assert!(ink_in(&f, 24, top + 4, 200, 50) < 200 * 50);
    }

    #[test]
    fn fit_cuts_on_a_boundary_and_leaves_room_for_the_ellipsis() {
        let st = style();
        let long = "Zażółć gęślą jaźń Zażółć gęślą jaźń Zażółć gęślą jaźń";
        let (shown, cut) = fit(&st.body, long, 200);
        assert!(cut);
        assert!(long.starts_with(shown));
        assert!(text_width(&st.body, shown) + st.body.advance('…') as i32 <= 200);
        assert_eq!(fit(&st.body, "Short", 200), ("Short", false));
        assert_eq!(fit(&st.body, long, 0).0, "");
    }

    #[test]
    fn long_lists_page_and_hostile_input_never_panics() {
        let st = style();
        let stat = STAT;
        let titles: Vec<std::string::String> = (0..70).map(|i| "W".repeat(i * 7)).collect();
        let rows: Vec<Row<'_>> = titles
            .iter()
            .map(|t| Row {
                title: t,
                right: "99%",
                bar: Some(50),
                pill: true,
                ..Row::default()
            })
            .collect();
        let per = rows_per_screen(&rows);
        assert!((6..=12).contains(&per));
        let mut buf = vec![0u8; FRAME_LEN];
        let mut f = Frame(buf.as_mut_slice().try_into().unwrap());
        for sel in [0, per - 1, per, 69, 500] {
            list(&mut f, &st, &stat, "Library", &rows, sel, "", Keys::ALL);
        }
        list(&mut f, &st, &stat, "", &[], 0, "", Keys::NONE);
        list(
            &mut f,
            &st,
            &stat,
            "T",
            &[],
            3,
            "Nothing here.\n\nSend one.",
            Keys::BACK,
        );
        let long_word = "x".repeat(900);
        message(&mut f, &st, &stat, &long_word, &long_word, Keys::ALL);
        let long_ref: &str = &long_word;
        pairs(
            &mut f,
            &st,
            &stat,
            "D",
            &[(long_ref, long_ref); 30],
            Keys::BACK,
        );
        let steps = [Step {
            label: &long_word,
            state: "ok",
        }; 40];
        receive(&mut f, &st, &stat, "R", 9, 3, &steps, Keys::NONE);
        receive(&mut f, &st, &stat, "R", 0, 0, &[], Keys::NONE);
        // Emoji and a missing glyph fall back instead of failing.
        message(&mut f, &st, &stat, "😀", "😀 \u{0}\u{fffd}", Keys::BACK);
    }

    #[test]
    fn progress_bar_fills_in_proportion() {
        let st = style();
        let stat = STAT;
        let mut buf = vec![0u8; FRAME_LEN];
        let mut f = Frame(buf.as_mut_slice().try_into().unwrap());
        receive(&mut f, &st, &stat, "R", 1, 2, &[], Keys::NONE);
        let y = HEADER_H as usize + 16 + BAR_H as usize / 2;
        let inked = (MARGIN as usize + 3..WIDTH - MARGIN as usize - 3)
            .filter(|&x| f.is_ink(x, y))
            .count();
        let inner = WIDTH - 2 * MARGIN as usize - 6;
        assert!(inked.abs_diff(inner / 2) <= 3, "{inked} of {inner}");
    }

    #[test]
    fn home_inverts_only_the_selection_and_survives_hostile_data() {
        let st = style();
        let stat = STAT;
        let tile = |icon, label| Tile {
            icon,
            label,
            caption: "x",
        };
        let tiles = [
            tile(Icon::Book, "Library"),
            tile(Icon::Token, "Tokens"),
            tile(Icon::Receive, "Receive"),
            tile(Icon::Device, "Device"),
        ];
        let long = "W".repeat(900);
        let hero = Hero {
            title: &long,
            caption: &long,
            percent: 250,
        };
        let mut buf = vec![0u8; FRAME_LEN];
        let mut f = Frame(buf.as_mut_slice().try_into().unwrap());
        for sel in 0..8 {
            home(&mut f, &st, &stat, Some(&hero), &tiles, sel, Keys::HOME);
            home(&mut f, &st, &stat, None, &tiles, sel, Keys::HOME);
        }
        // Tile 2 (index 1 + 1) selected: its body is solid ink, tile 1's is not.
        home(&mut f, &st, &stat, None, &tiles, 2, Keys::HOME);
        let inner = WIDTH as i32 - 2 * MARGIN;
        let tw = (inner - 16) / 2;
        let grid_top = (STATUS_H + 8 + 204 + 16) as usize;
        let second_x = (MARGIN + tw + 16 + tw - 12) as usize;
        assert_eq!(ink_in(&f, second_x, grid_top + 100, 8, 20), 8 * 20);
        assert_eq!(
            ink_in(
                &f,
                MARGIN as usize + tw as usize - 12,
                grid_top + 100,
                8,
                20
            ),
            0
        );
    }
}
