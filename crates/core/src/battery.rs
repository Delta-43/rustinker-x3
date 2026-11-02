//! BQ27220 fuel gauge standard commands: two-byte little-endian reads.
//!
//! Register map per CrossPoint, MarigoldOS and the stock-firmware notes
//! (HARDWARE.md). 0x0C and 0x14 are both read because the references
//! disagree on which current to use.

pub const ADDRESS: u8 = 0x55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reg {
    Temperature = 0x06,
    Voltage = 0x08,
    BatteryStatus = 0x0A,
    Current = 0x0C,
    RemainingCapacity = 0x10,
    FullChargeCapacity = 0x12,
    AverageCurrent = 0x14,
    StateOfCharge = 0x2C,
    StateOfHealth = 0x2E,
    DesignCapacity = 0x3C,
}

pub const ALL: [Reg; 10] = [
    Reg::Temperature,
    Reg::Voltage,
    Reg::BatteryStatus,
    Reg::Current,
    Reg::RemainingCapacity,
    Reg::FullChargeCapacity,
    Reg::AverageCurrent,
    Reg::StateOfCharge,
    Reg::StateOfHealth,
    Reg::DesignCapacity,
];

/// Decoded readings. `None` where a read failed or a value is out of range.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub millivolts: Option<u16>,
    pub percent: Option<u8>,
    /// Positive while charging.
    pub current_ma: Option<i16>,
    pub average_ma: Option<i16>,
    /// Tenths of a degree Celsius.
    pub deci_celsius: Option<i16>,
    pub remaining_mah: Option<u16>,
    pub full_mah: Option<u16>,
    pub health_percent: Option<u8>,
    pub design_mah: Option<u16>,
    pub status: Option<u16>,
}

/// Builds a reading from raw register words (`None` for failed reads).
pub fn decode(raw: &[(Reg, Option<u16>)]) -> Reading {
    let mut r = Reading::default();
    for &(reg, word) in raw {
        let Some(w) = word else { continue };
        match reg {
            // 0.1 K; plausible range −40 to +85 °C.
            Reg::Temperature => {
                let dc = w as i32 - 2732;
                if (-400..=850).contains(&dc) {
                    r.deci_celsius = Some(dc as i16);
                }
            }
            Reg::Voltage => r.millivolts = (2500..=4600).contains(&w).then_some(w),
            Reg::BatteryStatus => r.status = Some(w),
            Reg::Current => r.current_ma = Some(w as i16),
            Reg::AverageCurrent => r.average_ma = Some(w as i16),
            Reg::RemainingCapacity => r.remaining_mah = (w <= 10_000).then_some(w),
            Reg::FullChargeCapacity => r.full_mah = (w <= 10_000).then_some(w),
            Reg::StateOfCharge => r.percent = (w <= 100).then_some(w as u8),
            Reg::StateOfHealth => r.health_percent = (w <= 100).then_some(w as u8),
            Reg::DesignCapacity => r.design_mah = (w <= 10_000).then_some(w),
        }
    }
    r
}
