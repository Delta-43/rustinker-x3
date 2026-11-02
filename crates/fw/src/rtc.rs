//! I²C bus (SDA GPIO20, SCL GPIO0) bring-up: a bus scan and the DS3231 RTC.
use core::fmt::Write;
use esp_hal::{Blocking, i2c::master::I2c};
use x3_core::rtc::{ADDRESS, DateTime, OSF, REG_STATUS, REG_TIME};

const REG_CONTROL: u8 = 0x0E;
/// Control bit 7: oscillator disabled while on the backup battery.
const EOSC: u8 = 0x80;

/// Lists every 7-bit address that acknowledges a one-byte read.
pub fn scan(i2c: &mut I2c<'_, Blocking>, out: &mut impl Write) {
    let mut found = 0;
    for addr in 0x08u8..0x78 {
        let mut b = [0u8; 1];
        if i2c.read(addr, &mut b).is_ok() {
            found += 1;
            let name = match addr {
                0x55 => " (BQ27220 gauge?)",
                0x68 => " (DS3231 RTC?)",
                0x6A | 0x6B => " (QMI8658 IMU?)",
                _ => "",
            };
            let _ = writeln!(out, "i2c: 0x{addr:02X}{name}");
        }
    }
    let _ = writeln!(out, "i2c: {found} devices");
}

/// Prints the time, the oscillator-stop flag and the control register.
pub fn show(i2c: &mut I2c<'_, Blocking>, out: &mut impl Write) {
    let mut regs = [0u8; 7];
    let mut status = [0u8; 1];
    let mut control = [0u8; 1];
    let r = i2c
        .write_read(ADDRESS, &[REG_TIME], &mut regs)
        .and_then(|_| i2c.write_read(ADDRESS, &[REG_STATUS], &mut status))
        .and_then(|_| i2c.write_read(ADDRESS, &[REG_CONTROL], &mut control));
    if r.is_err() {
        let _ = writeln!(out, "time: RTC read failed: {r:?}");
        return;
    }
    match DateTime::from_registers(&regs) {
        Some(t) => {
            let _ = write!(out, "time: {t}");
        }
        None => {
            let _ = write!(out, "time: invalid registers {regs:02x?}");
        }
    }
    let _ = writeln!(
        out,
        "; oscillator stopped since last set: {}; battery oscillator {}",
        status[0] & OSF != 0,
        if control[0] & EOSC != 0 {
            "DISABLED"
        } else {
            "on"
        }
    );
}

/// The stored time, or `None` if it is unreadable, invalid, or the
/// oscillator stopped since it was last set.
pub fn read(i2c: &mut I2c<'_, Blocking>) -> Option<DateTime> {
    let mut regs = [0u8; 7];
    let mut status = [0u8; 1];
    i2c.write_read(ADDRESS, &[REG_TIME], &mut regs)
        .and_then(|_| i2c.write_read(ADDRESS, &[REG_STATUS], &mut status))
        .ok()?;
    if status[0] & OSF != 0 {
        return None;
    }
    DateTime::from_registers(&regs)
}

/// Sets the RTC from the companion's `hello` time (PROTOCOL.md A.3) when
/// it is off by more than 60 s or not trustworthy. The RTC keeps UTC.
pub fn sync(i2c: &mut I2c<'_, Blocking>, unix: u64, out: &mut impl Write) {
    let Some(new) = DateTime::from_unix(unix) else {
        let _ = writeln!(
            out,
            "time: companion time {unix} is out of range; RTC left alone"
        );
        return;
    };
    let old = read(i2c);
    match old.and_then(|t| t.to_unix()) {
        Some(t) if t.abs_diff(unix) <= 60 => {
            let _ = writeln!(
                out,
                "time: RTC within {} s of the companion",
                t.abs_diff(unix)
            );
        }
        _ => match set(i2c, &new) {
            Ok(()) => {
                let _ = writeln!(out, "time: RTC set to {new} UTC (was {old:?})");
            }
            Err(e) => {
                let _ = writeln!(out, "time: {e}");
            }
        },
    }
}

/// Sets the time, clears the oscillator-stop flag and makes sure the
/// oscillator keeps running on the backup battery.
pub fn set(i2c: &mut I2c<'_, Blocking>, t: &DateTime) -> Result<(), &'static str> {
    let regs = t.to_registers().ok_or("invalid time")?;
    let mut msg = [0u8; 8];
    msg[0] = REG_TIME;
    msg[1..].copy_from_slice(&regs);
    i2c.write(ADDRESS, &msg).map_err(|_| "time write failed")?;
    let mut b = [0u8; 1];
    i2c.write_read(ADDRESS, &[REG_STATUS], &mut b)
        .map_err(|_| "status read failed")?;
    i2c.write(ADDRESS, &[REG_STATUS, b[0] & !OSF])
        .map_err(|_| "status write failed")?;
    i2c.write_read(ADDRESS, &[REG_CONTROL], &mut b)
        .map_err(|_| "control read failed")?;
    if b[0] & EOSC != 0 {
        i2c.write(ADDRESS, &[REG_CONTROL, b[0] & !EOSC])
            .map_err(|_| "control write failed")?;
    }
    Ok(())
}

fn gauge_word(i2c: &mut I2c<'_, Blocking>, reg: x3_core::battery::Reg) -> Option<u16> {
    let mut b = [0u8; 2];
    i2c.write_read(x3_core::battery::ADDRESS, &[reg as u8], &mut b)
        .ok()
        .map(|_| u16::from_le_bytes(b))
}

/// Every BQ27220 register of interest, decoded. Reads only.
pub fn gauge(i2c: &mut I2c<'_, Blocking>) -> x3_core::battery::Reading {
    use x3_core::battery::{ALL, Reg, decode};
    let mut raw = [(Reg::Voltage, None); ALL.len()];
    for (slot, reg) in raw.iter_mut().zip(ALL) {
        *slot = (reg, gauge_word(i2c, reg));
    }
    decode(&raw)
}

/// The gauge's present current in mA, positive while charging.
pub fn current_ma(i2c: &mut I2c<'_, Blocking>) -> Option<i16> {
    gauge_word(i2c, x3_core::battery::Reg::Current).map(|w| w as i16)
}

/// Reads every BQ27220 register of interest, prints raw words and the
/// decoded values. Reads only.
pub fn battery(i2c: &mut I2c<'_, Blocking>, out: &mut impl Write) {
    use x3_core::battery::{ALL, Reg, decode};
    let mut raw = [(Reg::Voltage, None); ALL.len()];
    for (slot, reg) in raw.iter_mut().zip(ALL) {
        let word = gauge_word(i2c, reg);
        *slot = (reg, word);
        let _ = writeln!(out, "battery: 0x{:02X} {reg:?} = {word:?}", reg as u8);
    }
    let _ = writeln!(out, "battery: {:?}", decode(&raw));
}

/// QMI8658 off: soft reset, then sensors and internal oscillator disabled
/// (CTRL1 bit 0). `imu` resets it again before use.
pub fn imu_off(i2c: &mut I2c<'_, Blocking>) {
    const ADDR: u8 = 0x6B;
    if i2c.write(ADDR, &[0x60, 0xB0]).is_ok() {
        esp_hal::delay::Delay::new().delay_millis(20);
        let _ = i2c.write(ADDR, &[0x08, 0x00]);
        let _ = i2c.write(ADDR, &[0x02, 0x01]);
    }
}

/// QMI8658 accelerometer: checks WHO_AM_I, enables ±8 g at 125 Hz and
/// prints one sample in milli-g. Configuration resets at power-off.
pub fn imu(i2c: &mut I2c<'_, Blocking>, out: &mut impl Write) {
    const ADDR: u8 = 0x6B;
    const WHO_AM_I: u8 = 0x00;
    const CTRL1: u8 = 0x02; // bit 6: address auto-increment
    const CTRL2: u8 = 0x03; // accel: bits 6:4 range (010 = ±8 g), 3:0 ODR (6 = 125 Hz)
    const CTRL7: u8 = 0x08; // bit 0: accel enable
    const AX_L: u8 = 0x35;
    let mut id = [0u8; 1];
    if i2c.write_read(ADDR, &[WHO_AM_I], &mut id).is_err() {
        let _ = writeln!(out, "imu: no answer at 0x6B");
        return;
    }
    let _ = writeln!(out, "imu: WHO_AM_I 0x{:02X} (expect 0x05)", id[0]);
    if id[0] != 0x05 {
        return;
    }
    // Soft reset first: other firmware can leave the sensor in a mode (likely
    // wake-on-motion via CTRL9) where data registers stop updating even
    // though plain control writes read back fine.
    const RESET: u8 = 0x60;
    if i2c.write(ADDR, &[RESET, 0xB0]).is_err() {
        let _ = writeln!(out, "imu: reset write failed");
        return;
    }
    esp_hal::delay::Delay::new().delay_millis(50);
    if i2c.write_read(ADDR, &[WHO_AM_I], &mut id).is_err() || id[0] != 0x05 {
        let _ = writeln!(out, "imu: no WHO_AM_I after reset ({:02X})", id[0]);
        return;
    }
    let setup = [(CTRL1, 0x40), (CTRL2, 0x26), (CTRL7, 0x01)];
    for (reg, val) in setup {
        if i2c.write(ADDR, &[reg, val]).is_err() {
            let _ = writeln!(out, "imu: write 0x{reg:02X} failed");
            return;
        }
    }
    esp_hal::delay::Delay::new().delay_millis(50);
    // Scale by the range the sensor reports, not the one requested: the
    // first run read 4× high, so the CTRL2 write had not taken.
    let mut ctrl = [0u8; 3];
    if i2c.write_read(ADDR, &[CTRL1], &mut ctrl).is_err() {
        let _ = writeln!(out, "imu: control read failed");
        return;
    }
    let lsb_per_g: i32 = 16384 >> ((ctrl[1] >> 4) & 0x03);
    let _ = writeln!(
        out,
        "imu: CTRL1 0x{:02X} CTRL2 0x{:02X} -> ±{} g",
        ctrl[0],
        ctrl[1],
        2 << ((ctrl[1] >> 4) & 0x03)
    );
    let mut c7 = [0u8; 1];
    let _ = i2c.write_read(ADDR, &[CTRL7], &mut c7);
    let _ = writeln!(out, "imu: CTRL7 0x{:02X} (expect 0x01)", c7[0]);
    let delay = esp_hal::delay::Delay::new();
    for n in 0..5 {
        // 0x2E STATUS0 (bit 0: new accel data), 0x30–0x32 sample timestamp,
        // 0x35–0x3A accel X, Y, Z.
        let mut st = [0u8; 1];
        let mut ts = [0u8; 3];
        let mut b = [0u8; 6];
        if i2c.write_read(ADDR, &[0x2E], &mut st).is_err()
            || i2c.write_read(ADDR, &[0x30], &mut ts).is_err()
            || i2c.write_read(ADDR, &[AX_L], &mut b).is_err()
        {
            let _ = writeln!(out, "imu: read failed");
            return;
        }
        let stamp = u32::from_le_bytes([ts[0], ts[1], ts[2], 0]);
        let mg = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]) as i32 * 1000 / lsb_per_g;
        let (x, y, z) = (mg(0), mg(2), mg(4));
        let axis = [("x", x), ("y", y), ("z", z)]
            .into_iter()
            .max_by_key(|(_, v)| v.abs())
            .map(|(n, v)| (n, if v < 0 { "-" } else { "+" }));
        let _ = writeln!(
            out,
            "imu {n}: status 0x{:02X} stamp {stamp} x {x} y {y} z {z} mg; gravity {}{}",
            st[0],
            axis.map_or("?", |a| a.1),
            axis.map_or("?", |a| a.0)
        );
        delay.delay_millis(100);
    }
}
