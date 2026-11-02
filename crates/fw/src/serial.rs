//! USB serial output that never blocks when no host is reading.
//!
//! esp-hal's blocking `write` waits for the host forever. Here each byte may
//! wait up to `TIMEOUT`. After a timeout the writer is "stalled": each byte
//! gets one try and no wait, so output resumes as soon as a host reads again
//! (dropping everything while stalled lost the first reply after connecting).
//! The 64-byte TX FIFO is only sent on a flush, so a full FIFO is flushed
//! before waiting (a reply longer than 64 bytes was lost otherwise).
use esp_hal::{
    Blocking,
    time::{Duration, Instant},
    usb_serial_jtag::UsbSerialJtag,
};

const TIMEOUT: Duration = Duration::from_millis(50);

pub struct Out<'d> {
    usb: UsbSerialJtag<'d, Blocking>,
    stalled: bool,
    received: u32,
    last: [u8; 8],
}

impl<'d> Out<'d> {
    pub fn new(usb: UsbSerialJtag<'d, Blocking>) -> Self {
        Self {
            usb,
            stalled: false,
            received: 0,
            last: [0; 8],
        }
    }

    /// Asks the hardware to send what is in the FIFO. Never waits.
    pub fn flush(&mut self) {
        let _ = self.usb.flush_tx_nb();
    }

    /// One received byte, if the host sent any.
    pub fn read_byte(&mut self) -> Option<u8> {
        let b = self.usb.read_byte().ok()?;
        self.last[self.received as usize % 8] = b;
        self.received = self.received.wrapping_add(1);
        Some(b)
    }

    /// Bytes received since start, for bring-up diagnostics.
    pub fn received(&self) -> u32 {
        self.received
    }

    /// The last eight bytes received, oldest first.
    pub fn last(&self) -> [u8; 8] {
        let mut out = [0; 8];
        for (i, b) in out.iter_mut().enumerate() {
            *b = self.last[(self.received as usize + i) % 8];
        }
        out
    }

    fn put(&mut self, byte: u8) {
        let deadline = Instant::now()
            + if self.stalled {
                Duration::ZERO
            } else {
                TIMEOUT
            };
        loop {
            if self.usb.write_byte_nb(byte).is_ok() {
                self.stalled = false;
                return;
            }
            let _ = self.usb.flush_tx_nb();
            if Instant::now() >= deadline {
                self.stalled = true;
                return;
            }
        }
    }
}

impl core::fmt::Write for Out<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.put(b'\r');
            }
            self.put(byte);
        }
        Ok(())
    }
}

/// Best-effort write for the panic handler: straight to the USB serial
/// FIFO, giving up on each byte after a short wait if no host reads.
pub fn panic_write(bytes: &[u8]) {
    let usb = esp_hal::peripherals::USB_DEVICE::regs();
    for chunk in bytes.chunks(32) {
        for &b in chunk {
            let mut tries = 0u32;
            while usb
                .ep1_conf()
                .read()
                .serial_in_ep_data_free()
                .bit_is_clear()
            {
                tries += 1;
                if tries > 100_000 {
                    return;
                }
            }
            // SAFETY: any byte value is valid for the FIFO write register.
            usb.ep1().write(|w| unsafe { w.rdwr_byte().bits(b) });
        }
        usb.ep1_conf().modify(|_, w| w.wr_done().set_bit());
    }
}
