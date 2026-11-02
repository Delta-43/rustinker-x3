//! The Device menu and the screens under it: Wi-Fi (choose the network
//! Receive tries first, or add one), Storage (what is on the card, delete
//! everything), About, and Back to CrossPoint. Each screen draws in a plain
//! function, so no 52 KB frame is held across an await.
extern crate alloc;

use crate::{Device, home, library, sd, serial::Out, tokens, wifi};
use alloc::{format, string::String, vec::Vec};
use core::fmt::Write;
use x3_core::{
    buttons::{Button, Debouncer, decode},
    gui::Keys,
    screens::{self, DeviceInfo, WifiSummary},
};

/// Waits for the next key press. `None` if a serial command arrived first:
/// the caller leaves, so the command loop can run it.
pub(crate) fn next_key(dev: &mut Device<'_>, out: &mut Out<'_>) -> Option<Button> {
    let mut debounce = Debouncer::default();
    loop {
        if out.read_byte().is_some() {
            return None;
        }
        let s = dev.buttons.sample();
        let down = decode(s.bottom, s.side, s.power_pressed);
        if down.is_some() {
            dev.power.activity();
        }
        if let Some(b) = debounce.feed(down) {
            return Some(b);
        }
        dev.nap(out, 20);
    }
}

/// Waits for any key (or a serial command).
pub(crate) fn wait_key(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let _ = next_key(dev, out);
}

/// Moves the selection for list screens. Returns whether it moved.
fn step(selected: &mut usize, len: usize, key: Button) -> bool {
    match key {
        Button::Down | Button::Right if *selected + 1 < len => *selected += 1,
        Button::Up | Button::Left if *selected > 0 => *selected -= 1,
        _ => return false,
    }
    true
}

/// The Device menu. Back returns to Home.
pub async fn device(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &home::Bar) {
    let mut selected = 0usize;
    let mut full = true;
    loop {
        let mut pin = [0u8; 64];
        let (saved, chosen) = wifi_summary(dev, &mut pin);
        let summary = library::summary(dev);
        let info = DeviceInfo {
            wifi: match chosen {
                Some(name) => WifiSummary::Chosen(name),
                None => WifiSummary::Automatic(saved),
            },
            articles: summary.articles,
            pictures: tokens::count(dev),
            firmware: env!("CARGO_PKG_VERSION"),
            battery: bar.battery(),
        };
        home::draw(dev, out, full, |f, style| {
            screens::device(f, style, &bar.status(), &info, selected);
        });
        full = false;
        let Some(key) = next_key(dev, out) else {
            return;
        };
        if step(&mut selected, screens::DEVICE_ROWS, key) {
            continue;
        }
        match key {
            Button::Back => return,
            Button::Confirm => {
                match selected {
                    0 => wifi_screen(dev, out, bar).await,
                    1 => storage(dev, out, bar),
                    2 => {
                        if crate::ensure_epd(dev, out) {
                            let r = dev.epd.clean(6);
                            let _ = writeln!(out, "clean: {r:?}");
                        }
                    }
                    3 => home::about(dev, out, bar),
                    _ => {
                        if home::confirm_crosspoint(dev, out, bar) {
                            crate::crosspoint(dev, out);
                        }
                    }
                }
                full = true;
            }
            _ => {}
        }
    }
}

/// For the Device menu: how many networks are saved, and the one chosen to
/// try first (read into `pin`).
fn wifi_summary<'p>(dev: &mut Device<'_>, pin: &'p mut [u8; 64]) -> (usize, Option<&'p str>) {
    let mut buf = [0u8; 512];
    let count = wifi::config(dev, &mut buf).map_or(0, |l| l.len());
    let chosen = wifi::chosen(dev, pin);
    (count, if count == 0 { None } else { chosen })
}

/// The Wi-Fi screen: Automatic, each saved network, Add a network. Confirm on
/// a network makes Receive try it first; the others stay as a fallback.
async fn wifi_screen(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &home::Bar) {
    let mut selected = 0usize;
    let mut full = true;
    loop {
        // Names only: passwords never leave `wifi::config`'s buffer.
        let names: Vec<String> = {
            let mut buf = [0u8; 512];
            wifi::config(dev, &mut buf)
                .map(|l| l.iter().map(|w| String::from(w.ssid)).collect())
                .unwrap_or_default()
        };
        let chosen: Option<String> = {
            let mut pin = [0u8; 64];
            wifi::chosen(dev, &mut pin)
                .filter(|c| names.iter().any(|n| n == c))
                .map(String::from)
        };
        let len = screens::wifi_rows(names.len());
        {
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let pick = chosen
                .as_deref()
                .and_then(|c| names.iter().position(|n| n == c));
            home::draw(dev, out, full, |f, style| {
                screens::wifi(f, style, &bar.status(), &refs, pick, selected);
            });
        }
        full = false;
        let Some(key) = next_key(dev, out) else {
            return;
        };
        if step(&mut selected, len, key) {
            continue;
        }
        match key {
            Button::Back => return,
            Button::Confirm if selected + 1 == len => {
                crate::setup::run(dev, out, bar).await;
                full = true;
            }
            Button::Confirm => {
                let pick = selected.checked_sub(1).and_then(|i| names.get(i));
                let r = wifi::choose(dev, pick.map(String::as_str));
                let _ = match &r {
                    Ok(()) => writeln!(
                        out,
                        "wifi: chosen network set (automatic: {})",
                        pick.is_none()
                    ),
                    Err(e) => writeln!(out, "wifi: cannot save the choice: {e}"),
                };
                if let Err(e) = r {
                    home::message(
                        dev,
                        out,
                        bar,
                        "Could not save",
                        &format!("{e}.\n\nCheck the card, then try again."),
                        Keys::BACK,
                    );
                    let _ = next_key(dev, out);
                    full = true;
                } else if let Some(name) = pick {
                    // Prove it: join that network now and say how it went.
                    home::message(
                        dev,
                        out,
                        bar,
                        "Checking Wi-Fi",
                        &format!("Connecting to {name}\u{2026}"),
                        Keys::NONE,
                    );
                    let (title, body) = match wifi::try_network(dev, out, name).await {
                        Ok(rssi) => (
                            "Connected",
                            format!(
                                "{name} works{}.\n\nReceive tries it first. If it is not there, Receive uses your other saved networks.",
                                rssi.map_or(String::new(), |r| format!(" (signal {r} dBm)"))
                            ),
                        ),
                        Err(e) => (
                            "Could not connect",
                            format!(
                                "{name}: {e}.\n\nIt is still chosen, so Receive tries it first and then your other networks. Check the network is on and in range, or add it again to fix the password."
                            ),
                        ),
                    };
                    home::message(dev, out, bar, title, &body, Keys::BACK);
                    let _ = next_key(dev, out);
                    full = true;
                }
            }
            _ => {}
        }
    }
}

/// The Storage screen: what is on the card. Confirm offers to delete every
/// article and token.
fn storage(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &home::Bar) {
    let summary = library::summary(dev);
    let tokens = tokens::count(dev);
    home::draw(dev, out, true, |f, style| {
        screens::storage(f, style, &bar.status(), summary.articles, tokens);
    });
    loop {
        match next_key(dev, out) {
            None | Some(Button::Back) => return,
            Some(Button::Confirm) => break,
            _ => {}
        }
    }
    home::draw(dev, out, true, |f, style| {
        screens::delete_everything(f, style, &bar.status(), summary.articles, tokens);
    });
    loop {
        match next_key(dev, out) {
            None | Some(Button::Back) => return,
            Some(Button::Confirm) => break,
            _ => {}
        }
    }
    home::message(dev, out, bar, "Deleting", "Deleting\u{2026}", Keys::NONE);
    let r = sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |store| {
        x3_core::receive::wipe(store).map_err(|_| "the card stopped answering")
    });
    let (title, body) = match r {
        Ok(done) => {
            let _ = writeln!(
                out,
                "storage: wiped {} metadata and {} payload files",
                done.metadata, done.payloads
            );
            (
                "Deleted",
                String::from("The card holds no articles or pictures now."),
            )
        }
        Err(e) => {
            let _ = writeln!(out, "storage: wipe stopped: {e}");
            (
                "Not finished",
                format!("{e}.\n\nSome items may be left. What is left is intact. Try again."),
            )
        }
    };
    home::message(dev, out, bar, title, &body, Keys::BACK);
    let _ = next_key(dev, out);
}
