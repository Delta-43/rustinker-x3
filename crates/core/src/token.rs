//! `tk1` tokens (PROTOCOL.md C.1) into the portrait reader frame, streamed:
//! the caller feeds the pixel bytes in any chunk sizes, straight from the
//! card, and never holds the whole payload (up to 104 KB at 2 bpp).
//!
//! One pass makes one 1-bit plane: a pixel is ink when bit `level` of the
//! pass's level mask is set (levels 0 black, 1 dark grey, 2 light grey,
//! 3 white). 1 bpp tokens use levels 0 and 3. The grey waveform (ADR 018,
//! CrossPoint's method) needs three passes: `BASE`, then `LOW` and `HIGH`.
//!
//! Landscape tokens (792 × 528) are turned a quarter clockwise to fit the
//! portrait frame: the image's top edge lands on the frame's right edge.
use crate::{
    Error,
    reader::{Frame, HEIGHT, WIDTH},
};
use x3_proto::payload::TokenHeader;

pub const HEADER_LEN: usize = 16;
/// Black and white only: black and dark grey are ink.
pub const MONO: u8 = 0b0011;
/// Grey base frame: everything but white is ink.
pub const BASE: u8 = 0b0111;
/// Grey low-bit plane (DTM1): dark grey only.
pub const LOW: u8 = 0b0010;
/// Grey high-bit plane (DTM2): dark and light grey.
pub const HIGH: u8 = 0b0110;

pub struct Decoder {
    header: TokenHeader,
    mask: u8,
    /// Next pixel index in payload order.
    at: usize,
}

impl Decoder {
    pub fn new(header: &[u8], mask: u8) -> Result<Self, Error> {
        let header = TokenHeader::decode(header).map_err(|_| Error::Corrupt)?;
        if mask > 0b1111 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            header,
            mask,
            at: 0,
        })
    }

    pub fn header(&self) -> &TokenHeader {
        &self.header
    }

    pub fn landscape(&self) -> bool {
        self.header.width > self.header.height
    }

    fn pixels(&self) -> usize {
        usize::from(self.header.width) * usize::from(self.header.height)
    }

    /// All pixels have been fed.
    pub fn done(&self) -> bool {
        self.at == self.pixels()
    }

    /// Decodes the next pixel bytes into `frame`. Bytes past the end are an
    /// error. Call `Frame::clear` first: only ink is drawn.
    pub fn feed(&mut self, frame: &mut Frame<'_>, bytes: &[u8]) -> Result<(), Error> {
        let bpp = usize::from(self.header.bpp);
        let per_byte = 8 / bpp;
        if bytes.len() * per_byte > self.pixels() - self.at {
            return Err(Error::TooLarge);
        }
        let width = usize::from(self.header.width);
        let landscape = self.landscape();
        for &b in bytes {
            for k in 0..per_byte {
                let raw = (b >> (8 - bpp * (k + 1))) & ((1 << bpp) - 1);
                // 1 bpp: 0 black, 1 white, the same as 2 bpp levels 0 and 3.
                let level = if bpp == 1 { raw * 3 } else { raw };
                let ink = self.mask >> level & 1 == 1;
                if ink {
                    let (x, y) = (self.at % width, self.at / width);
                    let (px, py) = if landscape {
                        (WIDTH - 1 - y, x)
                    } else {
                        (x, y)
                    };
                    debug_assert!(px < WIDTH && py < HEIGHT);
                    frame.ink(px as i32, py as i32);
                }
                self.at += 1;
            }
        }
        Ok(())
    }
}
