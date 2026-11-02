//! Idle power. Waiting for a button used to spin the CPU at
//! 30–60 mA. Now, while no USB host is connected, each wait is a light
//! sleep woken by a timer, so the button ladders are still polled (Back,
//! Confirm and Left never pull their line low enough for a GPIO wake).
//! With a host connected nothing changes: the USB serial port stops during
//! light sleep, and bring-up needs it.
//!
//! Time comes from the RTC slow clock, which keeps running in light sleep;
//! the system timer behind `esp_hal::time::Instant` does not.
use core::time::Duration;
use esp_hal::{
    delay::Delay,
    peripherals::{LPWR, USB_DEVICE},
    rtc_cntl::{Rtc, sleep::TimerWakeupSource},
};

/// One light sleep. A button seen down on a wake keeps the chip awake, so
/// the rest of the press is read every 10 ms; a tap over ~50 ms counts
/// (80 ms missed quick taps, 2026-11-01).
const SLEEP_MS: u64 = 50;
/// USB start-of-frame comes every 1 ms; none for this long means no host.
const HOST_GRACE_US: u64 = 500_000;
/// Stay awake this long after a press: snappy page turns, and a way back to
/// USB (press a button after plugging in so the host can enumerate).
const AWAKE_US: u64 = 1_500_000;
/// Panel charge pump off after this long without a press.
pub const PANEL_OFF_US: u64 = 3_000_000;
/// Deep sleep (power-button wake) after this long without a press.
pub const DEEP_AFTER_US: u64 = 600_000_000;
/// Gauge current sample interval while idle on battery.
pub const SAMPLE_US: u64 = 10_000_000;

/// Gauge readings over one stretch of sleeping, for `/X3/LOG.TXT`.
pub struct Stretch {
    pub start_us: u64,
    pub next_sample_us: u64,
    pub sum_ma: i64,
    pub samples: i64,
    pub start_mah: Option<u16>,
    pub naps: u32,
}

pub struct Power<'d> {
    pub rtc: Rtc<'d>,
    frame: u16,
    host_seen_us: u64,
    active_us: u64,
    pub stretch: Option<Stretch>,
}

impl<'d> Power<'d> {
    pub fn new(lpwr: LPWR<'d>) -> Self {
        let rtc = Rtc::new(lpwr);
        let now = rtc.time_since_power_up().as_micros();
        Self {
            rtc,
            frame: 0,
            // Treat boot as host activity: a host gets time to enumerate.
            host_seen_us: now,
            active_us: now,
            stretch: None,
        }
    }

    pub fn now_us(&self) -> u64 {
        self.rtc.time_since_power_up().as_micros()
    }

    /// A USB host is sending start-of-frame packets.
    pub fn host(&mut self) -> bool {
        let frame = USB_DEVICE::regs()
            .fram_num()
            .read()
            .sof_frame_index()
            .bits();
        let now = self.now_us();
        if frame != self.frame {
            self.frame = frame;
            self.host_seen_us = now;
        }
        now.saturating_sub(self.host_seen_us) < HOST_GRACE_US
    }

    /// A press or a command: restarts the idle timers.
    pub fn activity(&mut self) {
        self.active_us = self.now_us();
    }

    pub fn idle_us(&self) -> u64 {
        self.now_us().saturating_sub(self.active_us)
    }

    /// Waits about `ms`: a busy delay with a host or just after a press, a
    /// light sleep otherwise. Returns whether it slept.
    pub fn nap(&mut self, ms: u32, host: bool) -> bool {
        if host || self.idle_us() < AWAKE_US {
            Delay::new().delay_millis(ms);
            return false;
        }
        let timer = TimerWakeupSource::new(Duration::from_millis(SLEEP_MS));
        self.rtc.sleep_light(&[&timer]);
        if let Some(s) = self.stretch.as_mut() {
            s.naps += 1;
        }
        true
    }
}
