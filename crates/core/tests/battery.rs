use x3_core::battery::*;

#[test]
fn decodes_typical_charging_reading() {
    let r = decode(&[
        (Reg::Voltage, Some(4123)),
        (Reg::StateOfCharge, Some(99)),
        (Reg::Current, Some(152)),
        (Reg::AverageCurrent, Some((-35i16) as u16)),
        (Reg::Temperature, Some(2982)), // 25.0 °C
        (Reg::DesignCapacity, Some(642)),
        (Reg::StateOfHealth, Some(100)),
    ]);
    assert_eq!(r.millivolts, Some(4123));
    assert_eq!(r.percent, Some(99));
    assert_eq!(r.current_ma, Some(152));
    assert_eq!(r.average_ma, Some(-35));
    assert_eq!(r.deci_celsius, Some(250));
    assert_eq!(r.design_mah, Some(642));
    assert_eq!(r.health_percent, Some(100));
    assert_eq!(r.full_mah, None);
}

#[test]
fn implausible_and_failed_reads_are_dropped() {
    let r = decode(&[
        (Reg::Voltage, Some(0xFFFF)),
        (Reg::StateOfCharge, Some(101)),
        (Reg::Temperature, Some(0)),
        (Reg::StateOfHealth, Some(0xFFFF)),
        (Reg::DesignCapacity, None),
        (Reg::FullChargeCapacity, Some(65535)),
    ]);
    assert_eq!(r, Reading::default());
    assert_eq!(decode(&[]), Reading::default());
}

#[test]
fn every_register_is_listed_once() {
    for (i, a) in ALL.iter().enumerate() {
        assert!(ALL[i + 1..].iter().all(|b| a != b));
    }
}
