//! What each device screen says, built from plain data (audit F1). The
//! firmware reads the card, the clock and the gauge, fills these structs and
//! calls the function; the simulator calls the same function with sample
//! data. So captions, labels and wording live in one place, host-tested, and
//! the review PNGs match the device. Drawing primitives stay in `gui`.
use crate::{
    gui::{self, Hero, Icon, Keys, Row, Status, Tile},
    library::{MAX_ITEMS, Progress, token_name},
    reader::{Frame, Style},
    rtc::DateTime,
};
use core::fmt::Write;
use x3_proto::config::MAX_NETWORKS;

/// A fixed text buffer. Writing past the end keeps what fits, cut at a
/// character boundary, so a long value is shortened, never lost entirely.
#[derive(Clone, Copy)]
pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> Write for Text<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        append(&mut self.bytes, &mut self.len, s)
    }
}

/// One copy for every buffer size, so the firmware carries it once.
#[inline(never)]
fn append(bytes: &mut [u8], len: &mut usize, s: &str) -> core::fmt::Result {
    let room = bytes.len() - *len;
    let mut take = s.len().min(room);
    while !s.is_char_boundary(take) {
        take -= 1;
    }
    bytes[*len..*len + take].copy_from_slice(&s.as_bytes()[..take]);
    *len += take;
    if take < s.len() {
        Err(core::fmt::Error)
    } else {
        Ok(())
    }
}

impl<const N: usize> Text<N> {
    pub fn as_str(&self) -> &str {
        // Only whole characters are ever copied in.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

/// Back and Confirm only: a question or a result page.
pub const BACK_CONFIRM: Keys = Keys {
    back: true,
    confirm: true,
    ..Keys::NONE
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn articles(n: usize) -> Text<24> {
    let mut t = Text::default();
    let _ = match n {
        1 => write!(t, "1 article"),
        n => write!(t, "{n} articles"),
    };
    t
}

// ---------------------------------------------------------------- Home

/// The article you were last reading.
#[derive(Debug, Clone, Copy)]
pub struct HomeHero<'a> {
    pub title: &'a str,
    pub percent: u8,
}

/// Everything Home shows.
#[derive(Debug, Clone, Copy, Default)]
pub struct HomeInfo<'a> {
    /// The card answered.
    pub card: bool,
    pub articles: usize,
    pub pictures: usize,
    /// The last good Receive, local time.
    pub last_receive: Option<DateTime>,
    /// Today's date, local time, if the clock is set.
    pub today: Option<(u16, u8, u8)>,
    pub battery: u8,
    pub hero: Option<HomeHero<'a>>,
}

/// The Receive tile's caption: today's time, or the day of the last one.
fn receive_caption(last: Option<DateTime>, today: Option<(u16, u8, u8)>) -> Text<24> {
    let mut t = Text::default();
    let _ = match (last, today) {
        (Some(l), Some(d)) if (l.year, l.month, l.day) == d => {
            write!(t, "Today {:02}:{:02}", l.hour, l.minute)
        }
        (Some(l), _) => write!(
            t,
            "Last {} {}",
            l.day,
            MONTHS[usize::from(l.month.clamp(1, 12)) - 1]
        ),
        (None, _) => write!(t, "Get new items"),
    };
    t
}

/// Home: the card for the article you were reading and four tiles.
/// `selected` 0 is the card, 1–4 the tiles.
pub fn home(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    info: &HomeInfo<'_>,
    selected: usize,
) {
    let lib = if info.card {
        match info.articles {
            0 => str_text("No articles yet"),
            n => articles(n),
        }
    } else {
        str_text("No card")
    };
    let mut pics = Text::<24>::default();
    let _ = match info.pictures {
        _ if !info.card => write!(pics, "No card"),
        0 => write!(pics, "None yet"),
        n => write!(pics, "{n} saved"),
    };
    let rx = receive_caption(info.last_receive, info.today);
    let mut battery = Text::<24>::default();
    let _ = write!(battery, "Battery {}%", info.battery);
    let tiles = [
        Tile {
            icon: Icon::Book,
            label: "Library",
            caption: lib.as_str(),
        },
        Tile {
            icon: Icon::Token,
            label: "Pictures",
            caption: pics.as_str(),
        },
        Tile {
            icon: Icon::Receive,
            label: "Receive",
            caption: rx.as_str(),
        },
        Tile {
            icon: Icon::Device,
            label: "Device",
            caption: battery.as_str(),
        },
    ];
    let mut read = Text::<24>::default();
    let hero = info.hero.map(|h| {
        let _ = write!(read, "{}% read", h.percent);
        Hero {
            title: h.title,
            caption: read.as_str(),
            percent: h.percent,
        }
    });
    gui::home(
        frame,
        style,
        st,
        hero.as_ref(),
        &tiles,
        selected,
        Keys::HOME,
    );
}

fn str_text<const N: usize>(s: &str) -> Text<N> {
    let mut t = Text::default();
    let _ = t.write_str(s);
    t
}

// ---------------------------------------------------------------- Lists

/// An empty list offers only Back: the other keys would do nothing.
fn list_keys(rows: usize) -> Keys {
    if rows == 0 { Keys::BACK } else { Keys::ALL }
}

/// The Library list: one row per article, newest first. `titles` and
/// `progress` have the same length (at most `MAX_ITEMS`).
pub fn library(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    titles: &[&str],
    progress: &[Progress],
    selected: usize,
) {
    let n = titles.len().min(progress.len()).min(MAX_ITEMS);
    let mut labels = [Text::<8>::default(); MAX_ITEMS];
    for (l, p) in labels.iter_mut().zip(&progress[..n]) {
        let _ = match p {
            Progress::New => write!(l, "New"),
            p => write!(l, "{p}"),
        };
    }
    let mut rows = [Row::default(); MAX_ITEMS];
    for i in 0..n {
        let p = progress[i];
        rows[i] = Row {
            title: titles[i],
            right: labels[i].as_str(),
            bar: match p {
                Progress::Percent(n) => Some(n),
                _ => None,
            },
            pill: p == Progress::New,
            strong: p == Progress::New,
            caption: "",
        };
    }
    gui::list(
        frame,
        style,
        st,
        "Library",
        &rows[..n],
        selected,
        "No articles on the card yet.\n\nSend one from your phone, then choose Receive on Home.",
        list_keys(n),
    );
}

/// Most pictures the list holds (the token cache keeps the last 20).
pub const MAX_PICTURES: usize = 20;

/// The Pictures list, newest first, named from the item IDs.
pub fn pictures(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    items: &[&str],
    selected: usize,
) {
    let n = items.len().min(MAX_PICTURES);
    let mut names = [[0u8; 48]; MAX_PICTURES];
    let mut lens = [0usize; MAX_PICTURES];
    for i in 0..n {
        let mut b = [0u8; 48];
        let s = token_name(items[i], &mut b);
        lens[i] = s.len();
        names[i][..s.len()].copy_from_slice(s.as_bytes());
    }
    let mut rows = [Row::default(); MAX_PICTURES];
    for i in 0..n {
        rows[i].title = core::str::from_utf8(&names[i][..lens[i]]).unwrap_or("Picture");
        rows[i].right = if i == 0 { "Newest" } else { "" };
    }
    gui::list(
        frame,
        style,
        st,
        "Pictures",
        &rows[..n],
        selected,
        "No pictures on the card yet.\n\nSend a token or an image from the web page or a Shortcut, then choose Receive on Home.",
        list_keys(n),
    );
}

/// Which network Receive uses, for the Device menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSummary<'a> {
    /// One network is chosen to try first.
    Chosen(&'a str),
    /// None chosen: this many saved, the strongest used.
    Automatic(usize),
}

/// Everything the Device menu shows.
#[derive(Debug, Clone, Copy)]
pub struct DeviceInfo<'a> {
    pub wifi: WifiSummary<'a>,
    pub articles: usize,
    pub pictures: usize,
    pub firmware: &'a str,
    pub battery: u8,
}

/// Rows on the Device menu, in order: Wi-Fi, Storage, Clean screen, About,
/// Back to CrossPoint.
pub const DEVICE_ROWS: usize = 5;

/// The Device menu.
pub fn device(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    info: &DeviceInfo<'_>,
    selected: usize,
) {
    let mut wifi = Text::<64>::default();
    let _ = match info.wifi {
        WifiSummary::Automatic(0) => write!(wifi, "No network saved: add one here"),
        WifiSummary::Chosen(name) => write!(wifi, "Uses {name} first"),
        WifiSummary::Automatic(1) => write!(wifi, "1 network saved"),
        WifiSummary::Automatic(n) => write!(wifi, "Automatic: strongest of {n} saved"),
    };
    let mut storage = Text::<48>::default();
    let _ = write!(
        storage,
        "{} \u{b7} {} pictures",
        articles(info.articles).as_str(),
        info.pictures
    );
    let mut about = Text::<48>::default();
    let _ = write!(
        about,
        "Firmware {} \u{b7} Battery {}%",
        info.firmware, info.battery
    );
    let rows: [Row<'_>; DEVICE_ROWS] = [
        Row {
            title: "Wi-Fi",
            caption: wifi.as_str(),
            ..Row::default()
        },
        Row {
            title: "Storage",
            caption: storage.as_str(),
            ..Row::default()
        },
        Row {
            title: "Clean screen",
            caption: "Clears ghosting \u{b7} flashes for 20 s",
            ..Row::default()
        },
        Row {
            title: "About this device",
            caption: about.as_str(),
            ..Row::default()
        },
        Row {
            title: "Back to CrossPoint",
            caption: "To come back you need a computer",
            ..Row::default()
        },
    ];
    gui::list(frame, style, st, "Device", &rows, selected, "", Keys::ALL);
}

/// Rows on the Wi-Fi screen for `saved` networks: Automatic, each network,
/// Add a network.
pub fn wifi_rows(saved: usize) -> usize {
    saved.min(MAX_NETWORKS) + 2
}

/// The Wi-Fi screen. `chosen` is the index into `names` Receive tries first,
/// `None` for automatic.
pub fn wifi(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    names: &[&str],
    chosen: Option<usize>,
    selected: usize,
) {
    let n = names.len().min(MAX_NETWORKS);
    let mut rows = [Row::default(); MAX_NETWORKS + 2];
    rows[0] = Row {
        title: "Automatic",
        caption: "The strongest saved network",
        right: if chosen.is_none() { "Chosen" } else { "" },
        ..Row::default()
    };
    for (i, name) in names[..n].iter().enumerate() {
        let in_use = chosen == Some(i);
        rows[i + 1] = Row {
            title: name,
            caption: if in_use {
                "Tried first, then the others"
            } else {
                "Press Confirm to use it first"
            },
            right: if in_use { "Chosen" } else { "" },
            ..Row::default()
        };
    }
    rows[n + 1] = Row {
        title: "Add a network",
        caption: "Shows a QR code to scan with your phone",
        ..Row::default()
    };
    gui::list(
        frame,
        style,
        st,
        "Wi-Fi",
        &rows[..n + 2],
        selected,
        "",
        Keys::ALL,
    );
}

// ---------------------------------------------------------------- Pages

/// Storage: what is on the card, and Delete everything.
pub fn storage(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    articles: usize,
    pictures: usize,
) {
    let a: Text<12> = number(articles);
    let p: Text<12> = number(pictures);
    let items = [("Articles", a.as_str()), ("Pictures", p.as_str())];
    gui::pairs_action(
        frame,
        style,
        st,
        "Storage",
        &items,
        "Delete everything",
        BACK_CONFIRM,
    );
}

fn number<const N: usize>(n: usize) -> Text<N> {
    let mut t = Text::default();
    let _ = write!(t, "{n}");
    t
}

/// The question before Delete everything.
pub fn delete_everything(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    articles: usize,
    pictures: usize,
) {
    let mut body = Text::<320>::default();
    let _ = write!(
        body,
        "This deletes all {articles} articles and {pictures} pictures on the card, and your reading places.\n\nSaved Wi-Fi and the server settings stay. The companion does not send them again.\n\nConfirm deletes. Back cancels."
    );
    gui::message(
        frame,
        style,
        st,
        "Delete everything?",
        body.as_str(),
        BACK_CONFIRM,
    );
}

/// Everything About this device shows.
#[derive(Debug, Clone, Copy)]
pub struct AboutInfo<'a> {
    pub firmware: &'a str,
    pub battery: Option<u8>,
    pub millivolts: Option<u16>,
    /// Local time, `None` if the clock is not set.
    pub clock: Option<DateTime>,
    pub articles: usize,
    pub pictures: usize,
    /// Stack never touched since boot, in bytes.
    pub stack_free: usize,
}

/// About this device.
pub fn about(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>, info: &AboutInfo<'_>) {
    let mut battery = Text::<32>::default();
    let _ = match (info.battery, info.millivolts) {
        (Some(p), Some(mv)) => write!(battery, "{p}% ({mv} mV)"),
        (Some(p), None) => write!(battery, "{p}%"),
        _ => write!(battery, "unknown"),
    };
    let mut clock = Text::<32>::default();
    let _ = match info.clock {
        Some(t) => write!(
            clock,
            "{:04}-{:02}-{:02} {:02}:{:02}",
            t.year, t.month, t.day, t.hour, t.minute
        ),
        None => write!(clock, "not set"),
    };
    let mut stored = Text::<48>::default();
    let _ = write!(
        stored,
        "{} \u{b7} {} pictures",
        articles(info.articles).as_str(),
        info.pictures
    );
    let mut headroom = Text::<16>::default();
    let _ = write!(headroom, "{} KB", info.stack_free / 1024);
    let items = [
        ("Firmware", info.firmware),
        ("Battery", battery.as_str()),
        ("Clock", clock.as_str()),
        ("Stored", stored.as_str()),
        ("Stack free", headroom.as_str()),
    ];
    gui::pairs(frame, style, st, "About this device", &items, Keys::BACK);
}

/// The question before Back to CrossPoint (ADR 017).
pub fn confirm_crosspoint(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>) {
    gui::message(
        frame,
        style,
        st,
        "Go back to CrossPoint?",
        "CrossPoint starts at once.\n\nTo return to this firmware you need a computer and the install guide.\n\nConfirm switches. Back cancels.",
        BACK_CONFIRM,
    );
}

/// Home could not open the library or the pictures: no card.
pub fn no_card(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>) {
    gui::message(
        frame,
        style,
        st,
        "No card",
        "Insert the microSD card. Your articles and pictures are on it.\n\nPress any key.",
        Keys::BACK,
    );
}

/// After Confirm on an open picture.
pub fn sleep_picture_set(frame: &mut Frame<'_>, style: &Style<'_>, st: &Status<'_>, ok: bool) {
    let (title, body) = if ok {
        (
            "Sleep screen set",
            "The X3 shows this picture while it sleeps.\n\nPress any key.",
        )
    } else {
        (
            "Could not save",
            "Check the card, then try again.\n\nPress any key.",
        )
    };
    gui::message(frame, style, st, title, body, Keys::BACK);
}

/// What a finished Receive reports.
#[derive(Debug, Clone, Copy)]
pub struct ReceiveDone<'a> {
    pub new: usize,
    pub deleted: usize,
    pub rejected: usize,
    /// Tokens and articles removed to make room.
    pub made_room: usize,
    /// The Wi-Fi network that joined.
    pub network: &'a str,
    /// How the companion was reached.
    pub route: &'a str,
    pub took: &'a str,
    pub charge_used: Option<&'a str>,
    pub battery: &'a str,
}

/// The page after a good Receive. Rows with nothing to say are left out.
pub fn receive_done(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    r: &ReceiveDone<'_>,
) {
    let new: Text<12> = number(r.new);
    let deleted: Text<12> = number(r.deleted);
    let room: Text<12> = number(r.made_room);
    let rejected: Text<12> = number(r.rejected);
    let mut items = [("", ""); 9];
    let mut n = 0;
    let mut push = |k: &'static str, v| {
        items[n] = (k, v);
        n += 1;
    };
    push("New", new.as_str());
    push("Deleted", deleted.as_str());
    if r.made_room > 0 {
        push("Removed for room", room.as_str());
    }
    if r.rejected > 0 {
        push("Rejected", rejected.as_str());
    }
    push("Wi-Fi", r.network);
    push("Route", r.route);
    push("Took", r.took);
    if let Some(used) = r.charge_used {
        push("Charge used", used);
    }
    push("Battery", r.battery);
    gui::pairs(
        frame,
        style,
        st,
        "Receive complete",
        &items[..n],
        BACK_CONFIRM,
    );
}

/// The page after a failed Receive: the reason, then what to check.
pub fn receive_failed(
    frame: &mut Frame<'_>,
    style: &Style<'_>,
    st: &Status<'_>,
    reason: &str,
    network: &str,
) {
    let mut body = Text::<512>::default();
    let _ = write!(
        body,
        "{reason}\n\nWi-Fi: {network}\n\nCheck the Wi-Fi and the home server, then try again."
    );
    gui::message(
        frame,
        style,
        st,
        "Receive failed",
        body.as_str(),
        BACK_CONFIRM,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_whole_characters() {
        let mut t = Text::<5>::default();
        assert!(write!(t, "ab\u{17c}\u{17c}").is_err());
        assert_eq!(t.as_str(), "ab\u{17c}");
    }

    #[test]
    fn receive_caption_today_and_earlier() {
        let at = DateTime {
            year: 2026,
            month: 11,
            day: 2,
            hour: 9,
            minute: 5,
            second: 0,
        };
        assert_eq!(
            receive_caption(Some(at), Some((2026, 11, 2))).as_str(),
            "Today 09:05"
        );
        assert_eq!(
            receive_caption(Some(at), Some((2026, 11, 3))).as_str(),
            "Last 2 Nov"
        );
        assert_eq!(receive_caption(Some(at), None).as_str(), "Last 2 Nov");
        assert_eq!(receive_caption(None, None).as_str(), "Get new items");
    }

    #[test]
    fn wifi_rows_capped() {
        assert_eq!(wifi_rows(0), 2);
        assert_eq!(wifi_rows(MAX_NETWORKS + 3), MAX_NETWORKS + 2);
    }
}
