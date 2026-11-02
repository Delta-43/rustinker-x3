use crate::Error;
pub const MAX_TEXT: usize = 262_144;

/// Source byte range, preserving a stable reading position across UTF-8 text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
    pub hyphen: bool,
    pub heading: bool,
}
/// Greedy monospace line wrapping. Soft hyphens consume no display column
/// unless selected as a break. Unsupported glyph substitution belongs to Display.
pub struct Lines<'a> {
    text: &'a str,
    offset: usize,
    columns: usize,
}
impl<'a> Lines<'a> {
    pub fn new(text: &'a str, columns: usize, offset: usize) -> Result<Self, Error> {
        if text.len() > MAX_TEXT {
            return Err(Error::TooLarge);
        }
        if columns == 0
            || columns > 256
            || offset > text.len()
            || !text.is_char_boundary(offset)
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            text,
            offset,
            columns,
        })
    }
    pub fn position(&self) -> usize {
        self.offset
    }
}
impl<'a> Iterator for Lines<'a> {
    type Item = Line<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.text.len() {
            return None;
        }
        let start = self.offset;
        let heading = (start == 0 || self.text.as_bytes()[start - 1] == b'\n')
            && self.text[start..].starts_with("# ");
        let content = start + if heading { 2 } else { 0 };
        let mut visible = 0;
        let mut breakpoint = None;
        let mut end = self.text.len();
        let mut next = end;
        let mut hyphen = false;
        for (relative, c) in self.text[content..].char_indices() {
            let i = content + relative;
            if c == '\n' {
                end = i;
                next = i + 1;
                break;
            }
            if c == '\u{ad}' {
                if visible < self.columns {
                    breakpoint = Some((i, i + c.len_utf8(), true));
                }
                continue;
            }
            if c == ' ' || c == '\t' {
                breakpoint = Some((i, i + c.len_utf8(), false));
            }
            if visible == self.columns {
                if let Some((e, n, h)) = breakpoint.filter(|(e, _, _)| *e > content) {
                    end = e;
                    next = n;
                    hyphen = h;
                } else {
                    end = i;
                    next = i;
                }
                break;
            }
            visible += 1;
        }
        // Drop wrapping spaces, but preserve explicit paragraph newlines.
        while next < self.text.len() && matches!(self.text.as_bytes()[next], b' ' | b'\t') {
            next += 1;
        }
        self.offset = next;
        Some(Line {
            text: &self.text[content..end],
            start,
            end: next,
            hyphen,
            heading,
        })
    }
}

/// Visits at most `rows` lines; returns the next page's UTF-8 byte position.
pub fn page<'a>(
    text: &'a str,
    columns: usize,
    rows: usize,
    offset: usize,
    mut visit: impl FnMut(usize, Line<'a>),
) -> Result<usize, Error> {
    if rows == 0 || rows > 256 {
        return Err(Error::Invalid);
    }
    let mut lines = Lines::new(text, columns, offset)?;
    for row in 0..rows {
        if let Some(line) = lines.next() {
            visit(row, line);
        } else {
            break;
        }
    }
    Ok(lines.position())
}
