use std::string::ToString;
use x3_core::rtc::*;

fn dt(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

#[test]
fn round_trip_registers_and_text() {
    let t = dt(2026, 10, 27, 1, 45, 9);
    let r = t.to_registers().unwrap();
    assert_eq!(r, [0x09, 0x45, 0x01, 2, 0x27, 0x10, 0x26]); // Tuesday
    assert_eq!(DateTime::from_registers(&r), Some(t));
    assert_eq!(t.to_string(), "2026-10-27 01:45:09");
    assert_eq!(DateTime::parse(b"2026-10-27 01:45:09"), Some(t));
}

#[test]
fn weekdays() {
    assert_eq!(dt(2000, 1, 1, 0, 0, 0).weekday(), 6); // Saturday
    assert_eq!(dt(2024, 2, 29, 0, 0, 0).weekday(), 4); // Thursday
    assert_eq!(dt(2026, 11, 1, 0, 0, 0).weekday(), 7); // Sunday
}

#[test]
fn twelve_hour_and_century_bits() {
    // 12 h mode: 12 AM is 00, 12 PM is 12, 7 PM is 19.
    let mut r = [0, 0, 0x40 | 0x12, 1, 0x01, 0x01, 0x26];
    assert_eq!(DateTime::from_registers(&r).unwrap().hour, 0);
    r[2] = 0x40 | 0x20 | 0x12;
    assert_eq!(DateTime::from_registers(&r).unwrap().hour, 12);
    r[2] = 0x40 | 0x20 | 0x07;
    assert_eq!(DateTime::from_registers(&r).unwrap().hour, 19);
    r[2] = 0x40 | 0x13; // 13 in 12 h mode
    assert_eq!(DateTime::from_registers(&r), None);
    // Century bit pushes past the supported range.
    assert_eq!(DateTime::from_registers(&[0, 0, 0, 1, 1, 0x81, 0]), None);
}

#[test]
fn bad_registers_are_rejected() {
    assert_eq!(DateTime::from_registers(&[0xFF; 7]), None);
    assert_eq!(DateTime::from_registers(&[0; 7]), None); // month 0
    assert_eq!(DateTime::from_registers(&[0x5A, 0, 0, 1, 1, 1, 0]), None); // BCD A
    assert_eq!(DateTime::from_registers(&[0x60, 0, 0, 1, 1, 1, 0]), None); // 60 s
    assert_eq!(
        DateTime::from_registers(&[0, 0, 0, 1, 0x29, 0x02, 0x25]),
        None
    ); // 2025-02-29
    assert!(DateTime::from_registers(&[0, 0, 0, 1, 0x29, 0x02, 0x24]).is_some()); // 2024-02-29
}

#[test]
fn bad_text_is_rejected() {
    for s in [
        &b""[..],
        b"2026-10-27",
        b"2026-10-27 01:45:9",
        b"2026/10/27 01:45:09",
        b"2026-13-01 00:00:00",
        b"2026-04-31 00:00:00",
        b"2026-10-27 24:00:00",
        b"1999-12-31 23:59:59",
        b"2100-01-01 00:00:00",
        b"2026-10-27 01:45:0x",
        b"2026-10-27 01:45:09 ",
    ] {
        assert_eq!(
            DateTime::parse(s),
            None,
            "{}",
            std::str::from_utf8(s).unwrap()
        );
    }
    assert_eq!(dt(2026, 0, 1, 0, 0, 0).to_registers(), None);
}

#[test]
fn unix_time() {
    assert_eq!(
        DateTime::from_unix(946_684_800),
        Some(dt(2000, 1, 1, 0, 0, 0))
    );
    // The companion's time in the first device Receive.
    assert_eq!(
        DateTime::from_unix(1_793_097_388),
        Some(dt(2026, 10, 27, 10, 36, 28))
    );
    assert_eq!(
        DateTime::from_unix(951_782_400),
        Some(dt(2000, 2, 29, 0, 0, 0))
    );
    assert_eq!(
        DateTime::from_unix(4_102_444_799),
        Some(dt(2099, 12, 31, 23, 59, 59))
    );
    assert_eq!(DateTime::from_unix(4_102_444_800), None);
    assert_eq!(DateTime::from_unix(946_684_799), None);
    assert_eq!(DateTime::from_unix(u64::MAX), None);
    for secs in (946_684_800..4_102_444_800).step_by(86_399 * 7 + 13) {
        assert_eq!(DateTime::from_unix(secs).unwrap().to_unix(), Some(secs));
    }
    assert_eq!(dt(2026, 2, 30, 0, 0, 0).to_unix(), None);
}

#[test]
fn central_europe_follows_the_eu_summer_time_rule() {
    let at = |year, month, day, hour, minute, second| DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    };
    let local = |t: DateTime| {
        let l = t.central_europe().unwrap();
        (l.month, l.day, l.hour, l.minute, l.second)
    };
    // Winter: +1 h. 2026-10-31 is after the October change on the 25th.
    assert_eq!(local(at(2026, 10, 31, 0, 53, 0)), (10, 31, 1, 53, 0));
    assert_eq!(local(at(2026, 1, 15, 23, 30, 0)), (1, 16, 0, 30, 0));
    // Summer: +2 h.
    assert_eq!(local(at(2026, 7, 1, 12, 0, 0)), (7, 1, 14, 0, 0));
    // The switch-over seconds: 2026-03-29 and 2026-10-25, both at 01:00 UTC.
    assert_eq!(local(at(2026, 3, 29, 0, 59, 59)), (3, 29, 1, 59, 59));
    assert_eq!(local(at(2026, 3, 29, 1, 0, 0)), (3, 29, 3, 0, 0));
    assert_eq!(local(at(2026, 10, 25, 0, 59, 59)), (10, 25, 2, 59, 59));
    assert_eq!(local(at(2026, 10, 25, 1, 0, 0)), (10, 25, 2, 0, 0));
    // Another year whose last Sunday falls on the 31st (2027-10-31).
    assert_eq!(local(at(2027, 10, 31, 0, 59, 59)), (10, 31, 2, 59, 59));
    assert_eq!(local(at(2027, 10, 31, 1, 0, 0)), (10, 31, 2, 0, 0));
    // An invalid date gives nothing rather than a guess.
    assert!(at(2026, 2, 31, 0, 0, 0).central_europe().is_none());
}
