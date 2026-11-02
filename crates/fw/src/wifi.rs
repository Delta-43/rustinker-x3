//! Wi-Fi sessions: join the network from `/X3/WIFI.CFG`, get an address by
//! DHCP, run some work on the link, then leave and switch the radio off.
//!
//! The firmware is blocking; the radio is async. `main` runs on the esp-rtos
//! executor (`#[esp_rtos::main]`), and a session awaits the network runner
//! and its own work together with `select`, then returns to the blocking
//! command loop. (An earlier `block_on` from a plain blocking main stalled
//! before even its timers ran.)
//!
//! Each session builds the radio driver from a reborrow of the `WIFI`
//! peripheral and drops it at the end, which de-initialises Wi-Fi. So the
//! radio is off between sessions and any number of sessions can run per boot.
extern crate alloc;

use crate::{Device, sd, serial::Out, stack};
use core::fmt::Write;
use embassy_futures::select::{Either, select};
use embassy_net::{Config as NetConfig, Ipv4Cidr, Stack, StackResources};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_hal::peripherals::WIFI;
use esp_radio::wifi::{Config, scan::ScanConfig, sta::StationConfig};
use x3_proto::config::{MAX_NETWORKS, Wifi, WifiList};

/// Radio heap: esp-radio's buffers and task stacks. Taken from the stack's
/// share of RAM; see `stack::untouched` for what is left.
pub const HEAP: usize = 96 * 1024;

/// What the join found, for the log.
pub struct Link {
    pub joined_ms: u64,
    pub address_ms: u64,
    pub address: Option<Ipv4Cidr>,
    pub rssi: Option<i32>,
    /// Heap used by the radio once the link is up.
    pub radio_heap: usize,
    /// Which saved network joined: its place in `WIFI.CFG`.
    pub network: usize,
}

/// Reads and parses the saved networks into `buf`. Never prints the file: it
/// holds the network passwords. `WIFI.NEW` is the copy Wi-Fi setup writes
/// before it replaces `WIFI.CFG` (ADR 025); it is used if a power cut left
/// `WIFI.CFG` unreadable.
pub fn config<'b>(
    dev: &mut Device<'_>,
    buf: &'b mut [u8; 512],
) -> Result<WifiList<'b>, &'static str> {
    read_saved(dev.epd.bus(), &mut dev.sd_cs, buf)
        .ok_or("no Wi-Fi is saved: open Device, then press Left for Wi-Fi setup")
}

/// `config` for callers that hold the bus and chip select separately.
pub fn read_saved<'b, 'd>(
    spi: &mut esp_hal::spi::master::Spi<'d, esp_hal::Blocking>,
    cs: &mut esp_hal::gpio::Output<'d>,
    buf: &'b mut [u8; 512],
) -> Option<WifiList<'b>> {
    let mut found = None;
    for name in ["WIFI.CFG", "WIFI.NEW"] {
        if let Ok(n) = sd::read_x3_file(spi, cs, name, buf)
            && WifiList::parse(&buf[..n]).is_ok()
        {
            found = Some(n);
            break;
        }
    }
    WifiList::parse(&buf[..found?]).ok()
}

/// The network Receive tries first (`/X3/WIFI.PIN`), if one is chosen. A
/// missing or unreadable file means automatic.
pub fn chosen<'b>(dev: &mut Device<'_>, buf: &'b mut [u8; 64]) -> Option<&'b str> {
    let n = sd::read_x3_file(dev.epd.bus(), &mut dev.sd_cs, PIN, buf).ok()?;
    x3_proto::config::chosen_network(&buf[..n]).ok().flatten()
}

/// Makes `ssid` the network to try first; `None` goes back to automatic. Not
/// atomic: a torn file does not parse, which reads as automatic.
pub fn choose(dev: &mut Device<'_>, ssid: Option<&str>) -> Result<(), &'static str> {
    let mut file = [0u8; 64];
    let n = match ssid {
        Some(s) => x3_proto::config::write_chosen_network(s, &mut file)
            .map_err(|_| "that network name cannot be saved")?,
        None => 0,
    };
    sd::write_x3_file(dev.epd.bus(), &mut dev.sd_cs, PIN, &file[..n])
}

/// The chosen network's file in `/X3`.
const PIN: &str = "WIFI.PIN";

fn station(w: &Wifi<'_>) -> StationConfig {
    StationConfig::default()
        .with_ssid(w.ssid)
        .with_password(alloc::string::String::from(w.psk))
}

/// Joins, runs `work` with the network up, then leaves. The radio is off
/// again when this returns, whatever happened.
pub async fn online<R>(
    radio: &mut WIFI<'static>,
    wifi: &WifiList<'_>,
    chosen: Option<&str>,
    out: &mut Out<'_>,
    work: impl AsyncFnOnce(Stack<'_>, &Link, &mut Out<'_>) -> R,
) -> Result<R, alloc::string::String> {
    let heap_before = esp_alloc::HEAP.used();
    let (mut controller, interfaces) = esp_radio::wifi::new(radio.reborrow(), Default::default())
        .map_err(|e| alloc::format!("radio init {e:?}"))?;
    let first = wifi.iter().next().ok_or("no saved network")?;
    controller
        .set_config(&Config::Station(station(first)))
        .map_err(|e| alloc::format!("config {e:?}"))?;
    let seed =
        (esp_hal::rng::Rng::new().random() as u64) << 32 | esp_hal::rng::Rng::new().random() as u64;
    let mut resources = StackResources::<3>::new();
    let (net, mut runner) = embassy_net::new(
        interfaces.station,
        NetConfig::dhcpv4(Default::default()),
        &mut resources,
        seed,
    );
    let session = async {
        let t0 = Instant::now();
        // With more than one saved network, look first and join the strongest.
        // A failed scan only costs the ordering.
        let seen: alloc::vec::Vec<(alloc::string::String, i8)> = if wifi.len() > 1 {
            match controller
                .scan_async(&ScanConfig::default().with_max(24))
                .await
            {
                Ok(aps) => aps
                    .iter()
                    .map(|a| {
                        (
                            alloc::string::String::from(a.ssid.as_str()),
                            a.signal_strength,
                        )
                    })
                    .collect(),
                Err(_) => alloc::vec::Vec::new(),
            }
        } else {
            alloc::vec::Vec::new()
        };
        let mut names = [""; MAX_NETWORKS];
        for (slot, w) in names.iter_mut().zip(wifi.iter()) {
            *slot = w.ssid;
        }
        let seen_refs: alloc::vec::Vec<(&str, i8)> =
            seen.iter().map(|(n, r)| (n.as_str(), *r)).collect();
        let (tries, count) = x3_core::setup::join_order(&names[..wifi.len()], &seen_refs, chosen);
        // Several candidates: give each less time, so a missing network does
        // not hold up the next one.
        let limit = Duration::from_secs(if count > 1 { 15 } else { 30 });
        let mut failure = alloc::string::String::new();
        let mut joined = None;
        for (n, &i) in tries[..count].iter().enumerate() {
            let Some(w) = wifi.iter().nth(i) else {
                continue;
            };
            // The name stays off the serial log: it is the owner's home network.
            let signal = seen_refs
                .iter()
                .filter(|(name, _)| *name == w.ssid)
                .map(|(_, r)| *r)
                .max();
            let _ = writeln!(
                out,
                "wifi: try {} of {}: saved network {}{}, scan {}",
                n + 1,
                count,
                i + 1,
                if chosen == Some(w.ssid) {
                    " (chosen)"
                } else {
                    ""
                },
                match (signal, seen.is_empty()) {
                    (Some(r), _) => alloc::format!("saw it at {r} dBm"),
                    (None, true) => alloc::string::String::from("not run"),
                    (None, false) => alloc::string::String::from("did not see it"),
                }
            );
            out.flush();
            if controller.set_config(&Config::Station(station(w))).is_err() {
                failure = "config rejected".into();
                continue;
            }
            match attempt(&mut controller, out, t0, limit).await {
                Ok(()) => {
                    joined = Some(i);
                    break;
                }
                Err(e) => {
                    let _ = writeln!(out, "wifi: saved network {} failed: {e}", i + 1);
                    failure = e;
                }
            }
        }
        let Some(network) = joined else {
            return Err(failure);
        };
        let joined_ms = t0.elapsed().as_millis();
        if with_timeout(Duration::from_secs(20), net.wait_config_up())
            .await
            .is_err()
        {
            let _ = controller.disconnect_async().await;
            return Err("no DHCP address after 20 s".into());
        }
        let link = Link {
            joined_ms,
            address_ms: t0.elapsed().as_millis(),
            address: net.config_v4().map(|c| c.address),
            rssi: controller.rssi().ok(),
            radio_heap: esp_alloc::HEAP.used().saturating_sub(heap_before),
            network,
        };
        let _ = writeln!(
            out,
            "wifi: joined in {} ms, address {:?} after {} ms, RSSI {:?} dBm",
            link.joined_ms, link.address, link.address_ms, link.rssi
        );
        out.flush();
        let r = work(net, &link, out).await;
        let _ = controller.disconnect_async().await;
        Ok(r)
    };
    let r = match select(runner.run(), session).await {
        Either::First(_) => Err("network runner stopped".into()),
        Either::Second(r) => r,
    };
    drop(controller);
    let _ = writeln!(out, "wifi: off");
    r
}

/// One connection attempt, with a heartbeat on the serial line.
async fn attempt(
    controller: &mut esp_radio::wifi::WifiController<'_>,
    out: &mut Out<'_>,
    t0: Instant,
    limit: Duration,
) -> Result<(), alloc::string::String> {
    let heartbeat = async {
        loop {
            Timer::after(Duration::from_secs(2)).await;
            let _ = writeln!(out, "wifi: still joining, {} s", t0.elapsed().as_secs());
            out.flush();
        }
    };
    match select(with_timeout(limit, controller.connect_async()), heartbeat).await {
        Either::First(Ok(Ok(_))) => Ok(()),
        // Keep the reason code and signal only. The full error carries the
        // network name and the router's address, and this text is written to
        // /X3/LOG.TXT, which people paste into bug reports.
        Either::First(Ok(Err(esp_radio::wifi::WifiError::Disconnected(info)))) => {
            Err(alloc::format!(
                "connect refused: {:?} (signal {} dBm)",
                info.reason,
                info.rssi
            ))
        }
        Either::First(Ok(Err(e))) => Err(alloc::format!("connect {e:?}")),
        Either::First(Err(_)) => Err(alloc::format!(
            "connect timed out after {} s",
            limit.as_secs()
        )),
        Either::Second(()) => Err("heartbeat ended".into()),
    }
}

/// Joins one saved network by name and leaves at once: the check the Wi-Fi
/// screen runs when a network is chosen. Returns the signal in dBm.
pub async fn try_network(
    dev: &mut Device<'_>,
    out: &mut Out<'_>,
    ssid: &str,
) -> Result<Option<i32>, alloc::string::String> {
    let mut buf = [0u8; 512];
    let saved = config(dev, &mut buf).map_err(alloc::string::String::from)?;
    let w = saved.find(ssid).ok_or("that network is no longer saved")?;
    // A list of this one network only, so a fallback cannot hide a failure.
    let mut one = [0u8; 512];
    let n = x3_proto::config::save_network(None, w.ssid, w.psk, &mut one)
        .map_err(|_| "cannot read the saved network")?;
    let single = WifiList::parse(&one[..n]).map_err(|_| "cannot read the saved network")?;
    online(
        &mut dev.wifi,
        &single,
        None,
        out,
        async |_net, link, _out| link.rssi,
    )
    .await
}

/// The `wifi` command: join, report the link and RAM, leave.
pub async fn join(dev: &mut Device<'_>, out: &mut Out<'_>) {
    let mut buf = [0u8; 512];
    let wifi = match config(dev, &mut buf) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(out, "wifi: {e}");
            return;
        }
    };
    let mut pin = [0u8; 64];
    let chosen = chosen(dev, &mut pin);
    let r = online(
        &mut dev.wifi,
        &wifi,
        chosen,
        out,
        async |_net, link, out| {
            // Hold the link a moment so buffers reach a steady state.
            Timer::after(Duration::from_millis(500)).await;
            let _ = writeln!(
                out,
                "wifi: heap {} used (radio {}), {} free of {HEAP}; stack {} untouched of {}",
                esp_alloc::HEAP.used(),
                link.radio_heap,
                esp_alloc::HEAP.free(),
                stack::untouched(),
                stack::size()
            );
        },
    )
    .await;
    if let Err(e) = r {
        let _ = writeln!(out, "wifi: {e}");
    }
}
