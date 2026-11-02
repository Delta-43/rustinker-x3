//! A window onto a long article's text (ADR 027). Only part of the text is
//! held in RAM. Pages are laid out inside the window, and the window is moved
//! when a page would run past its end or when paging back past its start.
//!
//! Offsets given to and returned by `Window` are absolute UTF-8 byte offsets
//! into the whole text, so saved positions mean the same as before.
//!
//! A window starts at a paragraph start (just after a `\n`) where possible,
//! so headings and justification lay out as they do in the whole text. It
//! starts up to half a window before the place it is loaded for, which
//! leaves at least half a window ahead and some pages behind.
use crate::{
    Error,
    reader::{Frame, Style, next_page, previous_page, render_page},
};

/// Where the window reads its text from: the article's text, `len` bytes.
pub trait Source {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Reads up to `out.len()` bytes at `at`; 0 only at or past the end.
    fn read(&mut self, at: usize, out: &mut [u8]) -> Result<usize, Error>;
}

/// The smallest window: comfortably more than two pages of text.
pub const MIN_WINDOW: usize = 8 * 1024;

pub struct Window<'b> {
    buf: &'b mut [u8],
    base: usize,
    len: usize,
    total: usize,
    loaded: bool,
}

fn read_exact<S: Source>(src: &mut S, at: usize, out: &mut [u8]) -> Result<(), Error> {
    let mut done = 0;
    while done < out.len() {
        let n = src.read(at + done, &mut out[done..])?;
        if n == 0 || n > out.len() - done {
            return Err(Error::Corrupt);
        }
        done += n;
    }
    Ok(())
}

impl<'b> Window<'b> {
    /// `buf` is the window; it must hold at least `MIN_WINDOW` bytes.
    pub fn new(buf: &'b mut [u8], total: usize) -> Result<Self, Error> {
        if buf.len() < MIN_WINDOW {
            return Err(Error::BufferTooSmall);
        }
        if total > crate::text::MAX_TEXT {
            return Err(Error::TooLarge);
        }
        Ok(Self {
            buf,
            base: 0,
            len: 0,
            total,
            loaded: false,
        })
    }

    pub fn total(&self) -> usize {
        self.total
    }

    /// The first absolute byte held.
    pub fn base(&self) -> usize {
        self.base
    }

    /// The text held, starting at `base()`.
    pub fn text(&self) -> &str {
        // `load` only keeps valid UTF-8.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    fn at_end(&self) -> bool {
        self.base + self.len == self.total
    }

    fn holds(&self, at: usize) -> bool {
        self.loaded && at >= self.base && at < self.base + self.len
    }

    /// Loads the window for absolute offset `target`.
    pub fn load<S: Source>(&mut self, src: &mut S, target: usize) -> Result<(), Error> {
        if src.len() != self.total || target > self.total {
            return Err(Error::Invalid);
        }
        self.loaded = false;
        let half = self.buf.len() / 2;
        let mut start = target.saturating_sub(half);
        if start > 0 {
            // The first paragraph start in [start, target]; else the first
            // character boundary at or after `start`.
            let back = &mut self.buf[..target - start];
            read_exact(src, start, back)?;
            start += match back.iter().position(|&b| b == b'\n') {
                Some(n) => n + 1,
                None => back
                    .iter()
                    .position(|&b| b & 0xC0 != 0x80)
                    .unwrap_or(back.len()),
            };
        }
        let len = self.buf.len().min(self.total - start);
        read_exact(src, start, &mut self.buf[..len])?;
        let len = match core::str::from_utf8(&self.buf[..len]) {
            Ok(_) => len,
            // A character cut by the window's end; the next window has it.
            Err(e) if e.error_len().is_none() && start + len < self.total => e.valid_up_to(),
            Err(_) => return Err(Error::Corrupt),
        };
        let text = core::str::from_utf8(&self.buf[..len]).map_err(|_| Error::Corrupt)?;
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(Error::Invalid);
        }
        if len == 0 && start < self.total {
            return Err(Error::Corrupt);
        }
        self.base = start;
        self.len = len;
        self.loaded = true;
        Ok(())
    }

    /// The nearest valid place at or before `at`: a character boundary
    /// inside the text, or 0. Loads the window for it.
    pub fn settle<S: Source>(&mut self, src: &mut S, at: usize) -> Result<usize, Error> {
        let at = if at >= self.total { 0 } else { at };
        if !self.holds(at) {
            self.load(src, at)?;
        }
        let mut rel = at - self.base;
        while rel > 0 && !self.text().is_char_boundary(rel) {
            rel -= 1;
        }
        Ok(self.base + rel)
    }

    /// Ensures the page at `at` lies wholly inside the window.
    fn fit<S: Source>(&mut self, src: &mut S, style: &Style<'_>, at: usize) -> Result<(), Error> {
        if !self.holds(at) && !(self.loaded && at == self.total && self.at_end()) {
            self.load(src, at)?;
        }
        let next = next_page(self.text(), style, at - self.base)?;
        if next == self.len && !self.at_end() {
            // The page may continue past the window: move it to start here.
            self.load(src, at)?;
        }
        Ok(())
    }

    /// Draws the page at absolute `at`; returns where the next page starts.
    pub fn render<S: Source>(
        &mut self,
        src: &mut S,
        frame: &mut Frame<'_>,
        style: &Style<'_>,
        at: usize,
    ) -> Result<usize, Error> {
        self.fit(src, style, at)?;
        let next = render_page(frame, self.text(), style, at - self.base)?;
        Ok(self.base + next)
    }

    /// Where the page before absolute `at` starts.
    pub fn previous<S: Source>(
        &mut self,
        src: &mut S,
        style: &Style<'_>,
        at: usize,
    ) -> Result<usize, Error> {
        if at == 0 {
            return Ok(0);
        }
        let inside = self.loaded && at > self.base && at <= self.base + self.len;
        if !inside || (at - self.base) < MIN_WINDOW / 4 && self.base > 0 {
            // Too close to the window's start to find a whole page before.
            self.load(src, at)?;
        }
        if at == self.base {
            // No paragraph start found before `at` within half a window.
            return Ok(self.base);
        }
        let p = previous_page(self.text(), style, at - self.base)?;
        Ok(self.base + p)
    }
}
