//! DS3231 time registers 0x00–0x06 (BCD) and date-time text, validated.
//!
//! Registers: seconds, minutes, hours (24 h mode used; 12 h is decoded),
//! weekday 1–7, date, month (bit 7 = century), year 00–99 from 2000.
//! Status register 0x0F bit 7 (OSF) set means the oscillator stopped, so
//! the time is not trustworthy until set again.

pub const ADDRESS: u8 = 0x68;
pub const REG_TIME: u8 = 0x00;
pub const REG_STATUS: u8 = 0x0F;
pub const OSF: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn bcd(b: u8) -> Option<u8> {
    let (hi, lo) = (b >> 4, b & 0x0F);
    (hi <= 9 && lo <= 9).then_some(hi * 10 + lo)
}

fn to_bcd(v: u8) -> u8 {
    (v / 10) << 4 | (v % 10)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl DateTime {
    /// 2000-01-01 to 2099-12-31, a real calendar date, 24 h time.
    pub fn valid(&self) -> bool {
        (2000..=2099).contains(&self.year)
            && (1..=12).contains(&self.month)
            && (1..=days_in_month(self.year, self.month)).contains(&self.day)
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
    }

    /// Decodes registers 0x00–0x06. `None` for bad BCD or an invalid date.
    pub fn from_registers(r: &[u8; 7]) -> Option<Self> {
        let hour = if r[2] & 0x40 != 0 {
            // 12-hour mode: bit 5 is PM.
            let h = bcd(r[2] & 0x1F)?;
            if !(1..=12).contains(&h) {
                return None;
            }
            (h % 12) + if r[2] & 0x20 != 0 { 12 } else { 0 }
        } else {
            bcd(r[2] & 0x3F)?
        };
        let century = if r[5] & 0x80 != 0 { 100 } else { 0 };
        let t = Self {
            year: 2000 + century + bcd(r[6])? as u16,
            month: bcd(r[5] & 0x1F)?,
            day: bcd(r[4] & 0x3F)?,
            hour,
            minute: bcd(r[1] & 0x7F)?,
            second: bcd(r[0] & 0x7F)?,
        };
        t.valid().then_some(t)
    }

    /// Registers 0x00–0x06 in 24 h mode; weekday from the date (Monday 1).
    pub fn to_registers(&self) -> Option<[u8; 7]> {
        if !self.valid() {
            return None;
        }
        Some([
            to_bcd(self.second),
            to_bcd(self.minute),
            to_bcd(self.hour),
            self.weekday(),
            to_bcd(self.day),
            to_bcd(self.month),
            to_bcd((self.year - 2000) as u8),
        ])
    }

    /// ISO weekday, Monday 1 to Sunday 7 (Sakamoto's method).
    pub fn weekday(&self) -> u8 {
        const T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if self.month < 3 {
            self.year - 1
        } else {
            self.year
        };
        let d = (y + y / 4 - y / 100 + y / 400 + T[self.month as usize - 1] + self.day as u16) % 7;
        if d == 0 { 7 } else { d as u8 }
    }

    /// This UTC time as Central European local time (CET, CEST from the last
    /// Sunday of March 01:00 UTC to the last Sunday of October 01:00 UTC).
    /// The device's one built-in zone until a setting exists.
    pub fn central_europe(&self) -> Option<Self> {
        let last_sunday = |month: u8| {
            let last = Self {
                day: 31,
                month,
                hour: 1,
                minute: 0,
                second: 0,
                year: self.year,
            };
            let day = 31 - last.weekday() % 7;
            Self { day, ..last }.to_unix()
        };
        let now = self.to_unix()?;
        let summer = last_sunday(3)? <= now && now < last_sunday(10)?;
        Self::from_unix(now + if summer { 7200 } else { 3600 })
    }

    /// UTC from Unix seconds; `None` outside 2000–2099.
    /// Days to a civil date per Howard Hinnant's `civil_from_days`.
    pub fn from_unix(secs: u64) -> Option<Self> {
        let days = secs / 86_400;
        let rem = secs % 86_400;
        let z = days.checked_add(719_468)?;
        let era = z / 146_097;
        let doe = z % 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = yoe + era * 400 + u64::from(month <= 2);
        let t = Self {
            year: u16::try_from(year).ok()?,
            month,
            day,
            hour: (rem / 3600) as u8,
            minute: (rem / 60 % 60) as u8,
            second: (rem % 60) as u8,
        };
        t.valid().then_some(t)
    }

    /// Unix seconds, for a valid date (`days_from_civil`).
    pub fn to_unix(&self) -> Option<u64> {
        if !self.valid() {
            return None;
        }
        let y = u64::from(self.year) - u64::from(self.month <= 2);
        let m = u64::from(self.month);
        let era = y / 400;
        let yoe = y % 400;
        let doy = (153 * if m > 2 { m - 3 } else { m + 9 } + 2) / 5 + u64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(
            days * 86_400
                + u64::from(self.hour) * 3600
                + u64::from(self.minute) * 60
                + u64::from(self.second),
        )
    }

    /// Parses exactly `YYYY-MM-DD HH:MM:SS`.
    pub fn parse(s: &[u8]) -> Option<Self> {
        if s.len() != 19
            || s[4] != b'-'
            || s[7] != b'-'
            || s[10] != b' '
            || s[13] != b':'
            || s[16] != b':'
        {
            return None;
        }
        let num = |a: usize, b: usize| -> Option<u16> {
            s[a..b].iter().try_fold(0u16, |n, &c| {
                c.is_ascii_digit().then(|| n * 10 + (c - b'0') as u16)
            })
        };
        let t = Self {
            year: num(0, 4)?,
            month: num(5, 7)? as u8,
            day: num(8, 10)? as u8,
            hour: num(11, 13)? as u8,
            minute: num(14, 16)? as u8,
            second: num(17, 19)? as u8,
        };
        t.valid().then_some(t)
    }
}

impl core::fmt::Display for DateTime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}
