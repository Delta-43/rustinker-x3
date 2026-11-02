//! UC8253 driver for the X3 panel (blocking, black and white only).
//!
//! Init bytes, CDI values, waveform tables, refresh sequences, reset timing
//! and the two-phase BUSY handshake are ported from MarigoldOS 4778b2e
//! (`display/src/epd/uc8253.rs`, `fw/src/display_flush/uc8253.rs`,
//! `hal-ext/src/spi_dma.rs`; MIT, Copyright (c) 2026 Jon-Vii), which ports
//! CrossPoint's `Uc8253X3Driver` (MIT). See THIRD_PARTY.md and ADR 019.
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Input, Output},
    spi::master::Spi,
    time::{Duration, Instant},
};
use x3_core::panel::{HEIGHT, Pattern, ROW_BYTES};

/// Anything that can supply raw panel rows: test patterns or a rendered page.
pub trait Rows {
    fn row(&self, y: usize, out: &mut [u8; ROW_BYTES]);
}

impl Rows for Pattern {
    fn row(&self, y: usize, out: &mut [u8; ROW_BYTES]) {
        Pattern::row(self, y, out);
    }
}

impl Rows for x3_core::reader::Frame<'_> {
    fn row(&self, y: usize, out: &mut [u8; ROW_BYTES]) {
        self.raw_row(y, out);
    }
}

const PANEL_SETTING: u8 = 0x00;
const POWER_SETTING: u8 = 0x01;
const POWER_OFF: u8 = 0x02;
const POWER_OFF_SEQ: u8 = 0x03;
const POWER_ON: u8 = 0x04;
const BOOSTER_SOFT_START: u8 = 0x06;
const DEEP_SLEEP: u8 = 0x07;
const DTM1: u8 = 0x10;
const DATA_STOP: u8 = 0x11;
const DISPLAY_REFRESH: u8 = 0x12;
const DTM2: u8 = 0x13;
const LUT_VCOM: u8 = 0x20;
const LUT_WW: u8 = 0x21;
const LUT_BW: u8 = 0x22;
const LUT_WB: u8 = 0x23;
const LUT_BB: u8 = 0x24;
const PLL_CONTROL: u8 = 0x30;
const VCOM_DATA_INTERVAL: u8 = 0x50;
const RESOLUTION: u8 = 0x61;
const GATE_SOURCE_START: u8 = 0x65;
const VCOM_DC: u8 = 0x82;
const LV_SELECTION: u8 = 0xE1;
const DEEP_SLEEP_CHECK: u8 = 0xA5;

/// CDI values as CrossPoint/freeink-sdk send them for the X3 (0x29 for the
/// full and fast banks, 0xA9 for the half bank). Ours were 0x2F/0xAF, which
/// left a grey speckle on every screen (owner, 2026-11-02).
const CDI_DIFFERENTIAL: u8 = 0x29;

const CDI_ABSOLUTE: u8 = 0xA9;
const CDI_INTERVAL: u8 = 0x07;
/// Quiet after a refresh before the panel RAM is written again.
const SETTLE_MS: u32 = 200;

/// Resolution is 792×600: the gate driver spans 600 rows and the 72 below
/// the glass fall off-screen. Kept verbatim from the reference.
const INIT: &[(u8, &[u8])] = &[
    (PANEL_SETTING, &[0x3F, 0x0A]),
    (RESOLUTION, &[0x03, 0x18, 0x02, 0x58]),
    (GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00]),
    (POWER_OFF_SEQ, &[0x20]),
    (POWER_SETTING, &[0x07, 0x17, 0x3F, 0x3F, 0x17]),
    (VCOM_DC, &[0x24]),
    (BOOSTER_SOFT_START, &[0x25, 0x25, 0x3C, 0x37]),
    (PLL_CONTROL, &[0x09]),
    (LV_SELECTION, &[0x02]),
];

const LUT_LEN: usize = 42;
type Lut = [u8; LUT_LEN];

/// VCOM plus white→white, black→white, white→black and black→black.
struct Bank {
    vcom: Lut,
    ww: Lut,
    bw: Lut,
    wb: Lut,
    bb: Lut,
}

/// The first 12 bytes of each table; the remaining 30 are zero.
const fn lut(head: [u8; 12]) -> Lut {
    let mut t = [0; LUT_LEN];
    let mut i = 0;
    while i < 12 {
        t[i] = head[i];
        i += 1;
    }
    t
}

#[rustfmt::skip]
static HALF: Bank = Bank {
    vcom: lut([0x00, 0x06, 0x01, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01]),
    ww:   lut([0xAA, 0x06, 0x01, 0x06, 0x06, 0x01, 0xA0, 0x04, 0x01, 0x01, 0x00, 0x01]),
    bw:   lut([0xAA, 0x06, 0x01, 0x06, 0x06, 0x01, 0xA0, 0x04, 0x01, 0x01, 0x00, 0x01]),
    wb:   lut([0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01]),
    bb:   lut([0x55, 0x06, 0x01, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01]),
};
#[rustfmt::skip]
static FAST: Bank = Bank {
    vcom: lut([0x00, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
    ww:   lut([0x20, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
    bw:   lut([0xAA, 0x04, 0x02, 0x04, 0x04, 0x01, 0x80, 0x04, 0x01, 0x00, 0x00, 0x01]),
    wb:   lut([0x55, 0x04, 0x02, 0x04, 0x04, 0x01, 0x40, 0x04, 0x01, 0x00, 0x00, 0x01]),
    bb:   lut([0x10, 0x04, 0x02, 0x04, 0x04, 0x01, 0x00, 0x04, 0x01, 0x00, 0x00, 0x01]),
};
#[rustfmt::skip]
static FULL: Bank = Bank {
    vcom: lut([0x00, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x01]),
    ww:   lut([0x4A, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x01]),
    bw:   lut([0x0A, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x01]),
    wb:   lut([0x04, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x40, 0x0A, 0x00, 0x00, 0x00, 0x01]),
    bb:   lut([0x84, 0x18, 0x04, 0x0E, 0x0A, 0x01, 0x40, 0x0A, 0x00, 0x00, 0x00, 0x01]),
};

// Four-grey tokens (ADR 018): CrossPoint's X3 method and banks, MIT, from
// freeink-sdk `libs/display/FreeInkDisplay/src/lut/Uc8253X3Luts.h` at
// CrossPlant 98a528f (THIRD_PARTY.md). `HALF` above is byte-identical to
// their scrub bank. These run with their own CDI values, not ours.
/// CDI for the grey nudge (`GC`).
const CDI_GREY: u8 = 0x29;
/// CDI for the settle (`PRE_BW_MID`) and the scrub (`HALF`).
const CDI_SETTLE: u8 = 0xA9;
/// Gentle settle with DTM1 == DTM2 == the base frame, leaving the
/// particles receptive to the weak grey nudge.
#[rustfmt::skip]
static PRE_BW_MID: Bank = Bank {
    vcom: lut([0x00, 0x06, 0x03, 0x06, 0x06, 0x01, 0x00, 0x04, 0x01, 0x01, 0x00, 0x01]),
    ww:   lut([0x20, 0x06, 0x03, 0x06, 0x06, 0x01, 0x20, 0x04, 0x01, 0x01, 0x00, 0x01]),
    bw:   lut([0xAA, 0x06, 0x03, 0x06, 0x06, 0x01, 0xA8, 0x04, 0x01, 0x01, 0x00, 0x01]),
    wb:   lut([0x55, 0x06, 0x03, 0x06, 0x06, 0x01, 0x50, 0x04, 0x01, 0x01, 0x00, 0x01]),
    bb:   lut([0x10, 0x06, 0x03, 0x06, 0x06, 0x01, 0x14, 0x04, 0x01, 0x01, 0x00, 0x01]),
};
/// Grey nudge. (DTM1, DTM2) bit pairs: (1, 1) dark grey (`ww`, weaker),
/// (0, 1) light grey (`bw`, stronger), (0, 0) unchanged.
#[rustfmt::skip]
static GC: Bank = Bank {
    vcom: lut([0x00, 0x03, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    ww:   lut([0x20, 0x03, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    bw:   lut([0x80, 0x03, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    wb:   lut([0x54, 0x03, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
    bb:   lut([0x00, 0x03, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
};

/// A grey plane: the frame's ink pixels are the ones to nudge, sent as 1.
pub struct Inverted<'a, R: Rows>(pub &'a R);

impl<R: Rows> Rows for Inverted<'_, R> {
    fn row(&self, y: usize, out: &mut [u8; ROW_BYTES]) {
        self.0.row(y, out);
        for b in out.iter_mut() {
            *b = !*b;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Spi,
    /// BUSY never went low after a command: the controller ignored it.
    NeverAsserted,
    /// BUSY stayed low past the ceiling.
    TimedOut,
}

/// Milliseconds spent in each phase of one refresh, for the serial report.
#[derive(Debug, Default, Clone, Copy)]
pub struct Timing {
    pub write_ms: u64,
    pub power_on_ms: u64,
    pub refresh_ms: [u64; 2],
    pub total_ms: u64,
}

/// Screen changes between full refreshes.
const CHANGE_FULL_EVERY: u8 = 6;

pub struct Epd<'d> {
    spi: Spi<'d, Blocking>,
    cs: Output<'d>,
    dc: Output<'d>,
    rst: Output<'d>,
    busy: Input<'d>,
    powered: bool,
    /// DTM1 holds what the glass shows (set by `full` and `turn`). Survives
    /// `power_off`, which keeps the controller's RAM.
    staged: bool,
    /// The glass shows a grey nudge: scrub before the next black-and-white
    /// update.
    grey: bool,
    /// Screen changes since the last full refresh.
    changes: u8,
    /// The next `change` is a full refresh.
    force_full: bool,
    delay: Delay,
}

impl<'d> Epd<'d> {
    pub fn new(
        spi: Spi<'d, Blocking>,
        cs: Output<'d>,
        dc: Output<'d>,
        rst: Output<'d>,
        busy: Input<'d>,
    ) -> Self {
        Self {
            spi,
            cs,
            dc,
            rst,
            busy,
            powered: false,
            staged: false,
            grey: false,
            changes: 0,
            force_full: false,
            delay: Delay::new(),
        }
    }

    /// The shared SPI bus, for the SD card while the display is idle.
    pub fn bus(&mut self) -> &mut Spi<'d, Blocking> {
        &mut self.spi
    }

    pub fn busy_high(&self) -> bool {
        self.busy.is_high()
    }

    /// Reset, send the init sequence and whiten both RAM planes. Nothing on
    /// the glass changes until a refresh.
    pub fn init(&mut self) -> Result<(), Error> {
        self.rst.set_high();
        self.delay.delay_millis(20);
        self.rst.set_low();
        self.delay.delay_millis(2);
        self.rst.set_high();
        self.delay.delay_millis(20);
        self.delay.delay_millis(50);
        for (cmd, data) in INIT {
            self.command(*cmd, data)?;
        }
        self.plane(DTM1, &Pattern::White)?;
        self.command(DATA_STOP, &[])?;
        self.plane(DTM2, &Pattern::White)?;
        self.command(DATA_STOP, &[])?;
        self.powered = false;
        self.staged = false;
        self.grey = false;
        Ok(())
    }

    /// Full refresh to `next` from a white baseline, then a fast settle pass
    /// that leaves DTM1 holding `next`.
    pub fn full(&mut self, next: &dyn Rows) -> Result<Timing, Error> {
        self.scrub()?;
        let start = Instant::now();
        let mut t = Timing::default();
        self.load_bank(&FULL, CDI_DIFFERENTIAL)?;
        // The full bank is made for a white old plane (freeink-sdk), so DTM1
        // is white whatever the glass shows. (Keeping the real old plane, or
        // one half-bank pass, did no better on the owner's unit, 2026-11-02.)
        self.plane(DTM1, &Pattern::White)?;
        self.command(DATA_STOP, &[])?;
        self.plane(DTM2, next)?;
        t.write_ms = start.elapsed().as_millis();
        if !self.powered {
            t.power_on_ms = self.power_on()?;
        }
        t.refresh_ms[0] = self.refresh()?;
        // Settle pass: old == new with the fast bank (the first differential
        // after a full garbles on the X3 otherwise, freeink-sdk notes).
        self.delay.delay_millis(SETTLE_MS);
        self.plane(DTM1, next)?;
        self.command(DATA_STOP, &[])?;
        self.load_bank(&FAST, CDI_DIFFERENTIAL)?;
        self.plane(DTM2, next)?;
        t.refresh_ms[1] = self.refresh()?;
        self.plane(DTM1, next)?;
        self.command(DATA_STOP, &[])?;
        self.staged = true;
        t.total_ms = start.elapsed().as_millis();
        Ok(t)
    }

    /// Fast differential refresh from `prev` (what the glass shows) to `next`.
    /// With the charge pump off, DTM1 cannot be trusted, so this falls back to
    /// the absolute "half" bank, as the reference does.
    pub fn fast(&mut self, prev: &dyn Rows, next: &dyn Rows) -> Result<Timing, Error> {
        self.scrub()?;
        let start = Instant::now();
        let mut t = Timing::default();
        if self.powered {
            self.load_bank(&FAST, CDI_DIFFERENTIAL)?;
            self.plane(DTM1, prev)?;
            self.command(DATA_STOP, &[])?;
            self.plane(DTM2, next)?;
            t.write_ms = start.elapsed().as_millis();
            t.refresh_ms[0] = self.refresh()?;
        } else {
            self.load_bank(&HALF, CDI_ABSOLUTE)?;
            self.plane(DTM2, next)?;
            t.write_ms = start.elapsed().as_millis();
            t.power_on_ms = self.power_on()?;
            t.refresh_ms[0] = self.refresh()?;
            self.delay.delay_millis(SETTLE_MS);
        }
        t.total_ms = start.elapsed().as_millis();
        Ok(t)
    }

    /// Page turn when DTM1 already holds what the glass shows (after `full`
    /// or a previous `turn`): write only the new page, refresh, then stage it
    /// as the next turn's old plane. After `power_off` the pump comes back
    /// on first; with nothing staged it falls back to `fast` from white.
    pub fn turn(&mut self, next: &dyn Rows) -> Result<Timing, Error> {
        self.turn_with(next, false)
    }

    /// `turn` with the clean pass, for menus and lists: their selection is
    /// a solid black block, and one fast pass leaves it light grey when it
    /// moves away (owner video, 2026-11-02). Reader pages are thin text and
    /// keep the single pass (about 0.4 s).
    pub fn turn_clean(&mut self, next: &dyn Rows) -> Result<Timing, Error> {
        self.turn_with(next, true)
    }

    fn turn_with(&mut self, next: &dyn Rows, clean: bool) -> Result<Timing, Error> {
        // With the charge pump off (idle on battery) the next update gets the
        // strong half bank, as freeink-sdk does on wake: `fast` takes that
        // path when unpowered.
        if !self.staged || self.grey || !self.powered {
            let t = self.fast(&Pattern::White, next)?;
            if self.staged {
                self.plane(DTM1, next)?;
                self.command(DATA_STOP, &[])?;
            }
            return Ok(t);
        }
        let start = Instant::now();
        let mut t = Timing::default();
        if !self.powered {
            t.power_on_ms = self.power_on()?;
        }
        self.load_bank(&FAST, CDI_DIFFERENTIAL)?;
        self.plane(DTM2, next)?;
        t.write_ms = start.elapsed().as_millis();
        t.refresh_ms[0] = self.refresh()?;
        // Clean pass: DTM1 still holds the old frame and DTM2 the new, so a
        // second refresh repeats the drive on the changed pixels only. No
        // data is written.
        if clean {
            t.refresh_ms[1] = self.refresh()?;
        }
        // On screen now; staging the old plane is after the turn the reader
        // sees, but still inside this call.
        t.total_ms = start.elapsed().as_millis();
        self.plane(DTM1, next)?;
        self.command(DATA_STOP, &[])?;
        Ok(t)
    }

    /// A screen change: the clean double fast refresh (new screen in ~0.4 s,
    /// done in ~0.8 s), with a full refresh every `CHANGE_FULL_EVERY` changes,
    /// when nothing is staged, after a grey picture, or when asked
    /// (`force_full`). Full refreshes on every change took ~1.7 s with no
    /// response meanwhile (audit, 2026-11-02).
    pub fn change(&mut self, next: &dyn Rows) -> Result<Timing, Error> {
        if core::mem::take(&mut self.force_full)
            || self.changes >= CHANGE_FULL_EVERY
            || !self.staged
            || self.grey
        {
            self.changes = 0;
            return self.full(next);
        }
        self.changes += 1;
        self.turn_clean(next)
    }

    /// The next `change` is a full refresh (Back on Home clears the glass).
    pub fn force_full(&mut self) {
        self.force_full = true;
    }

    /// Clears retained ghosting: `cycles` full refreshes to black and back
    /// to white (owner test, 2026-11-02: six cycles cleared a menu ghost
    /// that single refreshes could not). About 3.5 s per cycle.
    pub fn clean(&mut self, cycles: u8) -> Result<(), Error> {
        for _ in 0..cycles {
            self.full(&Pattern::Black)?;
            self.full(&Pattern::White)?;
        }
        Ok(())
    }

    /// Grey step 1: the base frame (everything but white as ink) by a full
    /// refresh, then the settle pass with both RAM planes holding it.
    pub fn grey_base(&mut self, base: &dyn Rows) -> Result<Timing, Error> {
        let mut t = self.full(base)?;
        let start = Instant::now();
        self.load_bank(&PRE_BW_MID, CDI_SETTLE)?;
        self.refresh()?;
        t.total_ms += start.elapsed().as_millis();
        Ok(t)
    }

    /// Grey step 2: one plane, already inverted (1 = nudge). Low plane to
    /// DTM1, high plane to DTM2. The RAM no longer mirrors the glass.
    pub fn grey_plane(&mut self, high: bool, plane: &dyn Rows) -> Result<(), Error> {
        self.staged = false;
        if high {
            self.plane(DTM2, plane)
        } else {
            self.plane(DTM1, plane)?;
            self.command(DATA_STOP, &[])
        }
    }

    /// Grey step 3: the nudge.
    pub fn grey_show(&mut self) -> Result<u64, Error> {
        self.load_bank(&GC, CDI_GREY)?;
        if !self.powered {
            self.power_on()?;
        }
        let ms = self.refresh()?;
        self.grey = true;
        self.staged = false;
        Ok(ms)
    }

    /// After a grey picture: both planes white and the scrub bank, so the
    /// next black-and-white update starts from clean white.
    fn scrub(&mut self) -> Result<(), Error> {
        if !self.grey {
            return Ok(());
        }
        self.grey = false;
        self.plane(DTM1, &Pattern::White)?;
        self.command(DATA_STOP, &[])?;
        self.plane(DTM2, &Pattern::White)?;
        self.command(DATA_STOP, &[])?;
        self.load_bank(&HALF, CDI_SETTLE)?;
        if !self.powered {
            self.power_on()?;
        }
        self.refresh()?;
        self.staged = false;
        Ok(())
    }

    /// Charge pump off while idle. The image stays on the glass and the
    /// staged page stays in RAM, so the next `turn` only powers on again.
    pub fn power_off(&mut self) -> Result<(), Error> {
        if self.powered {
            self.command(POWER_OFF, &[])?;
            self.wait()?;
            self.powered = false;
        }
        Ok(())
    }

    pub fn powered(&self) -> bool {
        self.powered
    }

    /// Charge pump off, then controller deep sleep. Needs `init` to wake.
    pub fn sleep(&mut self) -> Result<(), Error> {
        self.power_off()?;
        self.staged = false;
        self.command(DEEP_SLEEP, &[DEEP_SLEEP_CHECK])
    }

    fn power_on(&mut self) -> Result<u64, Error> {
        self.command(POWER_ON, &[])?;
        let ms = self.wait()?;
        self.powered = true;
        Ok(ms)
    }

    fn refresh(&mut self) -> Result<u64, Error> {
        self.command(DISPLAY_REFRESH, &[])?;
        self.wait()
    }

    fn load_bank(&mut self, bank: &Bank, cdi: u8) -> Result<(), Error> {
        self.command(VCOM_DATA_INTERVAL, &[cdi, CDI_INTERVAL])?;
        self.command(LUT_VCOM, &bank.vcom)?;
        self.command(LUT_WW, &bank.ww)?;
        self.command(LUT_BW, &bank.bw)?;
        self.command(LUT_WB, &bank.wb)?;
        self.command(LUT_BB, &bank.bb)
    }

    /// Streams one RAM plane, row by row. No DATA_STOP: callers add it where
    /// the reference does (never between DTM2 and its refresh).
    fn plane(&mut self, cmd: u8, pattern: &dyn Rows) -> Result<(), Error> {
        self.dc.set_low();
        self.cs.set_low();
        let mut r = self.spi.write(&[cmd]);
        self.dc.set_high();
        let mut row = [0u8; ROW_BYTES];
        for y in 0..HEIGHT {
            if r.is_err() {
                break;
            }
            pattern.row(y, &mut row);
            r = self.spi.write(&row);
        }
        self.cs.set_high();
        r.map_err(|_| Error::Spi)
    }

    fn command(&mut self, cmd: u8, data: &[u8]) -> Result<(), Error> {
        self.dc.set_low();
        self.cs.set_low();
        let mut r = self.spi.write(&[cmd]);
        if r.is_ok() && !data.is_empty() {
            self.dc.set_high();
            r = self.spi.write(data);
        }
        self.cs.set_high();
        r.map_err(|_| Error::Spi)
    }

    /// UC8253 BUSY: low while working, high when done. Wait up to 1 s for the
    /// fall (a command that never drops it was ignored), then up to 30 s for
    /// the rise. Returns the elapsed milliseconds.
    fn wait(&mut self) -> Result<u64, Error> {
        let start = Instant::now();
        // The buttons are read while the panel works, so a press made
        // during a refresh is not lost (crate::buttons keeps one).
        crate::buttons::wait_begin();
        while self.busy.is_high() {
            if start.elapsed() > Duration::from_secs(1) {
                return Err(Error::NeverAsserted);
            }
        }
        while self.busy.is_low() {
            crate::buttons::wait_poll();
            if start.elapsed() > Duration::from_secs(30) {
                return Err(Error::TimedOut);
            }
        }
        Ok(start.elapsed().as_millis())
    }
}
