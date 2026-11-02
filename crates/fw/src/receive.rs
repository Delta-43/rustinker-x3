//! The `receive` command and the Confirm button (Phase 4): join Wi-Fi, run
//! one Receive session against the companion in `/X3/SERVER.CFG`, write new
//! items to `/X3/DATA`, then switch Wi-Fi off. The session itself is
//! `x3_core::receive`, the same code the simulator runs; this file is its
//! TCP transport and glue.
//!
//! Each session also samples the gauge's current every 250 ms, appends one
//! line to `/X3/LOG.TXT` and shows a summary on the panel, so it can be
//! measured on battery, away from the USB cable that charges the device.
extern crate alloc;

use crate::{Device, rtc, sd, serial::Out, stack, tls, tokens, wifi};
use alloc::{format, string::String};
use core::fmt::Write;
use embassy_futures::select::{Either, select};
use embassy_net::{IpEndpoint, Stack, dns::DnsQueryType, tcp::TcpSocket};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use embedded_tls::{Aes128GcmSha256, TlsConfig, TlsConnection, TlsContext};
use esp_hal::{Blocking, i2c::master::I2c};
use x3_core::receive::{Index, Net, NetError, Target, run};
use x3_proto::{config::Companion, lines::Hello};

/// PROTOCOL.md A.1: a whole session stops after this, even if jobs remain.
const SESSION: Duration = Duration::from_secs(180);
const RX_LEN: usize = 4096;
const TX_LEN: usize = 1024;
const LOG: &str = "LOG.TXT";
/// The log starts again once it passes this, so it never fills the card.
const LOG_CAP: usize = 12 * 1024;

/// Gauge current samples taken during a session.
#[derive(Default)]
struct Samples {
    sum_ma: i64,
    count: i64,
}

/// Runs until dropped, beside the session.
async fn sample(i2c: &mut I2c<'_, Blocking>, s: &mut Samples) -> core::convert::Infallible {
    loop {
        if let Some(ma) = rtc::current_ma(i2c) {
            s.sum_ma += i64::from(ma);
            s.count += 1;
        }
        Timer::after(Duration::from_millis(250)).await;
    }
}

type TlsConn<'a> = TlsConnection<'a, TcpSocket<'a>, Aes128GcmSha256>;

/// What the one socket is doing. A LAN request uses the plain socket. The
/// remote name wraps it in TLS (ADR 026), and the wrapper stays open for the
/// whole session; closing it gives the socket back. One `Link` lives per
/// session, so the size gap between variants costs nothing worth boxing.
#[allow(clippy::large_enum_variant)]
enum Link<'a> {
    Plain(TcpSocket<'a>),
    Tls(TlsConn<'a>),
    /// A failed TLS handshake or close consumed the socket.
    Empty,
}

/// One reusable socket. A LAN request connects each time, and `close` aborts
/// whatever is left so the socket is closed and ready for the next connect.
struct Tcp<'a> {
    net: Stack<'a>,
    link: Link<'a>,
    /// TLS record buffers, lent to the one TLS connection of the session.
    tls_buffers: Option<(&'a mut [u8], &'a mut [u8])>,
    deadline: Instant,
}

impl Tcp<'_> {
    fn limit(&self, secs: u64) -> Result<Duration, NetError> {
        let now = Instant::now();
        if now >= self.deadline {
            return Err(NetError::Timeout);
        }
        Ok((self.deadline - now).min(Duration::from_secs(secs)))
    }
}

impl<'a> Tcp<'a> {
    /// Closes TLS if it is open, so the plain socket is free for a LAN connect.
    async fn plain(&mut self) -> Result<&mut TcpSocket<'a>, NetError> {
        if matches!(self.link, Link::Tls(_)) {
            self.close().await;
        }
        match &mut self.link {
            Link::Plain(s) => Ok(s),
            _ => Err(NetError::Io),
        }
    }

    async fn connect_remote(&mut self, host: &str, port: u16) -> Result<(), NetError> {
        let addr =
            match with_timeout(self.limit(5)?, self.net.dns_query(host, DnsQueryType::A)).await {
                Ok(Ok(a)) if !a.is_empty() => a[0],
                Ok(_) => return Err(NetError::Connect),
                Err(_) => return Err(NetError::Timeout),
            };
        let Link::Plain(mut socket) = core::mem::replace(&mut self.link, Link::Empty) else {
            return Err(NetError::Io);
        };
        let t = self.limit(5)?;
        match with_timeout(t, socket.connect(IpEndpoint::new(addr, port))).await {
            Ok(Ok(())) => {}
            other => {
                self.link = Link::Plain(socket);
                return Err(if other.is_ok() {
                    NetError::Connect
                } else {
                    NetError::Timeout
                });
            }
        }
        let Some((read, write)) = self.tls_buffers.take() else {
            self.link = Link::Plain(socket);
            return Err(NetError::Io);
        };
        let mut tls = TlsConnection::new(socket, read, write);
        let config = TlsConfig::new().with_server_name(host);
        let t = self.limit(20)?;
        let opened = with_timeout(
            t,
            tls.open(TlsContext::new(&config, tls::Unchecked::default())),
        )
        .await;
        match opened {
            Ok(Ok(())) => {
                self.link = Link::Tls(tls);
                Ok(())
            }
            other => {
                // The record buffers went into `tls` and are gone for this
                // session; take the socket back so a close is clean.
                let (Ok(s) | Err((s, _))) = tls.close().await;
                self.link = Link::Plain(s);
                Err(if other.is_err() {
                    NetError::Timeout
                } else {
                    NetError::Connect
                })
            }
        }
    }
}

impl Net for Tcp<'_> {
    fn random(&mut self, out: &mut [u8]) {
        esp_hal::rng::Rng::new().read(out);
    }
    async fn open(&mut self, to: Target<'_>) -> Result<(), NetError> {
        match to {
            Target::Lan { ip, port } => {
                let t = self.limit(5)?;
                let [a, b, c, d] = ip;
                let to = (embassy_net::Ipv4Address::new(a, b, c, d), port);
                let socket = self.plain().await?;
                let r = match with_timeout(t, socket.connect(to)).await {
                    Ok(Ok(())) => return Ok(()),
                    Ok(Err(_)) => NetError::Connect,
                    Err(_) => NetError::Timeout,
                };
                // A timed-out connect leaves the socket in SYN-SENT; the next
                // connect on it would fail at once. Reset it now.
                self.close().await;
                Err(r)
            }
            Target::Remote { host, port } => {
                if matches!(self.link, Link::Tls(_)) {
                    return Ok(());
                }
                self.connect_remote(host, port).await
            }
        }
    }
    async fn send(&mut self, mut bytes: &[u8]) -> Result<(), NetError> {
        while !bytes.is_empty() {
            let t = self.limit(10)?;
            let written = match &mut self.link {
                Link::Plain(s) => with_timeout(t, s.write(bytes))
                    .await
                    .map(|r| r.map_err(|_| ())),
                Link::Tls(c) => with_timeout(t, c.write(bytes))
                    .await
                    .map(|r| r.map_err(|_| ())),
                Link::Empty => return Err(NetError::Io),
            };
            match written {
                Ok(Ok(0)) | Ok(Err(())) => return Err(NetError::Io),
                Ok(Ok(n)) => bytes = &bytes[n..],
                Err(_) => return Err(NetError::Timeout),
            }
        }
        let t = self.limit(10)?;
        if let Link::Tls(c) = &mut self.link {
            match with_timeout(t, c.flush()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return Err(NetError::Io),
                Err(_) => return Err(NetError::Timeout),
            }
        }
        Ok(())
    }
    async fn recv(&mut self, out: &mut [u8]) -> Result<usize, NetError> {
        let t = self.limit(10)?;
        let got = match &mut self.link {
            Link::Plain(s) => with_timeout(t, s.read(out))
                .await
                .map(|r| r.map_err(|_| ())),
            Link::Tls(c) => with_timeout(t, c.read(out))
                .await
                .map(|r| r.map_err(|_| ())),
            Link::Empty => return Err(NetError::Io),
        };
        match got {
            Ok(Ok(n)) => Ok(n),
            Ok(Err(())) => Err(NetError::Io),
            Err(_) => Err(NetError::Timeout),
        }
    }
    async fn close(&mut self) {
        // The response has been read to its end (or the request failed), so
        // nothing is lost by a reset. It leaves the socket Closed at once.
        match core::mem::replace(&mut self.link, Link::Empty) {
            Link::Plain(mut s) => {
                s.abort();
                let _ = with_timeout(Duration::from_secs(1), s.flush()).await;
                self.link = Link::Plain(s);
            }
            Link::Tls(c) => {
                if let Ok(Ok(mut s) | Err((mut s, _))) =
                    with_timeout(Duration::from_secs(2), c.close()).await
                {
                    s.abort();
                    let _ = with_timeout(Duration::from_secs(1), s.flush()).await;
                    self.link = Link::Plain(s);
                }
            }
            Link::Empty => {}
        }
    }
}

/// When the last good Receive ended (UTC), kept in RTC memory so it
/// survives deep sleep: [packed time, packed time ^ CHECK]. Persistent RTC
/// RAM can hold random bytes after some resets, hence the check word.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut LAST_RECEIVE: [u32; 2] = [0; 2];
const CHECK: u32 = 0x5A3C_96E1;

fn set_last_receive(t: &x3_core::rtc::DateTime) {
    let packed = (u32::from(t.year.saturating_sub(2000).min(4095)) << 20)
        | (u32::from(t.month & 15) << 16)
        | (u32::from(t.day & 31) << 11)
        | (u32::from(t.hour & 31) << 6)
        | u32::from(t.minute & 63);
    // SAFETY: single core, main task only, never touched from an interrupt.
    unsafe { core::ptr::addr_of_mut!(LAST_RECEIVE).write([packed, packed ^ CHECK]) };
}

/// The last good Receive (UTC), if one is recorded and its check holds.
pub fn last_receive() -> Option<x3_core::rtc::DateTime> {
    // SAFETY: as in `set_last_receive`.
    let [packed, check] = unsafe { core::ptr::addr_of!(LAST_RECEIVE).read() };
    if packed == 0 || packed ^ CHECK != check {
        return None;
    }
    let t = x3_core::rtc::DateTime {
        year: 2000 + (packed >> 20) as u16,
        month: (packed >> 16 & 15) as u8,
        day: (packed >> 11 & 31) as u8,
        hour: (packed >> 6 & 31) as u8,
        minute: (packed & 63) as u8,
        second: 0,
    };
    (1..=12).contains(&t.month).then_some(t)
}

pub async fn receive(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let mut wifi_buf = [0u8; 512];
    let wifi = match wifi::config(dev, &mut wifi_buf) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(out, "receive: {e}");
            return;
        }
    };
    let mut server_buf = [0u8; 512];
    let companion =
        match sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, "SERVER.CFG", &mut server_buf) {
            // Never print the file: it holds the device token.
            Ok(n) => match Companion::parse(&server_buf[..n]) {
                Ok(c) => c,
                Err(_) => {
                    let _ = writeln!(
                        out,
                        "receive: /X3/SERVER.CFG is not valid (url=…, token=… lines)"
                    );
                    return;
                }
            },
            Err(_) => {
                let _ = writeln!(out, "receive: no companion is configured (/X3/SERVER.CFG)");
                return;
            }
        };
    let hello = Hello {
        firmware: env!("CARGO_PKG_VERSION"),
        width: 792,
        height: 528,
        formats: "tk1,ar1,im1",
        free_kib: 0,
        battery: None,
    };
    let before = dev.i2c.as_mut().map(rtc::gauge);
    // A token shown after an earlier Receive still owes its `displayed` ack.
    let owed = tokens::owed(dev);
    let mut on_screen = [0u8; 48];
    let mut on_screen_len = 0;
    if let Some(o) = &owed {
        on_screen[..o.item().len()].copy_from_slice(o.item().as_bytes());
        on_screen_len = o.item().len();
    }
    let mut pin = [0u8; 64];
    let chosen = wifi::chosen(dev, &mut pin);
    let t0 = Instant::now();
    let mut samples = Samples::default();
    let mut timing = (0u64, 0u64);
    let mut network = None;
    let Device {
        wifi: radio,
        epd,
        sd_cs,
        i2c,
        ..
    } = &mut *dev;
    let session = wifi::online(
        radio,
        &wifi,
        chosen,
        out,
        async |net: Stack<'_>, link: &wifi::Link, out| {
            timing = (link.joined_ms, link.address_ms);
            network = Some(link.network);
            sd::with_data_async(epd.bus(), sd_cs, async |store| {
            let mut index = Index::load(store).map_err(|_| "cannot read /X3/DATA")?;
            let mut rx = [0u8; RX_LEN];
            let mut tx = [0u8; TX_LEN];
            // TLS record buffers are only needed for the internet form, and are
            // big (16 KB), so they come from the heap only then.
            let mut tls_read = alloc::vec::Vec::new();
            let mut tls_write = alloc::vec::Vec::new();
            let tls_buffers = if companion.remote.is_some() {
                tls_read.resize(16_640, 0u8);
                tls_write.resize(2_048, 0u8);
                Some((&mut tls_read[..], &mut tls_write[..]))
            } else {
                None
            };
            let mut tcp = Tcp {
                net,
                link: Link::Plain(TcpSocket::new(net, &mut rx, &mut tx)),
                tls_buffers,
                deadline: Instant::now() + SESSION,
            };
            let report = run(&mut tcp, store, &mut index, &companion, &hello, owed.as_ref()).await;
            // Keep the newest 20 tokens and the one that is (or will be) on screen.
            let keep = match &report.show {
                Some(s) => s.item(),
                None => core::str::from_utf8(&on_screen[..on_screen_len]).unwrap_or(""),
            };
            let evicted = index.evict_tokens(store, keep, 20).unwrap_or(0);
            let _ = writeln!(
                out,
                "receive: {} new, {} deleted, {} rejected, {evicted} tokens and {} articles evicted; companion time {:?}; show {:?}",
                report.received,
                report.deleted,
                report.rejected,
                index.articles_evicted,
                report.time,
                report.show.as_ref().map(|s| s.item()),
            );
            // Superseded and abandoned files go now; the index is current.
            let t_gc = Instant::now();
            match index.collect_garbage(store) {
                Ok(c) => {
                    let _ = writeln!(
                        out,
                        "receive: gc removed {} metadata, {} payloads, kept {} unsure, {} ms",
                        c.metadata,
                        c.payloads,
                        c.kept_unsure,
                        t_gc.elapsed().as_millis()
                    );
                }
                Err(e) => {
                    let _ = writeln!(out, "receive: gc stopped: {e:?}");
                }
            }
            let live = index.iter().count();
            let tokens = index.iter().filter(|r| r.token).count();
            let _ = writeln!(
                out,
                "receive: {live} items on the card ({tokens} tokens); stack {} untouched of {}",
                stack::untouched(),
                stack::size()
            );
            Ok((report, evicted, index.articles_evicted))
        })
        .await
        },
    );
    let r = match i2c.as_mut() {
        Some(bus) => match select(session, sample(bus, &mut samples)).await {
            Either::First(r) => r,
            Either::Second(never) => match never {},
        },
        None => session.await,
    };
    let ms = t0.elapsed().as_millis();
    let mut counts = (0, 0, 0, 0);
    let mut articles_evicted = 0;
    let mut shown = None;
    let mut remote = None;
    let outcome: Result<(), String> = match r {
        Ok(Ok((mut report, evicted, articles))) => {
            counts = (report.received, report.deleted, report.rejected, evicted);
            remote = Some(report.remote);
            articles_evicted = articles;
            if report.displayed {
                tokens::set_owed(dev, out, None);
            }
            shown = report.show.take();
            if let (Some(t), Some(i2c)) = (report.time, dev.i2c.as_mut()) {
                rtc::sync(i2c, t, out);
            }
            report
                .result
                .map_err(|e| format!("{} ({e:?})", e.message()))
        }
        Ok(Err(e)) => Err(e.into()),
        Err(e) => Err(format!("Wi-Fi: {e}")),
    };
    match &outcome {
        Ok(()) => {
            let _ = writeln!(out, "receive: done in {ms} ms");
        }
        Err(e) => {
            let _ = writeln!(out, "receive: {e} after {ms} ms");
        }
    }
    // Charge drawn: mean current (negative on battery) times duration.
    // mA × ms / 3600 = µAh.
    let mean_ma = (samples.count > 0).then(|| samples.sum_ma / samples.count);
    let used_uah = mean_ma.map(|m| -m * ms as i64 / 3600);
    let after = dev.i2c.as_mut().map(rtc::gauge);
    let when = dev.i2c.as_mut().and_then(rtc::read);
    if outcome.is_ok()
        && let Some(t) = &when
    {
        set_last_receive(t);
    }
    let pair = |f: fn(&x3_core::battery::Reading) -> Option<i64>| {
        let v = |r: &Option<x3_core::battery::Reading>| {
            r.as_ref()
                .and_then(f)
                .map_or(String::from("?"), |v| format!("{v}"))
        };
        format!("{}>{}", v(&before), v(&after))
    };
    // The route goes in the log; the network name does not (people paste
    // the log into bug reports).
    let route = match remote {
        Some(true) => "internet",
        Some(false) => "lan",
        None => "-",
    };
    let line = format!(
        "{} up={}s ms={ms} join={} dhcp={} net={} route={route} new={} del={} rej={} ev={} aev={} result={} mean_ma={} uah={} samples={} mv={} soc={} mah={}\n",
        when.map_or(String::from("-"), |t| format!("{t}")),
        Instant::now().as_secs(),
        timing.0,
        timing.1,
        network.map_or(String::from("-"), |n| format!("{}", n + 1)),
        counts.0,
        counts.1,
        counts.2,
        counts.3,
        articles_evicted,
        outcome
            .as_ref()
            .map_or_else(|e| e.replace(' ', "_"), |()| String::from("ok")),
        mean_ma.map_or(String::from("?"), |m| format!("{m}")),
        used_uah.map_or(String::from("?"), |u| format!("{u}")),
        samples.count,
        pair(|r| r.millivolts.map(i64::from)),
        pair(|r| r.percent.map(i64::from)),
        pair(|r| r.remaining_mah.map(i64::from)),
    );
    let _ = write!(out, "receive: log {line}");
    if let Err(e) = sd::append_x3_file(dev.epd.bus(), &mut dev.sd_cs, LOG, line.as_bytes(), LOG_CAP)
    {
        let _ = writeln!(out, "receive: /X3/LOG.TXT: {e}");
    }
    let battery = match after.and_then(|a| a.percent.zip(a.millivolts)) {
        Some((p, mv)) => format!("{p}% ({mv} mV)"),
        None => String::from("unknown"),
    };
    let took = format!("{}.{} s", ms / 1000, ms % 1000 / 100);
    let used = used_uah.map(|u| format!("{}.{:03} mAh", u / 1000, u.rem_euclid(1000)));
    let network_name = network
        .and_then(|n| wifi.iter().nth(n))
        .map_or(String::from("not joined"), |w| String::from(w.ssid));
    let route_name = match remote {
        Some(true) => "Internet (signed)",
        Some(false) => "Home network",
        None => "none",
    };
    let result = ResultScreen {
        network: &network_name,
        route: route_name,
        outcome: &outcome,
        counts,
        articles_evicted,
        took: &took,
        used: used.as_deref(),
        battery: &battery,
    };
    // A `show` token replaces the summary on the panel (PROTOCOL.md A.2);
    // its `displayed` ack goes out at the next Receive.
    if let Some(token) = shown {
        match tokens::draw(dev, out, token.item()) {
            Ok(()) => {
                tokens::set_owed(dev, out, Some(&token));
                return;
            }
            Err(e) => {
                let _ = writeln!(out, "receive: show token: {e}");
            }
        }
    }
    show(dev, out, &result);
}

/// What the result page says.
struct ResultScreen<'a> {
    /// The Wi-Fi network that joined, by name.
    network: &'a str,
    /// How the companion was reached: the home LAN or the internet route.
    route: &'a str,
    outcome: &'a Result<(), String>,
    /// New, deleted, rejected, tokens evicted.
    counts: (usize, usize, usize, usize),
    articles_evicted: usize,
    took: &'a str,
    used: Option<&'a str>,
    battery: &'a str,
}

/// Draws the result page in the Home design with a full refresh. Back or
/// Confirm returns to Home.
fn show(dev: &mut Device<'_>, out: &mut Out<'_>, r: &ResultScreen<'_>) {
    use x3_core::{
        reader::{FRAME_LEN, Frame},
        screens::{self, ReceiveDone},
    };
    let Some(style) = crate::reader_style() else {
        return;
    };
    if !crate::ensure_epd(dev, out) {
        return;
    }
    let bar = crate::home::Bar::read(dev);
    let mut buf = [0u8; FRAME_LEN];
    let mut frame = Frame(&mut buf);
    match r.outcome {
        Ok(()) => {
            let (new, deleted, rejected, tokens_evicted) = r.counts;
            let done = ReceiveDone {
                new,
                deleted,
                rejected,
                made_room: tokens_evicted + r.articles_evicted,
                network: r.network,
                route: r.route,
                took: r.took,
                charge_used: r.used,
                battery: r.battery,
            };
            screens::receive_done(&mut frame, &style, &bar.status(), &done);
        }
        Err(e) => screens::receive_failed(&mut frame, &style, &bar.status(), e, r.network),
    }
    let r = dev.epd.change(&frame);
    let _ = writeln!(out, "receive: panel {r:?}");
}

/// The `log` command: prints `/X3/LOG.TXT`.
pub fn print_log(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let mut buf = [0u8; LOG_CAP + 1024];
    match sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, LOG, &mut buf) {
        Ok(n) => {
            for line in buf[..n].split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
                let _ = writeln!(out, "log: {}", core::str::from_utf8(line).unwrap_or("?"));
                out.flush();
            }
        }
        Err(e) => {
            let _ = writeln!(out, "log: /X3/LOG.TXT: {e}");
        }
    }
}

/// The test companion's LAN address (`tools/drill-companion.sh`): a throwaway
/// instance on the server, never the real one.
const DRILL_URL: &str = "http://192.168.1.20:18789";
const CONFIG: &str = "SERVER.CFG";
const BACKUP: &str = "SERVER.BAK";

/// `drill TOKEN` points the device at the test companion and keeps the real
/// `/X3/SERVER.CFG` in `/X3/SERVER.BAK`; `drill off` puts it back. Never prints
/// either file: they hold device tokens. Refuses to overwrite a backup, so the
/// real configuration cannot be lost by running `drill` twice.
pub fn drill(dev: &mut Device<'_>, out: &mut Out<'_>, arg: &str) {
    let say = |out: &mut Out<'_>, m: &str| {
        let _ = writeln!(out, "drill: {m}");
    };
    let mut real = [0u8; 512];
    let mut saved = [0u8; 512];
    let held = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, BACKUP, &mut saved).unwrap_or(0);
    if arg == "off" {
        if held == 0 {
            return say(out, "nothing to restore: there is no backup");
        }
        let bus = dev.epd.bus();
        if sd::write_x3_file(bus, &mut dev.sd_cs, CONFIG, &saved[..held]).is_err() {
            return say(out, "could not write /X3/SERVER.CFG; the backup is kept");
        }
        let mut check = [0u8; 512];
        let n = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, CONFIG, &mut check).unwrap_or(0);
        if check[..n] != saved[..held] {
            return say(out, "the restored file does not match; the backup is kept");
        }
        let _ = sd::write_x3_file(dev.epd.bus(), &mut dev.sd_cs, BACKUP, &[]);
        return say(out, "the real companion is configured again");
    }
    if arg.len() != 43
        || !arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return say(out, "usage: drill TOKEN (43 characters) | drill off");
    }
    if held != 0 {
        return say(out, "a backup already exists; run `drill off` first");
    }
    let Ok(n) = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, CONFIG, &mut real) else {
        return say(out, "no /X3/SERVER.CFG to back up");
    };
    let bus = dev.epd.bus();
    if sd::write_x3_file(bus, &mut dev.sd_cs, BACKUP, &real[..n]).is_err() {
        return say(out, "could not write the backup; nothing changed");
    }
    let m = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, BACKUP, &mut saved).unwrap_or(0);
    if saved[..m] != real[..n] {
        return say(out, "the backup does not match; nothing changed");
    }
    let mut cfg = [0u8; 160];
    let mut at = 0;
    for part in ["url=", DRILL_URL, "\ntoken=", arg, "\n"] {
        cfg[at..at + part.len()].copy_from_slice(part.as_bytes());
        at += part.len();
    }
    if sd::write_x3_file(dev.epd.bus(), &mut dev.sd_cs, CONFIG, &cfg[..at]).is_err() {
        return say(out, "could not write /X3/SERVER.CFG; run `drill off`");
    }
    say(
        out,
        "the device now uses the test companion; run `drill off` to go back",
    );
}
