//! RustInker-X3 firmware: boots from app1, prints chip info on USB serial
//! and takes line commands there (`help` lists them).
#![no_std]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(not(target_os = "none"))]
extern crate std;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(all(target_os = "none", feature = "link-proof"))]
mod link_proof;

#[cfg(target_os = "none")]
mod boot;
#[cfg(target_os = "none")]
mod buttons;
#[cfg(target_os = "none")]
mod capture;
#[cfg(target_os = "none")]
mod epd;
#[cfg(target_os = "none")]
mod home;
#[cfg(target_os = "none")]
mod library;
#[cfg(target_os = "none")]
mod menu;
#[cfg(target_os = "none")]
mod power;
#[cfg(target_os = "none")]
mod receive;
#[cfg(target_os = "none")]
mod rtc;
#[cfg(target_os = "none")]
mod sd;
#[cfg(target_os = "none")]
mod serial;
#[cfg(target_os = "none")]
mod setup;
#[cfg(target_os = "none")]
mod stack;
#[cfg(target_os = "none")]
mod tls;
#[cfg(target_os = "none")]
mod tokens;
#[cfg(target_os = "none")]
mod wifi;

// ESP-IDF's second-stage bootloader expects an app descriptor in the image.
#[cfg(target_os = "none")]
esp_bootloader_esp_idf::esp_app_desc!();

#[cfg(target_os = "none")]
const LINE_MAX: usize = 64;
#[cfg(target_os = "none")]
const HELP: &str = "commands: status, switch, capture on|off, display white|black|checker|corners, \
                    partial, panel sleep, buttons, keys, sd probe [high|low|float], sd test MIB, i2c scan, time, time set YYYY-MM-DD HH:MM:SS, battery, imu, sleep, read, library, check, wifi, receive (or the Receive tile on Home), log, tokens, check, drill TOKEN|off";

/// What the bring-up commands act on.
#[cfg(target_os = "none")]
struct Device<'d> {
    layout: Result<boot::Layout, boot::Error>,
    flash: esp_storage::FlashStorage<'d>,
    epd: epd::Epd<'d>,
    buttons: buttons::Handle,
    sd_cs: esp_hal::gpio::Output<'d>,
    i2c: Option<esp_hal::i2c::master::I2c<'d, esp_hal::Blocking>>,
    /// RTC, light sleep and idle bookkeeping.
    power: power::Power<'d>,
    /// The radio. Each Wi-Fi session borrows it and switches it off after.
    wifi: esp_hal::peripherals::WIFI<'static>,
    /// GPIO13, SD power per the stock-firmware notes; left unconfigured
    /// until `sd probe` picks a level.
    sd_power: esp_hal::gpio::Flex<'d>,
    /// What the glass shows, once a full refresh has run since `init`.
    shown: Option<x3_core::panel::Pattern>,
    epd_ready: bool,
}

#[cfg(target_os = "none")]
#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    use core::fmt::Write;
    use esp_hal::{
        analog::adc::{Adc, AdcCalBasic, AdcConfig, Attenuation},
        gpio::{Flex, Input, InputConfig, Level, Output, OutputConfig, Pull},
        spi::{
            Mode,
            master::{Config as SpiConfig, Spi},
        },
        time::{Duration, Instant, Rate},
    };

    // Retain the integration code in the ELF without running peripheral setup.
    #[cfg(feature = "link-proof")]
    core::hint::black_box(link_proof::check as fn() -> !);

    stack::paint();
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: wifi::HEAP);
    // The scheduler runs from boot: esp-radio needs it, and its executor runs
    // this function. Everything but `wifi` stays blocking.
    {
        let timers = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
        let ints =
            esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
        esp_rtos::start(timers.timer0, ints.software_interrupt0);
    }
    let mut out = serial::Out::new(esp_hal::usb_serial_jtag::UsbSerialJtag::new(
        peripherals.USB_DEVICE,
    ));
    let mut flash = esp_storage::FlashStorage::new(peripherals.FLASH);
    let layout = boot::Layout::read(&mut flash);

    // The SD card shares the SPI clock and data lines; keep it deselected.
    let sd_cs = Output::new(peripherals.GPIO12, Level::High, OutputConfig::default());
    // 16 MHz: the reference's earlier, more conservative clock for bring-up.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(16))
            .with_mode(Mode::_0),
    );
    let Ok(spi) = spi else {
        loop {
            let _ = writeln!(out, "error: SPI config");
            out.flush();
            esp_hal::delay::Delay::new().delay_millis(2000);
        }
    };
    let epd = epd::Epd::new(
        spi.with_sck(peripherals.GPIO8)
            .with_mosi(peripherals.GPIO10)
            .with_miso(peripherals.GPIO7),
        Output::new(peripherals.GPIO21, Level::High, OutputConfig::default()),
        Output::new(peripherals.GPIO4, Level::High, OutputConfig::default()),
        Output::new(peripherals.GPIO5, Level::High, OutputConfig::default()),
        Input::new(peripherals.GPIO6, InputConfig::default()),
    );
    let mut adc_config = AdcConfig::new();
    let bottom =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(peripherals.GPIO1, Attenuation::_11dB);
    let side =
        adc_config.enable_pin_with_cal::<_, AdcCalBasic<_>>(peripherals.GPIO2, Attenuation::_11dB);
    buttons::install(buttons::Buttons::new(
        Adc::new(peripherals.ADC1, adc_config),
        bottom,
        side,
        Input::new(
            peripherals.GPIO3,
            InputConfig::default().with_pull(Pull::Up),
        ),
    ));
    let buttons = buttons::Handle;
    let mut dev = Device {
        layout,
        flash,
        epd,
        buttons,
        sd_cs,
        sd_power: Flex::new(peripherals.GPIO13),
        power: power::Power::new(peripherals.LPWR),
        wifi: peripherals.WIFI,
        // 100 kHz for bring-up; the devices allow 400 kHz.
        i2c: esp_hal::i2c::master::I2c::new(
            peripherals.I2C0,
            esp_hal::i2c::master::Config::default().with_frequency(Rate::from_khz(100)),
        )
        .ok()
        .map(|i| i.with_sda(peripherals.GPIO20).with_scl(peripherals.GPIO0)),
        shown: None,
        epd_ready: false,
    };

    if let Some(i2c) = dev.i2c.as_mut() {
        // Unused for now; the previous firmware may leave it running.
        rtc::imu_off(i2c);
    }
    let rev = esp_hal::efuse::chip_revision();
    let reset = esp_hal::system::reset_reason();
    let wake = esp_hal::system::wakeup_cause();
    // The long timer wake (see `deep_sleep`) only keeps the button wake
    // working: go straight back to sleep, the sleep screen still showing.
    if matches!(wake, esp_hal::system::SleepSource::Timer) {
        deep_sleep_with(&mut dev, &mut out, false);
    }
    let mut line = [0u8; LINE_MAX];
    let mut len = 0usize;
    let mut overflow = false;
    let mut count: u32 = 0;
    let mut commands: u32 = 0;
    let mut next = Instant::now();
    // Home owns the buttons while idle; a press held through a session does
    // not act twice.
    // Primed with what is held now: the Power press that woke the chip must
    // not read as a fresh press (which would send it straight back to sleep).
    let held = dev.buttons.sample();
    let mut keys = x3_core::buttons::Debouncer::primed(x3_core::buttons::decode(
        held.bottom,
        held.side,
        held.power_pressed,
    ));
    let mut home = home::Home::new();
    loop {
        let s = dev.buttons.sample();
        let down = x3_core::buttons::decode(s.bottom, s.side, s.power_pressed);
        if down.is_some() {
            // Seen down on a sleep sample: stay awake to read the press.
            dev.power.activity();
        }
        let pressed = keys.feed(down);
        home.tick(&mut dev, &mut out, pressed).await;
        if Instant::now() >= next {
            let uptime = Instant::now().duration_since_epoch();
            let _ = writeln!(
                out,
                "x3 hello {count}: {} {} on ESP32-C3 rev v{}.{}, up {} ms, boot {reset:?}/{wake:?}, rx {} {:02x?}, cmd {commands}",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                rev.major,
                rev.minor,
                uptime.as_millis(),
                out.received(),
                out.last(),
            );
            out.flush();
            count = count.wrapping_add(1);
            next = Instant::now() + Duration::from_millis(2000);
        }
        while let Some(byte) = out.read_byte() {
            dev.power.activity();
            if byte != b'\n' && byte != b'\r' {
                match line.get_mut(len) {
                    Some(slot) => {
                        *slot = byte;
                        len += 1;
                    }
                    None => overflow = true,
                }
                continue;
            }
            let command = if overflow { &[][..] } else { &line[..len] };
            if !command.is_empty() || overflow {
                commands = commands.wrapping_add(1);
                if command == b"wifi" {
                    wifi::join(&mut dev, &mut out).await;
                } else if command == b"receive" {
                    receive::receive(&mut dev, &mut out).await;
                } else {
                    run(command, &mut dev, &mut out);
                }
                // A command may have used the screen; Home returns on a key.
                home.invalidate();
            }
            len = 0;
            overflow = false;
        }
        dev.nap(&mut out, 10);
    }
}

#[cfg(target_os = "none")]
impl Device<'_> {
    /// Waits about `ms` for the next button poll. Without a USB host this
    /// light-sleeps, turns the panel's charge pump off after a short idle,
    /// samples the gauge for `/X3/LOG.TXT` and deep-sleeps after a long one.
    fn nap(&mut self, out: &mut serial::Out<'_>, ms: u32) {
        let host = self.power.host();
        // Charge pump off after a short idle on USB too: a static image held
        // under drive for hours left a retained ghost and speckle that only
        // black/white cycles cleared (photos, 2026-11-02).
        if self.power.idle_us() > power::PANEL_OFF_US && self.epd.powered() {
            let _ = self.epd.power_off();
        }
        if host {
            if self.power.stretch.is_some() {
                self.log_idle(out, "usb");
            }
        } else {
            let now = self.power.now_us();
            if self.power.stretch.is_none() {
                let mah = self.i2c.as_mut().and_then(|i| rtc::gauge(i).remaining_mah);
                self.power.stretch = Some(power::Stretch {
                    start_us: now,
                    next_sample_us: now,
                    sum_ma: 0,
                    samples: 0,
                    start_mah: mah,
                    naps: 0,
                });
            }
            let idle = self.power.idle_us();
            // (Charge pump off is above, for USB and battery alike. Controller
            // deep sleep while running turned a resting image grey after
            // ~30 s, 2026-10-27, so only the pump goes off.)
            if let (Some(s), Some(i2c)) = (self.power.stretch.as_mut(), self.i2c.as_mut())
                && now >= s.next_sample_us
            {
                if let Some(ma) = rtc::current_ma(i2c) {
                    s.sum_ma += i64::from(ma);
                    s.samples += 1;
                }
                s.next_sample_us = now + power::SAMPLE_US;
            }
            if idle > power::DEEP_AFTER_US {
                self.log_idle(out, "deep");
                deep_sleep(self, out);
            }
        }
        self.power.nap(ms, host);
    }

    /// Appends one `idle` line for the stretch of sleeping that just ended.
    fn log_idle(&mut self, out: &mut serial::Out<'_>, end: &str) {
        use core::fmt::Write;
        let Some(s) = self.power.stretch.take() else {
            return;
        };
        let secs = self.power.now_us().saturating_sub(s.start_us) / 1_000_000;
        let mah = self.i2c.as_mut().and_then(|i| rtc::gauge(i).remaining_mah);
        let when = self.i2c.as_mut().and_then(rtc::read);
        let mut line = Line::default();
        let _ = match when {
            Some(t) => write!(line, "{t}"),
            None => write!(line, "-"),
        };
        let _ = write!(line, " idle s={secs} naps={} samples={}", s.naps, s.samples);
        if s.samples > 0 {
            let _ = write!(line, " mean_ma={}", s.sum_ma / s.samples);
        }
        let _ = writeln!(line, " mah={:?}>{:?} end={end}", s.start_mah, mah);
        let _ = write!(out, "power: {}", line.as_str());
        if let Err(e) = sd::append_x3_file(
            self.epd.bus(),
            &mut self.sd_cs,
            "LOG.TXT",
            line.as_str().as_bytes(),
            12 * 1024,
        ) {
            let _ = writeln!(out, "power: /X3/LOG.TXT: {e}");
        }
    }
}

/// A short log line built without the heap.
#[cfg(target_os = "none")]
struct Line {
    buf: [u8; 192],
    len: usize,
}

#[cfg(target_os = "none")]
impl Default for Line {
    fn default() -> Self {
        Self {
            buf: [0; 192],
            len: 0,
        }
    }
}

#[cfg(target_os = "none")]
impl Line {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

#[cfg(target_os = "none")]
impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(target_os = "none")]
fn run(command: &[u8], dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    use x3_core::panel::Pattern;
    // `drill TOKEN` and `drill off` swap the companion for a test one.
    if let Some(arg) = command.strip_prefix(b"drill ") {
        receive::drill(dev, out, core::str::from_utf8(arg).unwrap_or(""));
        return;
    }
    match command {
        b"sleep" => deep_sleep(dev, out),
        b"log" => receive::print_log(dev, out),
        b"tokens" => tokens::browse(dev, out),
        b"check" => library::check(dev, out),
        b"library" => {
            if ensure_epd(dev, out) {
                match reader_style() {
                    Some(style) => {
                        let bar = home::Bar::read(dev);
                        library::run(dev, out, &style, &bar, library::Open::List)
                    }
                    None => {
                        let _ = writeln!(out, "library: bad built-in font");
                    }
                }
            }
        }
        b"read" => {
            if ensure_epd(dev, out) {
                read(dev, out);
            }
            let _ = writeln!(
                out,
                "read: stack {} untouched of {}",
                stack::untouched(),
                stack::size()
            );
        }
        b"help" => {
            let _ = writeln!(out, "{HELP}");
        }
        b"status" => status(dev, out),
        b"switch" => switch_firmware(dev, out),
        b"capture on" => {
            capture::set(true);
            let _ = writeln!(out, "capture: on");
        }
        b"capture off" => {
            capture::set(false);
            let _ = writeln!(out, "capture: off");
        }
        b"partial" => {
            if ensure_epd(dev, out) {
                partial(dev, out);
            }
        }
        b"buttons" => watch_buttons(dev, out),
        b"keys" => watch_keys(dev, out),
        b"panel sleep" => {
            let r = dev.epd.sleep();
            dev.epd_ready = false;
            dev.shown = None;
            let _ = writeln!(out, "panel sleep: {r:?}");
        }
        b"sd probe" => sd_probe(dev, out, Some(esp_hal::gpio::Level::High)),
        b"sd probe high" => sd_probe(dev, out, Some(esp_hal::gpio::Level::High)),
        b"sd probe low" => sd_probe(dev, out, Some(esp_hal::gpio::Level::Low)),
        b"sd probe float" => sd_probe(dev, out, None),
        b"i2c scan" | b"time" | b"battery" | b"imu" => match dev.i2c.as_mut() {
            Some(i2c) if command == b"imu" => rtc::imu(i2c, out),
            Some(i2c) if command == b"time" => rtc::show(i2c, out),
            Some(i2c) if command == b"battery" => rtc::battery(i2c, out),
            Some(i2c) => rtc::scan(i2c, out),
            None => {
                let _ = writeln!(out, "error: I2C config");
            }
        },
        _ if command.starts_with(b"time set ") => {
            match (
                dev.i2c.as_mut(),
                x3_core::rtc::DateTime::parse(&command[9..]),
            ) {
                (Some(i2c), Some(t)) => {
                    let r = rtc::set(i2c, &t);
                    let _ = writeln!(out, "time set {t}: {r:?}");
                    rtc::show(i2c, out);
                }
                (None, _) => {
                    let _ = writeln!(out, "error: I2C config");
                }
                (_, None) => {
                    let _ = writeln!(out, "time set: use YYYY-MM-DD HH:MM:SS");
                }
            }
        }
        _ if command.starts_with(b"sd test ") => {
            let mib = core::str::from_utf8(&command[8..])
                .ok()
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0);
            let _ = writeln!(out, "sd test {mib} MiB in /X3TEST/");
            out.flush();
            let r = sd::write_test(dev.epd.bus(), &mut dev.sd_cs, mib, out);
            let _ = writeln!(out, "sd test: {r:?}");
        }
        _ => match command.strip_prefix(b"display ").and_then(Pattern::parse) {
            Some(p) => {
                if ensure_epd(dev, out) {
                    let r = dev.epd.full(&p);
                    dev.shown = r.is_ok().then_some(p);
                    let _ = writeln!(out, "display {p:?} (full): {r:?}");
                }
            }
            None => {
                let _ = writeln!(out, "unknown command; {HELP}");
            }
        },
    }
    out.flush();
}

/// Initialises the panel on first use and reports the BUSY level around it.
#[cfg(target_os = "none")]
fn ensure_epd(dev: &mut Device<'_>, out: &mut serial::Out<'_>) -> bool {
    use core::fmt::Write;
    if dev.epd_ready {
        return true;
    }
    let before = dev.epd.busy_high();
    let r = dev.epd.init();
    let _ = writeln!(
        out,
        "panel init: {r:?} (BUSY high before {before}, after {})",
        dev.epd.busy_high()
    );
    out.flush();
    dev.epd_ready = r.is_ok();
    dev.shown = None;
    dev.epd_ready
}

/// Eight fast refreshes moving a block across the panel, one report each.
#[cfg(target_os = "none")]
fn partial(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    use x3_core::panel::Pattern;
    if dev.shown.is_none() {
        let r = dev.epd.full(&Pattern::White);
        let _ = writeln!(out, "partial: white base (full): {r:?}");
        out.flush();
        if r.is_err() {
            return;
        }
        dev.shown = Some(Pattern::White);
    }
    for step in 0..8u8 {
        let Some(prev) = dev.shown else { return };
        let next = Pattern::Block(step);
        let r = dev.epd.fast(&prev, &next);
        let _ = writeln!(out, "partial step {step}: {r:?}");
        out.flush();
        match r {
            Ok(_) => dev.shown = Some(next),
            Err(_) => {
                dev.shown = None;
                return;
            }
        }
    }
}

/// Prints raw readings whenever one moves by more than 10 counts or the
/// power button changes, so each press shows as a line. Runs until any byte
/// arrives on serial (`x3cmd.py buttons 0` sends one on Ctrl+C).
#[cfg(target_os = "none")]
fn watch_buttons(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    use esp_hal::time::Instant;
    let delay = esp_hal::delay::Delay::new();
    let start = Instant::now();
    let mut last = dev.buttons.sample();
    let _ = writeln!(
        out,
        "buttons: idle {last:?}; press buttons, send any line to stop"
    );
    out.flush();
    let moved = |a: Option<u16>, b: Option<u16>| match (a, b) {
        (Some(a), Some(b)) => a.abs_diff(b) > 10,
        (a, b) => a.is_some() != b.is_some(),
    };
    while out.read_byte().is_none() {
        let s = dev.buttons.sample();
        if moved(s.bottom, last.bottom)
            || moved(s.side, last.side)
            || s.power_pressed != last.power_pressed
        {
            let _ = writeln!(
                out,
                "{:>5} ms bottom {:?} side {:?} power {}",
                start.elapsed().as_millis(),
                s.bottom,
                s.side,
                if s.power_pressed { "down" } else { "up" }
            );
            out.flush();
            last = s;
        }
        delay.delay_millis(20);
    }
    let _ = writeln!(out, "buttons: done");
}

/// Decoded, debounced presses (one line each) until any byte arrives.
#[cfg(target_os = "none")]
fn watch_keys(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    use esp_hal::time::Instant;
    use x3_core::buttons::{Debouncer, decode};
    let delay = esp_hal::delay::Delay::new();
    let start = Instant::now();
    let mut debounce = Debouncer::default();
    let mut presses: u32 = 0;
    let _ = writeln!(out, "keys: press buttons, send any line to stop");
    out.flush();
    while out.read_byte().is_none() {
        let s = dev.buttons.sample();
        if let Some(b) = debounce.feed(decode(s.bottom, s.side, s.power_pressed)) {
            presses += 1;
            let _ = writeln!(
                out,
                "{:>6} ms press {presses}: {b:?}",
                start.elapsed().as_millis()
            );
            out.flush();
        }
        delay.delay_millis(20);
    }
    let _ = writeln!(out, "keys: done, {presses} presses");
}

/// Read-only SD check with GPIO13 driven high, low or left floating.
#[cfg(target_os = "none")]
fn sd_probe(dev: &mut Device<'_>, out: &mut serial::Out<'_>, power: Option<esp_hal::gpio::Level>) {
    use core::fmt::Write;
    match power {
        Some(level) => {
            dev.sd_power.set_level(level);
            dev.sd_power.set_output_enable(true);
        }
        None => dev.sd_power.set_output_enable(false),
    }
    // Give the card's supply time to settle after a change.
    esp_hal::delay::Delay::new().delay_millis(50);
    let _ = writeln!(out, "sd probe, GPIO13 {power:?}");
    out.flush();
    let r = sd::probe(dev.epd.bus(), &mut dev.sd_cs, out);
    let _ = writeln!(out, "sd probe: {r:?}");
}

/// Panel to deep sleep (the image stays), then the chip to deep sleep. Wakes
/// on the power button (GPIO3 low) or, as a safety net, after 2 minutes.
/// Waking restarts the chip; the hello line then shows the wake cause.
/// Sleep now, from Home's Power key.
#[cfg(target_os = "none")]
fn sleep_now(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    deep_sleep(dev, out);
}

#[cfg(target_os = "none")]
fn deep_sleep(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    deep_sleep_with(dev, out, true);
}

/// Deep sleep; `screen` draws the sleep screen first (false when a timer
/// wake goes straight back to sleep with it still showing).
#[cfg(target_os = "none")]
fn deep_sleep_with(dev: &mut Device<'_>, out: &mut serial::Out<'_>, screen: bool) {
    use core::fmt::Write;
    use esp_hal::rtc_cntl::sleep::{RtcioWakeupSource, TimerWakeupSource, WakeupLevel};
    // A Power press that sent us here must be released first, or the held
    // button wakes the chip again at once.
    while dev.buttons.sample().power_pressed {
        esp_hal::delay::Delay::new().delay_millis(20);
    }
    if screen {
        sleep_screen(dev, out);
    }
    if dev.epd_ready {
        let r = dev.epd.sleep();
        let _ = writeln!(out, "sleep: panel {r:?}");
    }
    // On USB the ESP32-C3 hangs entering deep sleep (2026-11-02: no
    // wake until unplugged; deep sleep on battery works). Stand by instead:
    // poll Power every 100 ms and restart on a press, as a wake would. If
    // USB goes away meanwhile, fall through to deep sleep.
    if dev.power.host() {
        let _ = writeln!(out, "sleep: on USB, standing by; press Power to wake");
        out.flush();
        loop {
            esp_hal::delay::Delay::new().delay_millis(100);
            if dev.buttons.sample().power_pressed {
                esp_hal::system::software_reset();
            }
            if !dev.power.host() {
                break;
            }
        }
    }
    let _ = writeln!(out, "sleep: deep sleep now; wake with the power button");
    out.flush();
    esp_hal::delay::Delay::new().delay_millis(300);
    // SAFETY: GPIO3 belongs to the button reader, which never runs again:
    // deep sleep does not return, and waking restarts the chip.
    let mut power = unsafe { esp_hal::peripherals::GPIO3::steal() };
    let mut pins: [(&mut dyn esp_hal::gpio::RtcPinWithResistors, WakeupLevel); 1] =
        [(&mut power, WakeupLevel::Low)];
    let button = RtcioWakeupSource::new(&mut pins);
    // The button alone did not wake the chip (2026-11-02); with a
    // timer beside it, as in the 2026-10-27 test, it does. The timer is
    // long, and a timer wake goes straight back to sleep (see `main`).
    let timer = TimerWakeupSource::new(core::time::Duration::from_secs(6 * 3600));
    dev.power.rtc.sleep_deep(&[&timer, &button]);
}

/// The picture the X3 keeps while asleep: mostly white, no black blocks, so
/// nothing is held that could ghost. One clean cycle first, then a full
/// refresh, so the image is settled before the panel powers down.
#[cfg(target_os = "none")]
fn sleep_screen(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    if sleep_picture(dev, out) {
        return;
    }
    sleep_text(dev, out);
}

/// The picture chosen in Pictures (Confirm on a picture), if there is one
/// and it is still on the card. Separate from `sleep_text` so only one
/// 52 KB frame is on the stack at a time.
#[cfg(target_os = "none")]
#[inline(never)]
fn sleep_picture(dev: &mut Device<'_>, out: &mut serial::Out<'_>) -> bool {
    use core::fmt::Write;
    let mut buf = [0u8; 50];
    let Some(item) = tokens::sleep_picture(dev, &mut buf) else {
        return false;
    };
    let mut id = [0u8; 50];
    let len = item.len();
    id[..len].copy_from_slice(item.as_bytes());
    let item = core::str::from_utf8(&id[..len]).unwrap_or("");
    if !ensure_epd(dev, out) {
        return false;
    }
    let _ = dev.epd.clean(1);
    let r = tokens::draw(dev, out, item);
    let _ = writeln!(out, "sleep: picture {item}: {r:?}");
    r.is_ok()
}

/// The text sleep screen: "Sleeping", the article being read, Press Power.
#[cfg(target_os = "none")]
#[inline(never)]
fn sleep_text(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    extern crate alloc;
    use x3_core::reader::{FRAME_LEN, Frame};
    let Some(style) = reader_style() else {
        return;
    };
    if !ensure_epd(dev, out) {
        return;
    }
    let bar = home::Bar::read(dev);
    let summary = library::summary(dev);
    let mut body = alloc::string::String::from("Press Power to wake.");
    if let Some(h) = &summary.hero {
        let _ = write!(body, "\n\nReading: {}\n{}% read", h.title(), h.percent);
    }
    let _ = dev.epd.clean(1);
    let mut buf = [0u8; FRAME_LEN];
    let mut frame = Frame(&mut buf);
    x3_core::gui::message(
        &mut frame,
        &style,
        &bar.status(),
        "Sleeping",
        &body,
        x3_core::gui::Keys::NONE,
    );
    capture::send(out, "bw", &frame);
    let r = dev.epd.full(&frame);
    let _ = writeln!(out, "sleep: screen {r:?}");
}

#[cfg(target_os = "none")]
static BODY_FONT: &[u8] = include_bytes!("../../../assets/fonts/literata-regular-24.x3f");
#[cfg(target_os = "none")]
static HEADING_FONT: &[u8] = include_bytes!("../../../assets/fonts/literata-bold-28.x3f");
#[cfg(target_os = "none")]
static SAMPLE_ARTICLE: &str = include_str!("../../../assets/sample-article.txt");
/// A full refresh every this many page turns clears fast-refresh ghosting.
#[cfg(target_os = "none")]
const FULL_EVERY: u32 = 10;

/// The reader's fonts and margins (ADR 020).
#[cfg(target_os = "none")]
fn reader_style() -> Option<x3_core::reader::Style<'static>> {
    use x3_core::{font::Font, reader::Style};
    Some(Style {
        body: Font::parse(BODY_FONT).ok()?,
        heading: Font::parse(HEADING_FONT).ok()?,
        margin_x: 20,
        margin_top: 24,
        margin_bottom: 48,
    })
}

/// Reads the built-in sample.
#[cfg(target_os = "none")]
fn read(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    let Some(style) = reader_style() else {
        let _ = writeln!(out, "read: bad built-in font");
        return;
    };
    let mut frame = [0u8; x3_core::reader::FRAME_LEN];
    let end = page_loop(
        dev,
        out,
        &mut frame,
        &Text::Static(SAMPLE_ARTICLE),
        &style,
        0,
    );
    let _ = writeln!(out, "read: stopped at {end:?}");
}

/// Where the reader takes an article's text from.
#[cfg(target_os = "none")]
pub enum Text {
    /// Built into the firmware.
    Static(&'static str),
    /// On the card: `len` bytes of `key`, from byte `start`.
    Card {
        key: x3_core::storage::FileKey,
        start: usize,
        len: usize,
    },
}

#[cfg(target_os = "none")]
impl Text {
    fn len(&self) -> usize {
        match self {
            Text::Static(t) => t.len(),
            Text::Card { len, .. } => *len,
        }
    }
}

/// The reader's window source: reads go to the card when needed.
#[cfg(target_os = "none")]
struct Src<'x, 'd> {
    dev: &'x mut Device<'d>,
    text: &'x Text,
}

#[cfg(target_os = "none")]
impl x3_core::window::Source for Src<'_, '_> {
    fn len(&self) -> usize {
        self.text.len()
    }

    fn read(&mut self, at: usize, out: &mut [u8]) -> Result<usize, x3_core::Error> {
        use x3_core::storage::Store;
        let len = self.text.len();
        if at >= len {
            return Ok(0);
        }
        let n = out.len().min(len - at);
        match *self.text {
            Text::Static(t) => {
                out[..n].copy_from_slice(&t.as_bytes()[at..at + n]);
                Ok(n)
            }
            Text::Card { key, start, .. } => {
                sd::with_data(self.dev.epd.bus(), &mut self.dev.sd_cs, |s| {
                    s.read(key, start + at, &mut out[..n])
                        .map_err(|_| "read failed")
                })
                .map_err(|_| x3_core::Error::Storage)
            }
        }
    }
}

/// The reader's text window (ADR 027): longer articles are read from the
/// card a window at a time.
#[cfg(target_os = "none")]
/// Text window sizes to try, largest first. The window comes from the heap:
/// the radio uses it only during a Receive, so while reading it is idle,
/// and taking the window from it leaves the reader ~48 KB more stack
/// (audit, 2026-11-02: 9.8 KB left with the window on the stack).
const WINDOWS: [usize; 3] = [48 * 1024, 32 * 1024, 16 * 1024];

/// Shows `text` from `start`: Right/Down next page, Left/Up previous,
/// Confirm full refresh, Back (or any serial input) leaves. Returns the
/// offset of the page on screen when leaving, `None` on a display or read
/// error. Prints per-turn timings.
#[cfg(target_os = "none")]
fn page_loop(
    dev: &mut Device<'_>,
    out: &mut serial::Out<'_>,
    buf: &mut [u8; x3_core::reader::FRAME_LEN],
    text: &Text,
    style: &x3_core::reader::Style<'_>,
    start: usize,
) -> Option<usize> {
    use core::fmt::Write;
    use esp_hal::time::Instant;
    use x3_core::{
        buttons::{Button, Debouncer, decode},
        reader::Frame,
        window::Window,
    };
    extern crate alloc;
    let style = *style;
    let total = text.len();
    // Never panics on a full or fragmented heap: try smaller windows.
    let mut window_buf = alloc::vec::Vec::new();
    for size in WINDOWS {
        if window_buf.try_reserve_exact(size).is_ok() {
            window_buf.resize(size, 0u8);
            break;
        }
    }
    if window_buf.is_empty() {
        let _ = writeln!(out, "read: no memory for the text window");
        return None;
    }
    let _ = writeln!(out, "read: window {} KB", window_buf.len() / 1024);
    let mut window = match Window::new(&mut window_buf, total) {
        Ok(w) => w,
        Err(e) => {
            let _ = writeln!(out, "read: {e:?}");
            return None;
        }
    };
    let mut frame = Frame(buf);
    let mut offset = match window.settle(&mut Src { dev, text }, start) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(out, "read: bad text {e:?}");
            return None;
        }
    };
    let mut turns = 0u32;
    let mut full = true;
    let mut debounce = Debouncer::default();
    dev.shown = None;
    let _ = writeln!(
        out,
        "read: {total} bytes; Right/Down next, Left/Up back, Confirm full refresh, Back exits"
    );
    out.flush();
    loop {
        // Draw the page at `offset`.
        let t0 = Instant::now();
        let base = window.base();
        let next = match window.render(&mut Src { dev, text }, &mut frame, &style, offset) {
            Ok(n) => n,
            Err(e) => {
                let _ = writeln!(out, "read: render {e:?}");
                return None;
            }
        };
        let render_ms = t0.elapsed().as_millis();
        if window.base() != base {
            let _ = writeln!(out, "read: window moved to {}", window.base());
        }
        // The controller may have gone to deep sleep while idle.
        if !ensure_epd(dev, out) {
            return None;
        }
        capture::send(out, "bw", &frame);
        let r = if full {
            dev.epd.full(&frame)
        } else {
            dev.epd.turn(&frame)
        };
        let _ = writeln!(
            out,
            "read: page at {offset}/{total} ({}): render {render_ms} ms, panel {r:?}",
            if full { "full" } else { "fast" }
        );
        out.flush();
        if r.is_err() {
            return None;
        }
        full = false;
        // Wait for a page-turn key.
        let target = loop {
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
                Some(Button::Right | Button::Down) if next < total => {
                    turns += 1;
                    break Some(next);
                }
                Some(Button::Left | Button::Up) if offset > 0 => {
                    let t = Instant::now();
                    let p = match window.previous(&mut Src { dev, text }, &style, offset) {
                        Ok(p) => p,
                        Err(e) => {
                            let _ = writeln!(out, "read: previous {e:?}");
                            return None;
                        }
                    };
                    let _ = writeln!(
                        out,
                        "read: previous page found in {} ms",
                        t.elapsed().as_millis()
                    );
                    turns += 1;
                    break Some(p);
                }
                Some(Button::Confirm) => {
                    full = true;
                    break Some(offset);
                }
                Some(Button::Back) => break None,
                _ => {}
            }
            dev.nap(out, 20);
        };
        let Some(t) = target else {
            let _ = writeln!(out, "read: done after {turns} turns");
            // Leaving from the last page means the article is finished: save
            // the end of the text, so the library shows "read" and eviction
            // can take it first (ADR 023). Opening it again starts over.
            return Some(if next >= total { total } else { offset });
        };
        if t != offset && turns.is_multiple_of(FULL_EVERY) {
            full = true;
        }
        offset = t;
    }
}

#[cfg(target_os = "none")]
fn status(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    let layout = match &dev.layout {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(out, "error: partition layout: {e:?}");
            return;
        }
    };
    match layout.entries(&mut dev.flash) {
        Ok(entries) => {
            for (i, e) in entries.iter().enumerate() {
                match e {
                    Some(e) => {
                        let _ = writeln!(
                            out,
                            "otadata[{i}]: seq {}, state {:#x}, crc {}",
                            e.seq,
                            e.state,
                            if e.crc_ok { "ok" } else { "BAD" }
                        );
                    }
                    None => {
                        let _ = writeln!(out, "otadata[{i}]: blank");
                    }
                }
            }
            match layout.booted_slot(&entries) {
                Some(s) => {
                    let _ = writeln!(out, "boots: ota_{s}");
                }
                None => {
                    let _ = writeln!(out, "boots: first app");
                }
            }
        }
        Err(e) => {
            let _ = writeln!(out, "error: {e:?}");
        }
    }
}

#[cfg(target_os = "none")]
fn switch_firmware(dev: &mut Device<'_>, out: &mut serial::Out<'_>) {
    use core::fmt::Write;
    let layout = match &dev.layout {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(out, "error: partition layout: {e:?}");
            return;
        }
    };
    let _ = writeln!(
        out,
        "switch firmware: switching to ota_{}",
        boot::OTHER_SLOT
    );
    out.flush();
    match layout.switch_to(&mut dev.flash, boot::OTHER_SLOT) {
        Ok(()) => {
            let _ = writeln!(out, "otadata written and verified; restarting");
            out.flush();
            esp_hal::delay::Delay::new().delay_millis(200);
            esp_hal::system::software_reset();
        }
        Err(e) => {
            let _ = writeln!(out, "error: {e:?}; nothing restarted");
        }
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    use core::fmt::Write;
    // Say where, straight into the USB serial FIFO (the normal writer
    // belongs to the main loop), then restart rather than hang silently.
    let mut line = Line::default();
    let _ = match info.location() {
        Some(l) => writeln!(line, "\npanic at {}:{}; restarting", l.file(), l.line()),
        None => writeln!(line, "\npanic; restarting"),
    };
    serial::panic_write(line.as_str().as_bytes());
    esp_hal::delay::Delay::new().delay_millis(500);
    esp_hal::system::software_reset()
}
