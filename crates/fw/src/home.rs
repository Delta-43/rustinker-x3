//! The Home screen (Phase 6B): what the device shows when it is idle. A card
//! for the article you were reading, four tiles (Library, Tokens, Receive,
//! Device), moved with the buttons. It owns the screen between the other
//! modes: each one returns here when you press Back.
use crate::{
    Device, library, sd,
    serial::Out,
    {receive, rtc, stack, tokens},
};
use core::fmt::Write;
use x3_core::{
    buttons::Button,
    gui::{self, Keys, Status},
    reader::{FRAME_LEN, Frame},
    screens::{self, AboutInfo, HomeHero, HomeInfo},
};

/// The clock and the battery, for the status bar on every screen.
#[derive(Clone, Copy)]
pub struct Bar {
    clock: [u8; 5],
    battery: u8,
}

impl Bar {
    /// Battery charge in per cent, as the status bar shows it.
    pub fn battery(&self) -> u8 {
        self.battery
    }

    pub fn read(dev: &mut Device<'_>) -> Self {
        let mut bar = Self {
            clock: *b"--:--",
            battery: 0,
        };
        if let Some(i2c) = dev.i2c.as_mut() {
            if let Some(t) = rtc::read(i2c).and_then(|t| t.central_europe()) {
                bar.clock = [
                    b'0' + t.hour / 10,
                    b'0' + t.hour % 10,
                    b':',
                    b'0' + t.minute / 10,
                    b'0' + t.minute % 10,
                ];
            }
            bar.battery = rtc::gauge(i2c).percent.unwrap_or(0);
        }
        bar
    }

    pub fn status(&self) -> Status<'_> {
        Status {
            clock: core::str::from_utf8(&self.clock).unwrap_or("--:--"),
            battery: self.battery,
            wifi: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Nothing,
    /// Redraw Home now, full refresh.
    Now,
    /// A result is on screen; the next key press brings Home back.
    OnKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Continue,
    Library,
    Tokens,
    Receive,
    Device,
}

pub struct Home {
    selected: usize,
    pending: Pending,
    bar: Bar,
    summary: library::Summary,
    tokens: usize,
    /// The last good Receive and today's date, local time, for the tile.
    last_receive: Option<x3_core::rtc::DateTime>,
    today: Option<(u16, u8, u8)>,
    /// The card's size at the last check, `None` if the slot was empty.
    card: Option<u64>,
    /// When the card was last checked.
    checked: embassy_time::Instant,
}

/// How often Home checks the card while nothing is pressed.
const CARD_CHECK: embassy_time::Duration = embassy_time::Duration::from_secs(15);

impl Home {
    pub fn new() -> Self {
        Self {
            selected: 0,
            pending: Pending::Now,
            bar: Bar {
                clock: *b"--:--",
                battery: 0,
            },
            summary: library::Summary::default(),
            tokens: 0,
            last_receive: None,
            today: None,
            card: None,
            checked: embassy_time::Instant::now(),
        }
    }

    /// The X3 has no card-detect pin: ask the card on a key press and every
    /// `CARD_CHECK` while idle. A card taken out, put back or swapped makes
    /// Home read it again. Returns whether it changed.
    fn card_changed(&mut self, dev: &mut Device<'_>, out: &mut Out<'_>, pressed: bool) -> bool {
        if self.pending != Pending::Nothing || !(pressed || self.checked.elapsed() >= CARD_CHECK) {
            return false;
        }
        self.checked = embassy_time::Instant::now();
        let t = embassy_time::Instant::now();
        let now = sd::present(dev.epd.bus(), &mut dev.sd_cs);
        if now == self.card {
            return false;
        }
        let _ = writeln!(
            out,
            "home: card {} ({} ms)",
            if now.is_some() { "in" } else { "out" },
            t.elapsed().as_millis()
        );
        self.card = now;
        true
    }

    /// Something else has used the screen (a serial command); the next key
    /// press brings Home back.
    pub fn invalidate(&mut self) {
        self.pending = Pending::OnKey;
    }

    /// Called every pass of the main loop with this pass's key press.
    pub async fn tick(&mut self, dev: &mut Device<'_>, out: &mut Out<'_>, pressed: Option<Button>) {
        if self.card_changed(dev, out, pressed.is_some()) {
            // Redraw from the card as it is now; the key only woke us.
            self.pending = Pending::Now;
        }
        match self.pending {
            Pending::Now => {
                self.refresh(dev);
                self.draw(dev, out, true);
                self.pending = Pending::Nothing;
            }
            Pending::OnKey if pressed.is_some() => self.pending = Pending::Now,
            _ => {}
        }
        let Some(key) = pressed else { return };
        if self.pending != Pending::Nothing {
            return;
        }
        match key {
            Button::Up | Button::Left if self.selected > 0 => {
                self.selected -= 1;
                self.draw(dev, out, false);
            }
            Button::Down | Button::Right if self.selected < 4 => {
                self.selected += 1;
                self.draw(dev, out, false);
            }
            // Nothing is above Home, so Back clears the glass: a full refresh.
            Button::Back => {
                dev.epd.force_full();
                self.draw(dev, out, true);
            }
            // A short press of Power sleeps now (the sleep screen, then deep
            // sleep; Power wakes).
            Button::Power => crate::sleep_now(dev, out),
            Button::Confirm => {
                let action = match self.selected {
                    0 if self.summary.hero.is_some() => Action::Continue,
                    0 | 1 => Action::Library,
                    2 => Action::Tokens,
                    3 => Action::Receive,
                    _ => Action::Device,
                };
                self.open(dev, out, action).await;
            }
            _ => {}
        }
    }

    fn refresh(&mut self, dev: &mut Device<'_>) {
        self.card = sd::present(dev.epd.bus(), &mut dev.sd_cs);
        self.checked = embassy_time::Instant::now();
        self.bar = Bar::read(dev);
        self.last_receive = receive::last_receive().and_then(|t| t.central_europe());
        self.today = dev
            .i2c
            .as_mut()
            .and_then(rtc::read)
            .and_then(|t| t.central_europe())
            .map(|t| (t.year, t.month, t.day));
        self.summary = library::summary(dev);
        self.tokens = tokens::count(dev);
    }

    async fn open(&mut self, dev: &mut Device<'_>, out: &mut Out<'_>, action: Action) {
        let name = match action {
            Action::Continue => "continue",
            Action::Library => "library",
            Action::Tokens => "tokens",
            Action::Receive => "receive",
            Action::Device => "device",
        };
        let _ = writeln!(out, "home: open {name}");
        self.open_inner(dev, out, action).await;
        let _ = writeln!(
            out,
            "home: back from {name}; stack {} untouched of {}",
            stack::untouched(),
            stack::size()
        );
    }

    async fn open_inner(&mut self, dev: &mut Device<'_>, out: &mut Out<'_>, action: Action) {
        self.pending = Pending::Now;
        if !self.summary.card
            && matches!(action, Action::Continue | Action::Library | Action::Tokens)
        {
            draw(dev, out, true, |f, style| {
                screens::no_card(f, style, &self.bar.status());
            });
            crate::menu::wait_key(dev, out);
            return;
        }
        match action {
            Action::Continue | Action::Library => {
                let Some(style) = crate::reader_style() else {
                    let _ = writeln!(out, "home: reader fonts are missing");
                    return;
                };
                let open = if action == Action::Continue {
                    library::Open::Continue
                } else {
                    library::Open::List
                };
                library::run(dev, out, &style, &self.bar, open);
            }
            Action::Tokens => {
                let Some(style) = crate::reader_style() else {
                    let _ = writeln!(out, "home: reader fonts are missing");
                    return;
                };
                tokens::screen(dev, out, &style, &self.bar);
            }
            Action::Receive => {
                message(
                    dev,
                    out,
                    &self.bar,
                    "Receive",
                    "Connecting to the home server\u{2026}",
                    Keys::NONE,
                );
                receive::receive(dev, out).await;
                // The summary or the token it shows stays until a key press.
                self.pending = Pending::OnKey;
            }
            Action::Device => crate::menu::device(dev, out, &self.bar).await,
        }
    }

    fn draw(&self, dev: &mut Device<'_>, out: &mut Out<'_>, full: bool) {
        let Some(style) = crate::reader_style() else {
            return;
        };
        let info = HomeInfo {
            card: self.summary.card,
            articles: self.summary.articles,
            pictures: self.tokens,
            last_receive: self.last_receive,
            today: self.today,
            battery: self.bar.battery,
            hero: self.summary.hero.as_ref().map(|h| HomeHero {
                title: h.title(),
                percent: h.percent,
            }),
        };
        let mut buf = [0u8; FRAME_LEN];
        let mut frame = Frame(&mut buf);
        screens::home(&mut frame, &style, &self.bar.status(), &info, self.selected);
        show(dev, out, &frame, full);
    }
}

/// Puts a finished frame on the glass: a full refresh when the picture
/// changes completely, a fast one when only the selection moved.
pub(crate) fn show(dev: &mut Device<'_>, out: &mut Out<'_>, frame: &Frame<'_>, full: bool) {
    if !crate::ensure_epd(dev, out) {
        return;
    }
    let r = if full {
        dev.epd.change(frame)
    } else {
        dev.epd.turn_clean(frame)
    };
    if let Err(e) = r {
        let _ = writeln!(out, "home: display {e:?}");
    }
}

/// Draws one screen into a frame on the stack and puts it on the glass. A
/// plain function, so the 52 KB frame is never held across an await (F6).
pub(crate) fn draw(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    full: bool,
    paint: impl FnOnce(&mut Frame<'_>, &x3_core::reader::Style<'_>),
) {
    let Some(style) = crate::reader_style() else {
        return;
    };
    let mut buf = [0u8; FRAME_LEN];
    let mut frame = Frame(&mut buf);
    paint(&mut frame, &style);
    show(dev, out, &frame, full);
}

/// A message under a title, shown at once.
pub(crate) fn message(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    bar: &Bar,
    title: &str,
    body: &str,
    keys: Keys,
) {
    let Some(style) = crate::reader_style() else {
        return;
    };
    let mut buf = [0u8; FRAME_LEN];
    let mut frame = Frame(&mut buf);
    gui::message(&mut frame, &style, &bar.status(), title, body, keys);
    show(dev, out, &frame, true);
}

/// About this device: facts about this unit. Any key returns.
pub(crate) fn about(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &Bar) {
    let summary = library::summary(dev);
    let tokens = tokens::count(dev);
    let (gauge, clock) = match dev.i2c.as_mut() {
        Some(i2c) => (
            Some(rtc::gauge(i2c)),
            rtc::read(i2c).and_then(|t| t.central_europe()),
        ),
        None => (None, None),
    };
    let info = AboutInfo {
        firmware: env!("CARGO_PKG_VERSION"),
        battery: gauge.as_ref().and_then(|g| g.percent),
        millivolts: gauge.as_ref().and_then(|g| g.millivolts),
        clock,
        articles: summary.articles,
        pictures: tokens,
        stack_free: stack::untouched(),
    };
    draw(dev, out, true, |f, style| {
        screens::about(f, style, &bar.status(), &info);
    });
    crate::menu::wait_key(dev, out);
}

/// Asks before leaving for CrossPoint: from there only a computer brings this
/// firmware back (ADR 017). Confirm says yes, Back says no.
pub(crate) fn confirm_crosspoint(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &Bar) -> bool {
    draw(dev, out, true, |f, style| {
        screens::confirm_crosspoint(f, style, &bar.status());
    });
    loop {
        match crate::menu::next_key(dev, out) {
            Some(Button::Confirm) => return true,
            None | Some(Button::Back) => return false,
            _ => {}
        }
    }
}
