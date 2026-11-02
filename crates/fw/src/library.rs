//! The article library: a list of the articles in `/X3/DATA`,
//! newest first, and the reader on the chosen one. The reading position is
//! saved to the card when the article is left.
use crate::{Device, Text, home::Bar, page_loop, sd, serial::Out};
use core::fmt::Write;
use x3_core::{
    buttons::{Button, Debouncer, decode},
    fat::FatStore,
    library::{MAX_ITEMS, Progress, Shelf, truncate},
    reader::{FRAME_LEN, Frame, Style},
    receive::Index,
    screens,
    storage::{self, Catalog, FileKey, FileKind, RECORD_LEN, Record, Store},
};
use x3_proto::payload::ArticleHeader;

const TITLE_MAX: usize = 72;

struct Titles {
    bytes: [[u8; TITLE_MAX]; MAX_ITEMS],
    lens: [usize; MAX_ITEMS],
    text_len: [u32; MAX_ITEMS],
    progress: [Progress; MAX_ITEMS],
    /// Kind 1 in the header: an email or page summary, not a whole article.
    summary: [bool; MAX_ITEMS],
}

impl Titles {
    fn get(&self, i: usize) -> &str {
        core::str::from_utf8(&self.bytes[i][..self.lens[i]]).unwrap_or("?")
    }
}

fn read_exact<S: Store>(
    s: &mut S,
    key: FileKey,
    offset: usize,
    out: &mut [u8],
) -> Result<(), &'static str> {
    let mut done = 0;
    while done < out.len() {
        let n = s
            .read(key, offset + done, &mut out[done..])
            .map_err(|_| "read failed")?;
        if n == 0 {
            return Err("short read");
        }
        done += n;
    }
    Ok(())
}

fn header<S: Store>(s: &mut S, r: &Record) -> Result<ArticleHeader, &'static str> {
    let mut h = [0u8; 28];
    read_exact(s, r.payload_key(), 0, &mut h)?;
    ArticleHeader::decode(&h).map_err(|_| "bad article header")
}

/// Every metadata record on the card into the shelf, then each title.
fn scan<D, T>(
    store: &mut FatStore<'_, D, T>,
    shelf: &mut Shelf,
    titles: &mut Titles,
) -> Result<(), &'static str>
where
    D: embedded_sdmmc::BlockDevice,
    T: embedded_sdmmc::TimeSource,
{
    let mut after = 0;
    loop {
        let mut gens = [0u32; 16];
        let n = store
            .generations(after, &mut gens)
            .map_err(|_| "cannot list /X3/DATA")?;
        if n == 0 {
            break;
        }
        for &g in &gens[..n] {
            let key = FileKey {
                generation: g,
                kind: FileKind::Metadata,
            };
            let mut b = [0u8; RECORD_LEN];
            if read_exact(store, key, 0, &mut b).is_ok()
                && let Ok(r) = Record::decode(&b)
                && r.generation == g
            {
                shelf.add(r);
            }
        }
        after = gens[n - 1];
    }
    for (i, r) in shelf.articles().enumerate() {
        titles.lens[i] = 0;
        let Ok(h) = header(store, r) else { continue };
        titles.text_len[i] = h.text_len;
        titles.summary[i] = h.summary;
        titles.progress[i] = Progress::of(r.position, h.text_len);
        let mut t = [0u8; 256];
        let len = h.title_len as usize;
        if read_exact(store, r.payload_key(), 28, &mut t[..len]).is_err() {
            continue;
        }
        let shown = truncate(core::str::from_utf8(&t[..len]).unwrap_or("?"), TITLE_MAX);
        titles.bytes[i][..shown.len()].copy_from_slice(shown.as_bytes());
        titles.lens[i] = shown.len();
    }
    Ok(())
}

/// What Home shows about the library.
#[derive(Default)]
pub struct Summary {
    /// The card answered and `/X3/DATA` opened.
    pub card: bool,
    pub articles: usize,
    pub unread: usize,
    /// The article you were last reading: started, not finished, newest save.
    pub hero: Option<Hero>,
}

pub struct Hero {
    pub title: [u8; TITLE_MAX],
    pub len: usize,
    pub percent: u8,
}

impl Hero {
    pub fn title(&self) -> &str {
        core::str::from_utf8(&self.title[..self.len]).unwrap_or("")
    }
}

/// The article you were last reading: started, not finished, newest save.
fn last_read(shelf: &Shelf, titles: &Titles) -> Option<usize> {
    shelf
        .articles()
        .enumerate()
        .filter(|(i, _)| matches!(titles.progress[*i], Progress::Percent(_)))
        .max_by_key(|(_, r)| r.generation)
        .map(|(i, _)| i)
}

pub fn summary(dev: &mut Device<'_>) -> Summary {
    let mut shelf = Shelf::default();
    let mut titles = Titles {
        bytes: [[0; TITLE_MAX]; MAX_ITEMS],
        lens: [0; MAX_ITEMS],
        text_len: [0; MAX_ITEMS],
        progress: [Progress::New; MAX_ITEMS],
        summary: [false; MAX_ITEMS],
    };
    if sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |s| {
        scan(s, &mut shelf, &mut titles)
    })
    .is_err()
    {
        return Summary::default();
    }
    let mut out = Summary {
        card: true,
        articles: shelf.len(),
        ..Summary::default()
    };
    out.unread = (0..out.articles)
        .filter(|&i| titles.progress[i] == Progress::New)
        .count();
    if let Some(i) = last_read(&shelf, &titles)
        && let Progress::Percent(percent) = titles.progress[i]
    {
        let mut hero = Hero {
            title: [0; TITLE_MAX],
            len: titles.lens[i],
            percent,
        };
        hero.title[..hero.len].copy_from_slice(&titles.bytes[i][..hero.len]);
        out.hero = Some(hero);
    }
    out
}

/// Where the library opens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Open {
    /// The list.
    List,
    /// The article you were reading, straight into the page; Back then
    /// returns to Home instead of the list.
    Continue,
}

/// Verifies the whole payload and returns where its text lies. The reader
/// then reads the text from the card a window at a time (ADR 027).
fn locate<D, T>(store: &mut FatStore<'_, D, T>, r: &Record) -> Result<Text, &'static str>
where
    D: embedded_sdmmc::BlockDevice,
    T: embedded_sdmmc::TimeSource,
{
    storage::verify(store, r).map_err(|_| "article failed its checks")?;
    let h = header(store, r)?;
    if h.text_len as usize > x3_core::text::MAX_TEXT {
        return Err("article text too long");
    }
    Ok(Text::Card {
        key: r.payload_key(),
        start: 28 + h.title_len as usize + h.url_len as usize,
        len: h.text_len as usize,
    })
}

pub fn run(dev: &mut Device<'_>, out: &mut Out<'_>, style: &Style<'_>, bar: &Bar, open: Open) {
    let mut shelf = Shelf::default();
    let mut titles = Titles {
        bytes: [[0; TITLE_MAX]; MAX_ITEMS],
        lens: [0; MAX_ITEMS],
        text_len: [0; MAX_ITEMS],
        progress: [Progress::New; MAX_ITEMS],
        summary: [false; MAX_ITEMS],
    };
    if let Err(e) = sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |s| {
        scan(s, &mut shelf, &mut titles)
    }) {
        let _ = writeln!(out, "library: {e}");
    }
    let count = shelf.len();
    let _ = writeln!(out, "library: {count} articles");
    for i in 0..count {
        let kind = if titles.summary[i] { " (summary)" } else { "" };
        let _ = writeln!(
            out,
            "library:   {} [{}]{kind}",
            titles.get(i),
            titles.progress[i]
        );
    }
    out.flush();

    let mut frame_buf = [0u8; FRAME_LEN];
    let mut selected = 0usize;
    let mut auto_open = false;
    let mut from_home = false;
    if open == Open::Continue
        && let Some(i) = last_read(&shelf, &titles)
    {
        selected = i;
        auto_open = true;
    }
    let mut full = true;
    let mut debounce = Debouncer::default();
    loop {
        let choice = if core::mem::take(&mut auto_open) {
            from_home = true;
            Some(true)
        } else {
            let mut frame = Frame(&mut frame_buf);
            let status = bar.status();
            let mut names = [""; MAX_ITEMS];
            for (i, n) in names.iter_mut().enumerate().take(count) {
                *n = titles.get(i);
            }
            screens::library(
                &mut frame,
                style,
                &status,
                &names[..count],
                &titles.progress[..count],
                selected,
            );
            // The controller may have gone to deep sleep while idle.
            if !crate::ensure_epd(dev, out) {
                return;
            }
            crate::capture::send(out, "bw", &frame);
            let r = if full {
                dev.epd.change(&frame)
            } else {
                dev.epd.turn_clean(&frame)
            };
            if r.is_err() {
                let _ = writeln!(out, "library: display {r:?}");
                return;
            }
            full = false;
            loop {
                if out.read_byte().is_some() {
                    break None;
                }
                let s = dev.buttons.sample();
                let down = decode(s.bottom, s.side, s.power_pressed);
                if down.is_some() {
                    dev.power.activity();
                }
                let pressed = debounce.feed(down);
                if pressed.is_some() {
                    dev.power.activity();
                }
                match pressed {
                    Some(Button::Down | Button::Right) if selected + 1 < count => {
                        selected += 1;
                        break Some(false);
                    }
                    Some(Button::Up | Button::Left) if selected > 0 => {
                        selected -= 1;
                        break Some(false);
                    }
                    Some(Button::Confirm) if count > 0 => break Some(true),
                    Some(Button::Back) => break None,
                    _ => {}
                }
                dev.nap(out, 20);
            }
        };
        match choice {
            None => {
                let _ = writeln!(
                    out,
                    "library: done; stack {} untouched of {}",
                    crate::stack::untouched(),
                    crate::stack::size()
                );
                return;
            }
            Some(false) => continue,
            Some(true) => {}
        }
        // Open the selected article.
        let Some(record) = shelf.articles().nth(selected).cloned() else {
            continue;
        };
        let located = sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |s| locate(s, &record));
        let text = match located {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(out, "library: open {}: {e}", record.item());
                full = true;
                continue;
            }
        };
        let _ = writeln!(
            out,
            "library: open {} ({} bytes) at {}",
            record.item(),
            titles.text_len[selected],
            record.position
        );
        out.flush();
        let end = page_loop(
            dev,
            out,
            &mut frame_buf,
            &text,
            style,
            record.position as usize,
        );
        // Save the position as a new metadata generation.
        if let Some(end) = end
            && end as u32 != record.position
        {
            let saved = sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |s| {
                let g = s.next_generation().map_err(|_| "no free generation")?;
                storage::position(s, &record, g, end as u32).map_err(|_| "position not saved")
            });
            match saved {
                Ok(new) => {
                    let _ = writeln!(
                        out,
                        "library: position {end} saved as generation {}",
                        new.generation
                    );
                    titles.progress[selected] =
                        Progress::of(new.position, titles.text_len[selected]);
                    shelf.add(new);
                }
                Err(e) => {
                    let _ = writeln!(out, "library: {e}");
                }
            }
        }
        if from_home {
            return;
        }
        full = true;
    }
}

/// The `check` command: loads the card as boot does, verifies every live
/// item byte for byte, and counts the files on it. Run it after a power cut
/// or a reset in the middle of a Receive. Changes nothing.
pub fn check(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let counted = sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |s| {
        let index = Index::load(s).map_err(|_| "cannot load the index")?;
        let (mut items, mut ok, mut damaged) = (0usize, 0usize, 0usize);
        for r in index.iter() {
            items += 1;
            if storage::verify(s, r).is_ok() {
                ok += 1;
            } else {
                damaged += 1;
                let _ = writeln!(out, "check: DAMAGED {}", r.item());
            }
        }
        let count = |s: &mut FatStore<'_, _, _>, payloads: bool| -> Result<usize, &'static str> {
            let (mut n, mut after) = (0, 0);
            loop {
                let mut ids = [0u32; 32];
                let got = if payloads {
                    s.payload_generations(after, &mut ids)
                } else {
                    s.generations(after, &mut ids)
                }
                .map_err(|_| "cannot list /X3/DATA")?;
                if got == 0 {
                    return Ok(n);
                }
                n += got;
                after = ids[got - 1];
            }
        };
        let met = count(s, false)?;
        let dat = count(s, true)?;
        Ok((items, ok, damaged, met, dat))
    });
    match counted {
        Ok((items, ok, damaged, met, dat)) => {
            let _ = writeln!(
                out,
                "check: {items} items, {ok} verified, {damaged} damaged; {met} metadata and {dat} payload files on the card"
            );
        }
        Err(e) => {
            let _ = writeln!(out, "check: {e}");
        }
    }
}
