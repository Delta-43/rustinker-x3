//! Screen capture over USB serial, for documentation. `capture on` arms it;
//! from then on every frame sent to the panel is also written to serial as
//! `CAP <tag>`, 528 lines of hex (one raw panel row each, as `epd` sends them),
//! then `CAP end`. `tools/capture.py` turns the output into PNGs. Off at boot.
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::epd::Rows;
use crate::serial::Out;
use x3_core::panel::{HEIGHT, ROW_BYTES};

static ARMED: AtomicBool = AtomicBool::new(false);

pub fn set(on: bool) {
    ARMED.store(on, Ordering::Relaxed);
}

/// Writes `rows` to serial if capture is armed. `tag` says how to read it:
/// `bw` (0 bits are ink), or the grey steps `base`, `low`, `high` (1 bits
/// mark the pixels nudged to dark or light grey).
pub fn send(out: &mut Out<'_>, tag: &str, rows: &dyn Rows) {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let _ = writeln!(out, "CAP {tag}");
    let mut row = [0u8; ROW_BYTES];
    let mut line = [0u8; ROW_BYTES * 2];
    for y in 0..HEIGHT {
        rows.row(y, &mut row);
        for (i, b) in row.iter().enumerate() {
            line[2 * i] = HEX[(b >> 4) as usize];
            line[2 * i + 1] = HEX[(b & 15) as usize];
        }
        if let Ok(s) = core::str::from_utf8(&line) {
            let _ = writeln!(out, "{s}");
        }
    }
    let _ = writeln!(out, "CAP end");
    out.flush();
}
