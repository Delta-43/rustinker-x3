use x3_core::Error;
use x3_core::otadata::*;

fn raw(seq: u32, state: u32) -> [u8; ENTRY_LEN] {
    let mut e = [0xFF; ENTRY_LEN];
    e[0..4].copy_from_slice(&seq.to_le_bytes());
    e[24..28].copy_from_slice(&state.to_le_bytes());
    e[28..32].copy_from_slice(&crc(&seq.to_le_bytes()).to_le_bytes());
    e
}
fn entry(seq: u32, state: u32) -> Option<Entry> {
    Entry::parse(&raw(seq, state))
}
/// Applies a write the way the firmware does and re-reads both entries.
fn apply(entries: &[Option<Entry>; 2], w: &Write) -> [Option<Entry>; 2] {
    let mut out = *entries;
    out[w.sector] = Entry::parse(&w.entry);
    out
}

#[test]
fn crc_matches_rom_value() {
    assert_eq!(crc(&1u32.to_le_bytes()), 0x4743_989A);
}

#[test]
fn parse_reads_erased_bad_and_good_entries() {
    assert_eq!(Entry::parse(&[0xFF; ENTRY_LEN]), None);
    let good = entry(7, 2).unwrap();
    assert!(good.crc_ok && good.bootable());
    let mut bad = raw(7, 2);
    bad[28] ^= 1;
    assert!(!Entry::parse(&bad).unwrap().bootable());
    assert!(!entry(7, STATE_ABORTED).unwrap().bootable());
    assert!(!entry(7, STATE_INVALID).unwrap().bootable());
    assert!(entry(7, STATE_UNDEFINED).unwrap().bootable());
}

#[test]
fn owner_unit_round_trip() {
    // The owner's backup: CrossPoint in ota_0 at seq 7, older entry seq 6.
    let backup = [entry(7, 2), entry(6, 2)];
    assert_eq!(booted_slot(&backup, 2), Some(0));
    let to_ours = select(&backup, 2, 1).unwrap();
    assert_eq!(to_ours.sector, 1);
    let ours = apply(&backup, &to_ours);
    assert_eq!(ours[1].unwrap().seq, 8);
    assert_eq!(booted_slot(&ours, 2), Some(1));
    // "Back to CrossPoint" from the firmware overwrites the older entry.
    let back = select(&ours, 2, 0).unwrap();
    assert_eq!(back.sector, 0);
    let crosspoint = apply(&ours, &back);
    assert_eq!(crosspoint[0].unwrap().seq, 9);
    assert_eq!(booted_slot(&crosspoint, 2), Some(0));
    assert_eq!(ours[1], crosspoint[1]);
}

#[test]
fn blank_and_unbootable_entries() {
    assert_eq!(booted_slot(&[None, None], 2), None);
    let w = select(&[None, None], 2, 1).unwrap();
    assert_eq!((w.sector, Entry::parse(&w.entry).unwrap().seq), (0, 2));
    let ignored = [entry(9, STATE_ABORTED), entry(3, 2)];
    assert_eq!(booted_slot(&ignored, 2), Some(0));
    let w = select(&ignored, 2, 1).unwrap();
    assert_eq!(w.sector, 0);
    assert_eq!(booted_slot(&apply(&ignored, &w), 2), Some(1));
}

#[test]
fn hostile_values_are_rejected() {
    let e = [entry(7, 2), None];
    assert_eq!(select(&e, 2, 2), Err(Error::Invalid));
    assert_eq!(select(&e, 0, 0), Err(Error::Invalid));
    let top = [entry(u32::MAX - 1, 2), None];
    assert_eq!(select(&top, 2, 1), Err(Error::TooLarge));
    assert_eq!(select(&top, 2, 0), Err(Error::TooLarge));
    assert_eq!(booted_slot(&e, 0), None);
}
