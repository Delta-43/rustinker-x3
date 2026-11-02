//! X3 button decoding: two resistor ladders plus the power line, debounced.
//!
//! Owner unit, calibrated ADC (HARDWARE.md, 2026-10-27): GPIO1 reads 3518,
//! 2685, 1486 and 4 for the four bottom buttons, left to right; GPIO2 reads
//! 2236 (left side key) and 3 (right side key); idle 4095. Thresholds are the
//! midpoints CrossPlant uses; above 3900 is no press.
//!
//! **Functions are the owner's, not CrossPoint's** (ADR 024, 2026-10-31): the
//! bottom buttons are Left, Right, Back, Confirm from left to right.
//! CrossPoint binds the same four ladder levels to Back, Confirm, Left, Right.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Back,
    Confirm,
    Left,
    Right,
    Up,
    Down,
    Power,
}

const NO_PRESS: u16 = 3900;

/// The button a GPIO1 reading means, if any.
pub fn bottom(raw: u16) -> Option<Button> {
    Some(match raw {
        r if r > NO_PRESS => return None,
        3101.. => Button::Left,
        2091.. => Button::Right,
        751.. => Button::Back,
        _ => Button::Confirm,
    })
}

/// The button a GPIO2 reading means, if any.
pub fn side(raw: u16) -> Option<Button> {
    Some(match raw {
        r if r > NO_PRESS => return None,
        1121.. => Button::Up,
        _ => Button::Down,
    })
}

/// Turns samples into one press event per press. A reading must repeat for
/// `STABLE` consecutive samples before it counts, so bounce and the ramp
/// through other ladder levels on press and release are ignored.
#[derive(Debug, Default)]
pub struct Debouncer {
    candidate: Option<Button>,
    count: u8,
    held: Option<Button>,
}

pub const STABLE: u8 = 2;

impl Debouncer {
    /// A debouncer that treats `now` as already held: a key that is down
    /// when this starts never counts, only a press that begins later. Used
    /// while the panel refreshes, so the press that opened a screen is not
    /// read again as a new one.
    pub const fn primed(now: Option<Button>) -> Self {
        Self {
            candidate: now,
            count: STABLE,
            held: now,
        }
    }

    /// Feed one sample: what is down now (`None` for nothing). Returns a
    /// button on the sample its press becomes stable.
    pub fn feed(&mut self, now: Option<Button>) -> Option<Button> {
        if now == self.candidate {
            self.count = self.count.saturating_add(1);
        } else {
            self.candidate = now;
            self.count = 1;
        }
        if self.count < STABLE || self.held == self.candidate {
            return None;
        }
        self.held = self.candidate;
        self.held
    }
}

/// One sample's reading: ladder raws (`None` if the ADC failed) and power.
/// Power wins, then GPIO1, then GPIO2: one button is reported at a time.
pub fn decode(bottom_raw: Option<u16>, side_raw: Option<u16>, power: bool) -> Option<Button> {
    if power {
        return Some(Button::Power);
    }
    bottom_raw
        .and_then(bottom)
        .or_else(|| side_raw.and_then(side))
}
