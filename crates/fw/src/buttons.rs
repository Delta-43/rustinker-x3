//! Raw button readings for Phase 2.3 calibration. The bottom four buttons are
//! a resistor ladder on GPIO1, the side two on GPIO2 (both ADC1, 11 dB);
//! power is GPIO3, pulled up, low when pressed. Pins use esp-hal's basic
//! offset calibration: without it the readings carried a ~1636-count offset
//! that pushed Back and Confirm past full scale (owner unit, 2026-10-27).
use core::cell::RefCell;
use esp_hal::time::{Duration, Instant};
use esp_hal::{
    Blocking,
    analog::adc::{Adc, AdcCalBasic, AdcPin},
    delay::Delay,
    gpio::Input,
    peripherals::{ADC1, GPIO1, GPIO2},
};
use x3_core::buttons::{Button, Debouncer, decode};

/// The buttons, shared by the main loop and the panel's busy wait. The
/// ESP32-C3 has one core and no interrupt touches this, so only the main
/// task ever reaches it; the `RefCell`s catch any re-entry.
struct Global {
    buttons: RefCell<Option<Buttons<'static>>>,
    latch: RefCell<Latch>,
}
// SAFETY: single core, used only from the main task, never from an interrupt.
unsafe impl Sync for Global {}

static GLOBAL: Global = Global {
    buttons: RefCell::new(None),
    latch: RefCell::new(Latch {
        debounce: Debouncer::primed(None),
        pressed: None,
        replay: 0,
        last_poll: None,
    }),
};

/// A press made while the panel was busy, kept for the screen to read.
struct Latch {
    debounce: Debouncer,
    pressed: Option<Button>,
    /// Samples left to replay it (a debouncer wants it stable twice).
    replay: u8,
    last_poll: Option<Instant>,
}

/// Hands the buttons to the shared slot. Call once at start-up.
pub fn install(b: Buttons<'static>) {
    *GLOBAL.buttons.borrow_mut() = Some(b);
}

/// What the rest of the firmware holds: reads go through the shared slot.
pub struct Handle;

impl Handle {
    /// One sample. A press caught during a refresh comes first, as if it
    /// were held for `STABLE` samples; then the real buttons.
    pub fn sample(&mut self) -> Sample {
        {
            let mut l = GLOBAL.latch.borrow_mut();
            if l.replay > 0
                && let Some(b) = l.pressed
            {
                l.replay -= 1;
                if l.replay == 0 {
                    l.pressed = None;
                }
                return synthetic(b);
            }
        }
        real()
    }
}

fn real() -> Sample {
    GLOBAL
        .buttons
        .borrow_mut()
        .as_mut()
        .map_or(Sample::NONE, Buttons::sample)
}

/// A refresh is starting: whatever is down now does not count.
pub fn wait_begin() {
    let s = real();
    let mut l = GLOBAL.latch.borrow_mut();
    l.debounce = Debouncer::primed(decode(s.bottom, s.side, s.power_pressed));
    l.last_poll = Some(Instant::now());
}

/// Called from the panel's busy wait: every 10 ms, read the buttons and keep
/// the first new press.
pub fn wait_poll() {
    let now = Instant::now();
    if GLOBAL
        .latch
        .borrow()
        .last_poll
        .is_some_and(|t| now - t < Duration::from_millis(10))
    {
        return;
    }
    let s = real();
    let mut l = GLOBAL.latch.borrow_mut();
    l.last_poll = Some(now);
    if let Some(b) = l.debounce.feed(decode(s.bottom, s.side, s.power_pressed))
        && l.pressed.is_none()
    {
        l.pressed = Some(b);
        l.replay = x3_core::buttons::STABLE;
    }
}

/// Raw readings that decode as `b` (the middle of each ladder band).
fn synthetic(b: Button) -> Sample {
    let (bottom, side, power_pressed) = match b {
        Button::Left => (3500, 4095, false),
        Button::Right => (2500, 4095, false),
        Button::Back => (1400, 4095, false),
        Button::Confirm => (300, 4095, false),
        Button::Up => (4095, 2000, false),
        Button::Down => (4095, 500, false),
        Button::Power => (4095, 4095, true),
    };
    Sample {
        bottom: Some(bottom),
        side: Some(side),
        power_pressed,
    }
}

pub struct Buttons<'d> {
    adc: Adc<'d, ADC1<'d>, Blocking>,
    bottom: AdcPin<GPIO1<'d>, ADC1<'d>, AdcCalBasic<ADC1<'d>>>,
    side: AdcPin<GPIO2<'d>, ADC1<'d>, AdcCalBasic<ADC1<'d>>>,
    power: Input<'d>,
}

/// One sample: raw 12-bit counts (`None` if the ADC never finished) and the
/// power button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub bottom: Option<u16>,
    pub side: Option<u16>,
    pub power_pressed: bool,
}

impl Sample {
    const NONE: Self = Self {
        bottom: None,
        side: None,
        power_pressed: false,
    };
}

impl<'d> Buttons<'d> {
    pub fn new(
        adc: Adc<'d, ADC1<'d>, Blocking>,
        bottom: AdcPin<GPIO1<'d>, ADC1<'d>, AdcCalBasic<ADC1<'d>>>,
        side: AdcPin<GPIO2<'d>, ADC1<'d>, AdcCalBasic<ADC1<'d>>>,
        power: Input<'d>,
    ) -> Self {
        Self {
            adc,
            bottom,
            side,
            power,
        }
    }

    /// One reading per ladder. esp-hal notes the ADC can miss a one-shot
    /// start that comes too soon after the last conversion, which returned
    /// stale values from the other channel. So: 20 µs gaps, and the first
    /// conversion after switching channels is discarded.
    pub fn sample(&mut self) -> Sample {
        let bottom = self.read_bottom();
        let side = self.read_side();
        Sample {
            bottom,
            side,
            power_pressed: self.power.is_low(),
        }
    }

    fn read_bottom(&mut self) -> Option<u16> {
        let delay = Delay::new();
        let mut value = None;
        for _ in 0..2 {
            delay.delay_micros(20);
            value = None;
            for _ in 0..10_000 {
                if let Ok(v) = self.adc.read_oneshot(&mut self.bottom) {
                    value = Some(v);
                    break;
                }
            }
        }
        value
    }

    fn read_side(&mut self) -> Option<u16> {
        let delay = Delay::new();
        let mut value = None;
        for _ in 0..2 {
            delay.delay_micros(20);
            value = None;
            for _ in 0..10_000 {
                if let Ok(v) = self.adc.read_oneshot(&mut self.side) {
                    value = Some(v);
                    break;
                }
            }
        }
        value
    }
}
