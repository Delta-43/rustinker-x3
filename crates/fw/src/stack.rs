//! Stack high-water measurement (RAM budget). At boot the unused
//! stack below the current frame is painted; later, the painted words still
//! intact at the low end are stack that has never been touched.
unsafe extern "C" {
    static _stack_end: u32;
    static _stack_start: u32;
}

const PAINT: u32 = 0xA5A5_A5A5;
/// Left alone at the low end: esp-hal keeps its stack-guard word there.
const GUARD: usize = 512;
/// Left alone just below the painting frame.
const MARGIN: usize = 1024;

fn sp() -> usize {
    let sp: usize;
    // SAFETY: reads the stack pointer register; no memory is touched.
    unsafe { core::arch::asm!("mv {0}, sp", out(reg) sp) };
    sp
}

fn low() -> usize {
    (&raw const _stack_end) as usize + GUARD
}

/// Total stack size in bytes.
pub fn size() -> usize {
    (&raw const _stack_start) as usize - (&raw const _stack_end) as usize
}

/// Paints the never-used part of the stack. Call once, early in `main`.
pub fn paint() {
    let top = sp() - MARGIN;
    let mut a = low();
    while a + 4 <= top {
        // SAFETY: [low, top) lies below every live frame (top is under the
        // current stack pointer) and above the guard word; nothing reads it.
        unsafe { (a as *mut u32).write_volatile(PAINT) };
        a += 4;
    }
}

/// Bytes of stack never used since `paint` (the headroom left).
pub fn untouched() -> usize {
    let mut a = low();
    // SAFETY: reads words inside the stack region, below all live frames.
    while unsafe { (a as *const u32).read_volatile() } == PAINT {
        a += 4;
    }
    a - low()
}
