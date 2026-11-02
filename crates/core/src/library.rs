//! The article shelf: latest metadata record per item, newest first.
//!
//! Fed every metadata record found in the store, in any order. Keeps the
//! highest generation per item; a deleted record (tombstone) hides the item;
//! tokens are ignored. Bounded: at most `MAX_ITEMS` items, the newest kept.
use crate::storage::Record;

pub const MAX_ITEMS: usize = 32;

pub struct Shelf {
    items: [Option<Record>; MAX_ITEMS],
}

impl Default for Shelf {
    fn default() -> Self {
        Self {
            items: [const { None }; MAX_ITEMS],
        }
    }
}

impl Shelf {
    pub fn add(&mut self, r: Record) {
        if r.token {
            return;
        }
        if let Some(slot) = self
            .items
            .iter_mut()
            .flatten()
            .find(|old| old.item() == r.item())
        {
            if r.generation > slot.generation {
                *slot = r;
            }
            return;
        }
        if let Some(empty) = self.items.iter_mut().find(|s| s.is_none()) {
            *empty = Some(r);
            return;
        }
        // Full: replace the oldest item if this one is newer.
        if let Some(oldest) = self
            .items
            .iter_mut()
            .flatten()
            .min_by_key(|old| old.payload_generation)
            && r.payload_generation > oldest.payload_generation
        {
            *oldest = r;
        }
    }

    /// Readable articles, newest payload first.
    pub fn articles(&self) -> impl Iterator<Item = &Record> {
        let mut order: [Option<&Record>; MAX_ITEMS] = [None; MAX_ITEMS];
        let mut n = 0;
        for r in self.items.iter().flatten().filter(|r| !r.deleted) {
            order[n] = Some(r);
            n += 1;
        }
        order[..n].sort_unstable_by_key(|r| core::cmp::Reverse(r.map(|r| r.payload_generation)));
        order.into_iter().take(n).flatten()
    }

    pub fn len(&self) -> usize {
        self.articles().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Cuts `s` to at most `max` bytes on a character boundary, for list lines.
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// How much of an article has been read, for the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    New,
    Percent(u8),
    Read,
}

impl Progress {
    /// `position` and `text_len` are byte offsets into the article text. A
    /// position at the end is `Read`; anything else is 1–99 %, so a started
    /// article never looks new or finished.
    pub fn of(position: u32, text_len: u32) -> Self {
        if position == 0 {
            Self::New
        } else if position >= text_len {
            Self::Read
        } else {
            Self::Percent(((u64::from(position) * 100 / u64::from(text_len)) as u8).clamp(1, 99))
        }
    }
}

impl core::fmt::Display for Progress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::New => f.write_str("new"),
            Self::Percent(p) => write!(f, "{p}%"),
            Self::Read => f.write_str("Read"),
        }
    }
}

/// A picture's name from its item ID, written into `out`: a token
/// (`t-goblin-warrior-0123456789ab` gives "Goblin warrior") or an image
/// (`i-harbour-at-dusk-0123456789ab`, ADR 028). IDs without a name give
/// "Token" (`t-…`) or "Picture" (anything else).
pub fn token_name<'o>(item: &str, out: &'o mut [u8; 48]) -> &'o str {
    let none = if item.starts_with("t-") {
        "Token"
    } else {
        "Picture"
    };
    let slug = item
        .strip_prefix("t-")
        .or_else(|| item.strip_prefix("i-"))
        .and_then(|s| s.rsplit_once('-'))
        .filter(|(name, tail)| {
            tail.len() == 12
                && tail.bytes().all(|b| b.is_ascii_hexdigit())
                && !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        .map(|(name, _)| name);
    let Some(slug) = slug else {
        out[..none.len()].copy_from_slice(none.as_bytes());
        return core::str::from_utf8(&out[..none.len()]).unwrap_or("Picture");
    };
    let n = slug.len().min(out.len());
    for (i, b) in slug.bytes().take(n).enumerate() {
        out[i] = match b {
            b'-' => b' ',
            _ if i == 0 => b.to_ascii_uppercase(),
            _ => b,
        };
    }
    core::str::from_utf8(&out[..n]).unwrap_or("Picture")
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn progress_labels() {
        assert_eq!(Progress::of(0, 500), Progress::New);
        assert_eq!(Progress::of(0, 0), Progress::New);
        assert_eq!(Progress::of(1, 100_000), Progress::Percent(1));
        assert_eq!(Progress::of(250, 500), Progress::Percent(50));
        assert_eq!(Progress::of(499, 500), Progress::Percent(99));
        assert_eq!(Progress::of(500, 500), Progress::Read);
        assert_eq!(Progress::of(900, 500), Progress::Read);
        assert_eq!(Progress::of(250, 500).to_string(), "50%");
        assert_eq!(Progress::New.to_string(), "new");
        assert_eq!(Progress::Read.to_string(), "Read");
    }
}
