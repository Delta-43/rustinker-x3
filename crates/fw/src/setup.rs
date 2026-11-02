//! Wi-Fi setup from the phone (ADR 025). A WPA2 hotspot, a QR code on the
//! screen that joins it, one small page on 192.168.4.1 and a tiny DHCP server.
//! The radio is on only while this runs: Back, a saved network or five minutes
//! ends it.
//!
//! Nothing the phone sends is printed: the form holds a password.
extern crate alloc;

use crate::{Device, home::Bar, home::message, sd, serial::Out, wifi};
use core::fmt::Write;
use embassy_futures::select::{Either, select};
use embassy_net::{
    Config as NetConfig, IpEndpoint, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4,
    tcp::{State, TcpSocket},
    udp::{PacketMetadata, UdpSocket},
};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_radio::wifi::{
    AuthenticationMethod, Config, ap::AccessPointConfig, scan::ScanConfig, sta::StationConfig,
};
use x3_core::{
    buttons::{Button, Debouncer, decode},
    dhcp, dns, gui,
    gui::Keys,
    reader::{FRAME_LEN, Frame},
    setup::{self as page, Method, Notice, Parse},
};
use x3_proto::config::{WifiList, save_network};

/// The hotspot stays up this long.
const SESSION: Duration = Duration::from_secs(300);
/// A phone that opens a connection and says nothing is dropped after this.
const READ_LIMIT: Duration = Duration::from_secs(10);
const BODY_CAP: usize = 2048;

enum End {
    Saved,
    Back,
    TimedOut,
}

/// A fixed buffer to render text into. Sized to its use: these live in the
/// main task's future, and every byte here is a byte less for the reader's stack.
struct Sink<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
type Body = Sink<BODY_CAP>;
impl<const N: usize> Write for Sink<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
        let dst = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub async fn run(dev: &mut Device<'_>, out: &mut Out<'_>, bar: &Bar) {
    message(
        dev,
        out,
        bar,
        "Wi-Fi setup",
        "Starting the setup hotspot\u{2026}",
        Keys::NONE,
    );
    let end = hotspot(dev, out, bar).await;
    match end {
        Ok(End::Saved) => {
            message(
                dev,
                out,
                bar,
                "Wi-Fi saved",
                "Joining the new network\u{2026}",
                Keys::NONE,
            );
            let mut buf = [0u8; 512];
            let result = match wifi::config(dev, &mut buf) {
                Ok(list) => {
                    // The newest network is last; a one-network try is honest.
                    wifi::online(&mut dev.wifi, &list, None, out, async |_net, link, _out| {
                        link.rssi
                    })
                    .await
                }
                Err(e) => Err(alloc::string::String::from(e)),
            };
            let (title, body) = match result {
                Ok(rssi) => (
                    "Connected",
                    alloc::format!(
                        "The X3 joined the network (signal {} dBm).\n\nReceive will use it.\n\nBack returns.",
                        rssi.unwrap_or(0)
                    ),
                ),
                Err(e) => (
                    "Saved, but not joined",
                    alloc::format!(
                        "The network is saved, but the X3 could not join it: {e}.\n\nCheck the password and try setup again. A name that is already saved is replaced.\n\nBack returns."
                    ),
                ),
            };
            message(dev, out, bar, title, &body, Keys::BACK);
            wait_for_back(dev, out).await;
        }
        Ok(End::Back) => {}
        Ok(End::TimedOut) => {
            message(
                dev,
                out,
                bar,
                "Setup timed out",
                "Nothing was saved. Press Left on the Device screen to start again.\n\nBack returns.",
                Keys::BACK,
            );
            wait_for_back(dev, out).await;
        }
        Err(e) => {
            let _ = writeln!(out, "setup: {e}");
            message(
                dev,
                out,
                bar,
                "Wi-Fi setup failed",
                &alloc::format!("The hotspot did not start: {e}.\n\nBack returns."),
                Keys::BACK,
            );
            wait_for_back(dev, out).await;
        }
    }
}

async fn wait_for_back(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let mut debounce = Debouncer::default();
    loop {
        if out.read_byte().is_some() {
            return;
        }
        let s = dev.buttons.sample();
        let down = decode(s.bottom, s.side, s.power_pressed);
        if down.is_some() {
            dev.power.activity();
        }
        if debounce.feed(down) == Some(Button::Back) {
            return;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}

/// Draws the screen with the QR code. A plain function, so the 52 KB frame is
/// gone from the stack before the radio session starts.
fn draw(epd: &mut crate::epd::Epd<'_>, out: &mut Out<'_>, bar: &Bar, ssid: &str, password: &str) {
    let Some(style) = crate::reader_style() else {
        return;
    };
    let mut join = Sink::<128> {
        bytes: [0; 128],
        len: 0,
    };
    let _ = page::join_qr_text(&mut join, ssid, password);
    let text = core::str::from_utf8(&join.bytes[..join.len]).unwrap_or("");
    let mut buf = [0u8; FRAME_LEN];
    let mut frame = Frame(&mut buf);
    gui::wifi_setup(
        &mut frame,
        &style,
        &bar.status(),
        ssid,
        password,
        text,
        Keys::BACK,
    );
    if let Err(e) = epd.change(&frame) {
        let _ = writeln!(out, "setup: display {e:?}");
    }
}

async fn hotspot(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    bar: &Bar,
) -> Result<End, alloc::string::String> {
    let Device {
        wifi: radio,
        epd,
        sd_cs,
        buttons,
        power,
        ..
    } = &mut *dev;
    let (mut controller, interfaces) = esp_radio::wifi::new(radio.reborrow(), Default::default())
        .map_err(|e| alloc::format!("radio init {e:?}"))?;
    // The radio is on, so the hardware RNG is a real one.
    let rng = esp_hal::rng::Rng::new();
    let mut password = [0u8; page::PASSWORD_LEN];
    page::password(|| rng.random(), &mut password);
    let mut suffix = [0u8; 4];
    let mut suffix_source = [0u8; page::PASSWORD_LEN];
    page::password(|| rng.random(), &mut suffix_source);
    suffix.copy_from_slice(&suffix_source[..4]);
    let password = core::str::from_utf8(&password).unwrap_or("");
    let ssid = alloc::format!(
        "X3-setup-{}",
        core::str::from_utf8(&suffix).unwrap_or("0000")
    );
    let ap = AccessPointConfig::default()
        .with_ssid(ssid.as_str())
        .with_auth_method(AuthenticationMethod::Wpa2Personal)
        .with_password(alloc::string::String::from(password))
        .with_channel(6)
        .with_max_connections(1);
    controller
        .set_config(&Config::AccessPointStation(StationConfig::default(), ap))
        .map_err(|e| alloc::format!("config {e:?}"))?;
    // Look before the phone joins: a scan hops channels and would stall it.
    let mut names: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    if let Ok(found) = controller
        .scan_async(&ScanConfig::default().with_max(24))
        .await
    {
        let mut found: alloc::vec::Vec<_> = found.into_iter().collect();
        found.sort_by_key(|a| core::cmp::Reverse(a.signal_strength));
        for a in found {
            let name = a.ssid.as_str();
            if !name.is_empty()
                && !name.starts_with("X3-setup-")
                && !names.iter().any(|n| n == name)
            {
                names.push(alloc::string::String::from(name));
            }
        }
    }
    let _ = writeln!(out, "setup: hotspot up, {} networks seen", names.len());
    draw(epd, out, bar, &ssid, password);

    let seed =
        (esp_hal::rng::Rng::new().random() as u64) << 32 | esp_hal::rng::Rng::new().random() as u64;
    let mut resources = StackResources::<4>::new();
    let (net, mut runner) = embassy_net::new(
        interfaces.access_point,
        NetConfig::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 4, 1), 24),
            gateway: None,
            dns_servers: Default::default(),
        }),
        &mut resources,
        seed,
    );
    let serve = async {
        let mut udp_rx_meta = [PacketMetadata::EMPTY; 2];
        let mut udp_rx = [0u8; 512];
        let mut udp_tx_meta = [PacketMetadata::EMPTY; 2];
        let mut udp_tx = [0u8; dhcp::REPLY_CAP];
        let mut udp = UdpSocket::new(
            net,
            &mut udp_rx_meta,
            &mut udp_rx,
            &mut udp_tx_meta,
            &mut udp_tx,
        );
        udp.bind(67)
            .map_err(|e| alloc::format!("dhcp bind {e:?}"))?;
        let mut dns_rx_meta = [PacketMetadata::EMPTY; 2];
        let mut dns_rx = [0u8; dns::REQUEST_CAP];
        let mut dns_tx_meta = [PacketMetadata::EMPTY; 2];
        let mut dns_tx = [0u8; dns::REPLY_CAP];
        let mut resolver = UdpSocket::new(
            net,
            &mut dns_rx_meta,
            &mut dns_rx,
            &mut dns_tx_meta,
            &mut dns_tx,
        );
        resolver
            .bind(53)
            .map_err(|e| alloc::format!("dns bind {e:?}"))?;
        let mut tcp_rx = [0u8; 1408];
        let mut tcp_tx = [0u8; 1024];
        let mut tcp = TcpSocket::new(net, &mut tcp_rx, &mut tcp_tx);
        tcp.set_timeout(Some(READ_LIMIT));
        let started = Instant::now();
        let mut debounce = Debouncer::default();
        let mut packet = [0u8; 512];
        // One buffer for every answer, and `packet` for every request: the loop
        // handles one datagram at a time, and these live in the main task's future.
        let mut reply = [0u8; if dhcp::REPLY_CAP > dns::REPLY_CAP {
            dhcp::REPLY_CAP
        } else {
            dns::REPLY_CAP
        }];
        loop {
            if out.read_byte().is_some() {
                return Ok(End::Back);
            }
            let s = buttons.sample();
            let down = decode(s.bottom, s.side, s.power_pressed);
            if down.is_some() {
                power.activity();
            }
            if debounce.feed(down) == Some(Button::Back) {
                return Ok(End::Back);
            }
            if started.elapsed() > SESSION {
                return Ok(End::TimedOut);
            }
            if let Ok(Ok((n, _))) =
                with_timeout(Duration::from_millis(20), udp.recv_from(&mut packet)).await
                && let Ok(answer) =
                    <&mut [u8; dhcp::REPLY_CAP]>::try_from(&mut reply[..dhcp::REPLY_CAP])
                && let Some(len) = dhcp::reply(&packet[..n], answer)
            {
                let to = IpEndpoint::new(Ipv4Address::BROADCAST.into(), 68);
                let _ = udp.send_to(&reply[..len], to).await;
            }
            // Every name resolves to the X3, so the phone's internet check
            // reaches the setup page and opens it by itself.
            if let Ok(Ok((n, from))) =
                with_timeout(Duration::from_millis(20), resolver.recv_from(&mut packet)).await
                && let Ok(answer) =
                    <&mut [u8; dns::REPLY_CAP]>::try_from(&mut reply[..dns::REPLY_CAP])
                && let Some(len) = dns::reply(&packet[..n], answer)
            {
                let _ = resolver.send_to(&reply[..len], from.endpoint).await;
            }
            match tcp.state() {
                // `accept` puts the socket into Listen and returns when a client
                // connects. Dropping it on the timeout leaves the socket
                // listening, so it is only called again once the socket is closed.
                State::Closed => {
                    let _ = with_timeout(Duration::from_millis(40), tcp.accept(80)).await;
                }
                State::Listen | State::SynReceived | State::SynSent => {
                    Timer::after(Duration::from_millis(40)).await;
                }
                _ => {
                    let saved = serve_one(&mut tcp, &names, epd, sd_cs).await;
                    tcp.close();
                    let _ = with_timeout(Duration::from_secs(2), tcp.flush()).await;
                    tcp.abort();
                    if saved {
                        return Ok(End::Saved);
                    }
                }
            }
        }
    };
    let r = match select(runner.run(), serve).await {
        Either::First(_) => Err("network runner stopped".into()),
        Either::Second(r) => r,
    };
    drop(controller);
    let _ = writeln!(out, "setup: hotspot off");
    r
}

/// Reads one request and answers it. True once new credentials are saved.
async fn serve_one<'d>(
    tcp: &mut TcpSocket<'_>,
    networks: &[alloc::string::String],
    epd: &mut crate::epd::Epd<'d>,
    sd_cs: &mut esp_hal::gpio::Output<'d>,
) -> bool {
    let mut request = [0u8; page::REQUEST_CAP];
    let mut have = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if !matches!(page::parse_request(&request[..have]), Parse::Incomplete) {
            break;
        }
        if have == request.len() || Instant::now() > deadline {
            respond(tcp, 400, "Bad request", "").await;
            return false;
        }
        match with_timeout(READ_LIMIT, tcp.read(&mut request[have..])).await {
            Ok(Ok(n)) if n > 0 => have += n,
            _ => return false,
        }
    }
    let (method, path, body) = match page::parse_request(&request[..have]) {
        Parse::Request { method, path, body } => (method, path, body),
        _ => {
            respond(tcp, 400, "Bad request", "").await;
            return false;
        }
    };
    let mut html = Body {
        bytes: [0; BODY_CAP],
        len: 0,
    };
    match (method, path) {
        // Any path: phones probe addresses such as /hotspot-detect.html or
        // /generate_204, and anything but their expected answer opens the page.
        (Method::Get, _) => {
            let _ = page::page(&mut html, networks.iter().map(|n| n.as_str()), Notice::None);
            respond(
                tcp,
                200,
                "OK",
                core::str::from_utf8(&html.bytes[..html.len]).unwrap_or(""),
            )
            .await;
            false
        }
        (Method::Post, "/save") => match page::form_credentials(body) {
            Ok(c) => match save(epd, sd_cs, c.ssid(), c.psk()) {
                Ok(()) => {
                    respond(tcp, 200, "OK", page::DONE).await;
                    true
                }
                Err(_) => {
                    respond(tcp, 500, "Server error", "The card could not be written.").await;
                    false
                }
            },
            Err(e) => {
                let notice = match e {
                    page::FormError::Name => Notice::Name,
                    page::FormError::Password => Notice::Password,
                    page::FormError::Malformed => Notice::Malformed,
                };
                let _ = page::page(&mut html, networks.iter().map(|n| n.as_str()), notice);
                respond(
                    tcp,
                    400,
                    "Bad request",
                    core::str::from_utf8(&html.bytes[..html.len]).unwrap_or(""),
                )
                .await;
                false
            }
        },
        _ => {
            respond(tcp, 404, "Not found", "Not found").await;
            false
        }
    }
}

async fn respond(tcp: &mut TcpSocket<'_>, status: u16, reason: &str, body: &str) {
    let mut head = Sink::<384> {
        bytes: [0; 384],
        len: 0,
    };
    let _ = write!(
        head,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    );
    for part in [&head.bytes[..head.len], body.as_bytes()] {
        let mut left = part;
        while !left.is_empty() {
            match with_timeout(READ_LIMIT, tcp.write(left)).await {
                Ok(Ok(n)) if n > 0 => left = &left[n..],
                _ => return,
            }
        }
    }
}

/// Adds the network to `WIFI.CFG`. The new file is written whole to `WIFI.NEW`
/// first, so a cut during the write of `WIFI.CFG` leaves a good copy that
/// `wifi::config` falls back to (ADR 025).
fn save<'d>(
    epd: &mut crate::epd::Epd<'d>,
    sd_cs: &mut esp_hal::gpio::Output<'d>,
    ssid: &str,
    psk: &str,
) -> Result<(), &'static str> {
    let mut current = [0u8; 512];
    let list: Option<WifiList<'_>> = wifi::read_saved(epd.bus(), sd_cs, &mut current);
    let mut file = [0u8; 512];
    let n = save_network(list.as_ref(), ssid, psk, &mut file).map_err(|_| "invalid network")?;
    sd::write_x3_file(epd.bus(), sd_cs, "WIFI.NEW", &file[..n])?;
    sd::write_x3_file(epd.bus(), sd_cs, "WIFI.CFG", &file[..n])?;
    let _ = sd::delete_x3_file(epd.bus(), sd_cs, "WIFI.NEW");
    Ok(())
}
