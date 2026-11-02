//! X3 panel geometry and Phase 2.2 test patterns, as raw controller rows.
//!
//! A raw row is the 99 bytes sent to the UC8253 for one line, first byte
//! first, MSB first; a set bit is white. On the owner unit (HARDWARE.md,
//! verified with `corners`), raw (x, y) is portrait pixel (column y, row x)
//! with the buttons at the bottom: a transpose, no mirroring.

pub const WIDTH: usize = 792;
pub const HEIGHT: usize = 528;
pub const ROW_BYTES: usize = WIDTH / 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    White,
    Black,
    /// 33×33-pixel squares (24 × 16 across the panel).
    Checker,
    /// A 96-pixel black square at raw (0, 0), a 48-pixel one to its right
    /// along raw x and a 24-pixel one below it along raw y, on white.
    Corners,
    /// A black 96-pixel square at raw x `step * 96`, row band 216..312, for
    /// partial (fast) refresh tests.
    Block(u8),
}

impl Pattern {
    pub fn parse(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"white" => Self::White,
            b"black" => Self::Black,
            b"checker" => Self::Checker,
            b"corners" => Self::Corners,
            _ => return None,
        })
    }

    pub fn black(&self, x: usize, y: usize) -> bool {
        match *self {
            Self::White => false,
            Self::Black => true,
            Self::Checker => (x / 33 + y / 33) % 2 == 1,
            Self::Corners => {
                (x < 96 && y < 96)
                    || ((120..168).contains(&x) && y < 48)
                    || (x < 24 && (120..144).contains(&y))
            }
            Self::Block(step) => {
                let x0 = (step as usize % (WIDTH / 96)) * 96;
                (x0..x0 + 96).contains(&x) && (216..312).contains(&y)
            }
        }
    }

    /// Fills `out` with raw row `y`. Rows outside the panel are white.
    pub fn row(&self, y: usize, out: &mut [u8; ROW_BYTES]) {
        for (i, byte) in out.iter_mut().enumerate() {
            let mut b = 0xFF;
            if y < HEIGHT {
                for bit in 0..8 {
                    if self.black(i * 8 + bit, y) {
                        b &= !(0x80 >> bit);
                    }
                }
            }
            *byte = b;
        }
    }
}
