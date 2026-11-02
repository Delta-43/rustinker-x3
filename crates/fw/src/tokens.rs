//! Token mode. 2 bpp tokens are drawn in four greys (ADR 018,
//! CrossPoint's method): a black-and-white base, a settle pass, then the
//! two grey planes and a weak nudge. Each plane is a separate pass over the
//! card into the one frame buffer (`x3_core::token`). 1 bpp tokens stay
//! black and white.
//!
//! The token shown after a Receive owes the companion a `displayed` ack,
//! sent at the start of the next Receive (PROTOCOL.md A.2). It is kept in
//! `/X3/SHOWN.CFG` as one line `job item sha256`, so it survives a restart.
use crate::{Device, epd::Inverted, sd, serial::Out};
use core::fmt::Write;
use x3_core::{
    buttons::{Button, Debouncer, decode},
    reader::{FRAME_LEN, Frame},
    receive::{Index, Shown},
    storage::{Record, Store},
    token::{BASE, Decoder, HEADER_LEN, HIGH, LOW},
};

const SHOWN: &str = "SHOWN.CFG";

/// The owed `displayed` ack, if any.
pub fn owed(dev: &mut Device<'_>) -> Option<Shown> {
    let mut buf = [0u8; 160];
    let n = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, SHOWN, &mut buf).ok()?;
    let line = core::str::from_utf8(&buf[..n]).ok()?.strip_suffix('\n')?;
    let mut f = line.split(' ');
    let (job, item, sha) = (f.next()?, f.next()?, f.next()?);
    if f.next().is_some() {
        return None;
    }
    Shown::new(job, item, sha).ok()
}

/// Records (or with `None` clears) the owed ack.
pub fn set_owed(dev: &mut Device<'_>, out: &mut Out<'_>, shown: Option<&Shown>) {
    let mut line = [0u8; 160];
    let mut n = 0;
    if let Some(s) = shown {
        for part in [s.job(), " ", s.item(), " ", s.sha(), "\n"] {
            line[n..n + part.len()].copy_from_slice(part.as_bytes());
            n += part.len();
        }
    }
    if let Err(e) = sd::write_x3_file(dev.epd.bus(), &mut dev.sd_cs, SHOWN, &line[..n]) {
        let _ = writeln!(out, "token: /X3/SHOWN.CFG: {e}");
    }
}

/// Token records, newest first, at most 20 (the cache size).
fn list(dev: &mut Device<'_>) -> Result<([Option<Record>; 20], usize), &'static str> {
    sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |store| {
        let index = Index::load(store).map_err(|_| "cannot read /X3/DATA")?;
        let mut all = [const { None }; 20];
        let mut n = 0;
        for r in index.iter().filter(|r| r.token) {
            if n < all.len() {
                all[n] = Some(r.clone());
                n += 1;
            }
        }
        all[..n]
            .sort_unstable_by_key(|r| core::cmp::Reverse(r.as_ref().map(|r| r.payload_generation)));
        Ok((all, n))
    })
}

/// One pass over the token on the card into `frame`, inking the levels in
/// `mask`. Returns the token's bits per pixel.
fn decode_into(
    dev: &mut Device<'_>,
    frame: &mut Frame<'_>,
    r: &Record,
    mask: u8,
) -> Result<u8, &'static str> {
    frame.clear();
    sd::with_data(dev.epd.bus(), &mut dev.sd_cs, |store| {
        let key = r.payload_key();
        let mut header = [0u8; HEADER_LEN];
        if store.read(key, 0, &mut header) != Ok(HEADER_LEN) {
            return Err("cannot read the token");
        }
        let mut d = Decoder::new(&header, mask).map_err(|_| "not a token")?;
        let mut chunk = [0u8; 512];
        let mut at = HEADER_LEN;
        while at < r.len as usize {
            let want = chunk.len().min(r.len as usize - at);
            let got = store
                .read(key, at, &mut chunk[..want])
                .map_err(|_| "cannot read the token")?;
            if got == 0 {
                return Err("token cut short");
            }
            d.feed(frame, &chunk[..got]).map_err(|_| "token too long")?;
            at += got;
        }
        if d.done() {
            Ok(d.header().bpp)
        } else {
            Err("token cut short")
        }
    })
}

/// Draws one token: four greys for 2 bpp, black and white for 1 bpp.
fn draw_record(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    buf: &mut [u8; FRAME_LEN],
    r: &Record,
) -> Result<(), &'static str> {
    let t0 = esp_hal::time::Instant::now();
    let mut frame = Frame(buf);
    let bpp = decode_into(dev, &mut frame, r, BASE)?;
    if !crate::ensure_epd(dev, out) {
        return Err("panel not ready");
    }
    if bpp == 1 {
        // At 1 bpp the base pass already is the picture.
        crate::capture::send(out, "bw", &frame);
        dev.epd.full(&frame).map_err(|_| "panel error")?;
        let _ = writeln!(
            out,
            "token: {} drawn, 1 bpp ({} ms)",
            r.item(),
            t0.elapsed().as_millis()
        );
        return Ok(());
    }
    crate::capture::send(out, "base", &frame);
    dev.epd
        .grey_base(&frame)
        .map_err(|_| "panel error (base)")?;
    let base_ms = t0.elapsed().as_millis();
    decode_into(dev, &mut frame, r, LOW)?;
    crate::capture::send(out, "low", &Inverted(&frame));
    dev.epd
        .grey_plane(false, &Inverted(&frame))
        .map_err(|_| "panel error (low plane)")?;
    decode_into(dev, &mut frame, r, HIGH)?;
    crate::capture::send(out, "high", &Inverted(&frame));
    dev.epd
        .grey_plane(true, &Inverted(&frame))
        .map_err(|_| "panel error (high plane)")?;
    let nudge = dev.epd.grey_show().map_err(|_| "panel error (nudge)")?;
    let _ = writeln!(
        out,
        "token: {} drawn in greys (base {base_ms} ms, nudge {nudge} ms, total {} ms)",
        r.item(),
        t0.elapsed().as_millis()
    );
    Ok(())
}

/// How many tokens are on the card (at most 20), for Home.
pub fn count(dev: &mut Device<'_>) -> usize {
    list(dev).map_or(0, |(_, n)| n)
}

/// The picture chosen for the sleep screen: `/X3/SLEEP.CFG`, one item ID.
const SLEEP: &str = "SLEEP.CFG";

/// Makes `item` the sleep screen picture.
fn set_sleep_picture(dev: &mut Device<'_>, item: &str) -> Result<(), &'static str> {
    x3_proto::item_id(item).map_err(|_| "bad item")?;
    let mut line = [0u8; 50];
    line[..item.len()].copy_from_slice(item.as_bytes());
    line[item.len()] = b'\n';
    sd::write_x3_file(dev.epd.bus(), &mut dev.sd_cs, SLEEP, &line[..=item.len()])
}

/// The sleep screen picture's item ID, if one is chosen and valid.
pub fn sleep_picture<'b>(dev: &mut Device<'_>, buf: &'b mut [u8; 50]) -> Option<&'b str> {
    let n = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, SLEEP, buf).ok()?;
    let item = core::str::from_utf8(&buf[..n]).ok()?.strip_suffix('\n')?;
    x3_proto::item_id(item).ok()
}

/// Draws the token with this item ID, if it is on the card.
pub fn draw(dev: &mut Device<'_>, out: &mut Out<'_>, item: &str) -> Result<(), &'static str> {
    let (all, n) = list(dev)?;
    let r = all[..n]
        .iter()
        .flatten()
        .find(|r| r.item() == item)
        .ok_or("token not on the card")?;
    let mut buf = [0u8; FRAME_LEN];
    draw_record(dev, out, &mut buf, r)
}

/// The `tokens` command: newest token first; Left/Up older, Right/Down
/// newer, Back (or any serial input) leaves. The last one stays on screen.
pub fn browse(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let (all, n) = match list(dev) {
        Ok(x) => x,
        Err(e) => {
            let _ = writeln!(out, "tokens: {e}");
            return;
        }
    };
    if n == 0 {
        let _ = writeln!(out, "tokens: none on the card");
        return;
    }
    let _ = writeln!(
        out,
        "tokens: {n}; Left/Up older, Right/Down newer, Back leaves"
    );
    let mut buf = [0u8; FRAME_LEN];
    let mut at = 0usize;
    let mut debounce = Debouncer::default();
    loop {
        if let Some(r) = all[at].as_ref()
            && let Err(e) = draw_record(dev, out, &mut buf, r)
        {
            let _ = writeln!(out, "tokens: {e}");
        }
        out.flush();
        let next = loop {
            if out.read_byte().is_some() {
                break None;
            }
            let s = dev.buttons.sample();
            let down = decode(s.bottom, s.side, s.power_pressed);
            if down.is_some() {
                dev.power.activity();
            }
            let pressed = debounce.feed(down);
            if let Some(b) = pressed {
                let _ = writeln!(out, "tokens: key {b:?} at {}/{n}", at + 1);
                out.flush();
            }
            match pressed {
                Some(Button::Left | Button::Up) if at + 1 < n => break Some(at + 1),
                Some(Button::Right | Button::Down) if at > 0 => break Some(at - 1),
                Some(Button::Back) => break None,
                _ => {}
            }
            dev.nap(out, 20);
        };
        match next {
            Some(i) => at = i,
            None => {
                let _ = writeln!(out, "tokens: done");
                return;
            }
        }
    }
}

/// The Tokens screen from Home: the tokens on the card by name, newest
/// first. Confirm shows one in greys; on the card Left/Up and Right/Down
/// step older and newer, Back returns to the list. Back on the list leaves.
pub fn screen(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    style: &x3_core::reader::Style<'_>,
    bar: &crate::home::Bar,
) {
    use x3_core::screens;
    let (all, n) = match list(dev) {
        Ok(x) => x,
        Err(e) => {
            let _ = writeln!(out, "tokens: {e}");
            ([const { None }; 20], 0)
        }
    };
    let mut buf = [0u8; FRAME_LEN];
    let mut selected = 0usize;
    let mut full = true;
    let mut debounce = Debouncer::default();
    loop {
        let mut items = [""; screens::MAX_PICTURES];
        for (slot, r) in items.iter_mut().zip(&all[..n]) {
            *slot = r.as_ref().map_or("", |r| r.item());
        }
        let mut frame = Frame(&mut buf);
        screens::pictures(&mut frame, style, &bar.status(), &items[..n], selected);
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
            let _ = writeln!(out, "tokens: display {r:?}");
            return;
        }
        full = false;
        let open = loop {
            if out.read_byte().is_some() {
                return;
            }
            let s = dev.buttons.sample();
            let down = decode(s.bottom, s.side, s.power_pressed);
            if down.is_some() {
                dev.power.activity();
            }
            match debounce.feed(down) {
                Some(Button::Down | Button::Right) if selected + 1 < n => {
                    dev.power.activity();
                    selected += 1;
                    break false;
                }
                Some(Button::Up | Button::Left) if selected > 0 => {
                    dev.power.activity();
                    selected -= 1;
                    break false;
                }
                Some(Button::Confirm) if n > 0 => break true,
                Some(Button::Back) => return,
                _ => {}
            }
            dev.nap(out, 20);
        };
        if !open {
            continue;
        }
        // The card, with stepping between tokens.
        loop {
            if let Some(r) = all[selected].as_ref()
                && let Err(e) = draw_record(dev, out, &mut buf, r)
            {
                let _ = writeln!(out, "tokens: {e}");
            }
            out.flush();
            let next = loop {
                if out.read_byte().is_some() {
                    return;
                }
                let s = dev.buttons.sample();
                let down = decode(s.bottom, s.side, s.power_pressed);
                if down.is_some() {
                    dev.power.activity();
                }
                match debounce.feed(down) {
                    Some(Button::Left | Button::Up) if selected + 1 < n => {
                        break Some(selected + 1);
                    }
                    Some(Button::Right | Button::Down) if selected > 0 => break Some(selected - 1),
                    Some(Button::Back) => break None,
                    // Confirm: this picture becomes the sleep screen.
                    Some(Button::Confirm) => {
                        let item = all[selected].as_ref().map(|r| r.item());
                        let ok = item.is_some_and(|i| set_sleep_picture(dev, i).is_ok());
                        let mut frame = Frame(&mut buf);
                        screens::sleep_picture_set(&mut frame, style, &bar.status(), ok);
                        if crate::ensure_epd(dev, out) {
                            let _ = dev.epd.change(&frame);
                        }
                        crate::menu::wait_key(dev, out);
                        break Some(selected);
                    }
                    _ => {}
                }
                dev.nap(out, 20);
            };
            match next {
                Some(i) => selected = i,
                None => break,
            }
        }
        full = true;
    }
}
