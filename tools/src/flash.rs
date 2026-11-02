//! Read-only inspection of an ESP32-C3 flash dump (bootloader, partition table,
//! otadata and app headers), plus building a replacement otadata file from a
//! dump. Nothing here touches a device; flashing stays with the owner.
use std::fmt::Write;

pub const TABLE_OFFSET: usize = 0x8000;
const TABLE_MAX: usize = 0xC00;
const IMAGE_MAGIC: u8 = 0xE9;
const APP_DESC_MAGIC: u32 = 0xABCD_5432;
const CHIP_ESP32C3: u16 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub kind: u8,
    pub subtype: u8,
    pub offset: u32,
    pub size: u32,
    pub label: String,
    pub flags: u32,
}

impl Partition {
    fn kind_name(&self) -> String {
        match (self.kind, self.subtype) {
            (0, 0x00) => "app factory".into(),
            (0, s @ 0x10..=0x1F) => format!("app ota_{}", s - 0x10),
            (0, 0x20) => "app test".into(),
            (1, 0x00) => "data ota".into(),
            (1, 0x01) => "data phy".into(),
            (1, 0x02) => "data nvs".into(),
            (1, 0x03) => "data coredump".into(),
            (1, 0x04) => "data nvs_keys".into(),
            (1, 0x81) => "data fat".into(),
            (1, 0x82) => "data spiffs".into(),
            (1, 0x83) => "data littlefs".into(),
            (k, s) => format!("type {k:#04x}/{s:#04x}"),
        }
    }
    fn ota_index(&self) -> Option<u8> {
        (self.kind == 0 && (0x10..=0x1F).contains(&self.subtype)).then(|| self.subtype - 0x10)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub chip_id: u16,
    pub segments: u8,
    pub project: String,
    pub version: String,
    pub idf: String,
    pub built: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaEntry {
    pub seq: u32,
    pub state: u32,
    pub crc_ok: bool,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
/// A NUL-padded C string, with anything but printable ASCII shown as `?`.
fn text(b: &[u8]) -> String {
    b.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            if (0x20..0x7F).contains(&c) {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}

/// The ROM's `esp_rom_crc32_le(UINT32_MAX, …)` that otadata uses.
pub fn otadata_crc(bytes: &[u8]) -> u32 {
    x3_core::otadata::crc(bytes)
}

/// Parses the table at 0x8000. `Err` means no readable table (blank, damaged
/// or encrypted flash).
pub fn partitions(dump: &[u8]) -> Result<(Vec<Partition>, bool), String> {
    let table = dump
        .get(TABLE_OFFSET..TABLE_OFFSET + TABLE_MAX)
        .ok_or("dump ends before the partition table")?;
    let mut out = Vec::new();
    let mut md5_entry = false;
    for e in table.as_chunks::<32>().0 {
        match (e[0], e[1]) {
            (0xAA, 0x50) => out.push(Partition {
                kind: e[2],
                subtype: e[3],
                offset: u32_at(e, 4).unwrap_or(0),
                size: u32_at(e, 8).unwrap_or(0),
                label: text(&e[12..28]),
                flags: u32_at(e, 28).unwrap_or(0),
            }),
            (0xEB, 0xEB) => md5_entry = true,
            (0xFF, 0xFF) => break,
            _ => return Err(format!("unknown entry {:02x} {:02x}", e[0], e[1])),
        }
    }
    if out.is_empty() {
        return Err("no partition entries (blank or encrypted flash?)".into());
    }
    Ok((out, md5_entry))
}

/// Reads the image header and, for apps, `esp_app_desc_t` in the first segment.
pub fn image(dump: &[u8], offset: usize) -> Option<AppInfo> {
    let h = dump.get(offset..offset + 24)?;
    if h[0] != IMAGE_MAGIC {
        return None;
    }
    let desc = offset + 24 + 8;
    let mut info = AppInfo {
        chip_id: u16_at(h, 12)?,
        segments: h[1],
        project: String::new(),
        version: String::new(),
        idf: String::new(),
        built: String::new(),
    };
    if u32_at(dump, desc) == Some(APP_DESC_MAGIC) {
        let d = dump.get(desc..desc + 160)?;
        info.version = text(&d[16..48]);
        info.project = text(&d[48..80]);
        info.built = format!("{} {}", text(&d[96..112]), text(&d[80..96]));
        info.idf = text(&d[112..144]);
    }
    Some(info)
}

/// The two otadata entries (one per 4 KiB sector), `None` where blank.
pub fn otadata(dump: &[u8], p: &Partition) -> [Option<OtaEntry>; 2] {
    let read = |i: usize| -> Option<OtaEntry> {
        let at = p.offset as usize + i * 0x1000;
        let e = dump.get(at..at + 32)?;
        let seq = u32_at(e, 0)?;
        if seq == u32::MAX {
            return None;
        }
        Some(OtaEntry {
            seq,
            state: u32_at(e, 24)?,
            crc_ok: u32_at(e, 28)? == otadata_crc(&e[0..4]),
        })
    };
    [read(0), read(1)]
}

fn state_name(s: u32) -> &'static str {
    match s {
        0 => "new",
        1 => "pending verify",
        2 => "valid",
        3 => "invalid",
        4 => "aborted",
        u32::MAX => "undefined",
        _ => "unknown",
    }
}

/// A plain-text report for MEMORY.md. Read-only; no advice to write anything.
pub fn report(dump: &[u8]) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "Dump size: {} bytes ({:#x})", dump.len(), dump.len());
    if dump.len() != 0x100_0000 {
        let _ = writeln!(r, "WARNING: not 16 MiB. Was the whole flash read?");
    }
    match image(dump, 0) {
        Some(b) => {
            let chip = if b.chip_id == CHIP_ESP32C3 {
                "ESP32-C3"
            } else {
                "NOT ESP32-C3"
            };
            let _ = writeln!(
                r,
                "Bootloader at 0x0: image header found, chip id {} ({chip}), {} segments",
                b.chip_id, b.segments
            );
        }
        None => {
            let _ = writeln!(
                r,
                "Bootloader at 0x0: no image header (blank or encrypted?)"
            );
        }
    }
    let (parts, md5) = match partitions(dump) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(r, "Partition table at {TABLE_OFFSET:#x}: {e}");
            return r;
        }
    };
    let md5 = if md5 {
        "MD5 entry present, not checked"
    } else {
        "no MD5 entry"
    };
    let _ = writeln!(
        r,
        "Partition table at {TABLE_OFFSET:#x}: {} entries, {md5}",
        parts.len()
    );
    let _ = writeln!(
        r,
        "  {:<16} {:<15} {:>10} {:>10}  flags",
        "label", "type", "offset", "size"
    );
    for p in &parts {
        let _ = writeln!(
            r,
            "  {:<16} {:<15} {:>#10x} {:>#10x}  {:#x}",
            p.label,
            p.kind_name(),
            p.offset,
            p.size,
            p.flags
        );
        if p.offset as usize + p.size as usize > dump.len() {
            let _ = writeln!(r, "  WARNING: {} runs past the end of the dump", p.label);
        }
    }
    let apps: Vec<_> = parts.iter().filter(|p| p.kind == 0).collect();
    for p in &apps {
        match image(dump, p.offset as usize) {
            Some(a) if !a.project.is_empty() => {
                let _ = writeln!(
                    r,
                    "App {}: {} {} (IDF {}, built {})",
                    p.label, a.project, a.version, a.idf, a.built
                );
            }
            Some(a) => {
                let _ = writeln!(
                    r,
                    "App {}: image header, chip id {}, no app description",
                    p.label, a.chip_id
                );
            }
            None => {
                let _ = writeln!(r, "App {}: empty (no image header)", p.label);
            }
        }
    }
    let ota_slots = apps.iter().filter_map(|p| p.ota_index()).count() as u32;
    if let Some(od) = parts.iter().find(|p| p.kind == 1 && p.subtype == 0) {
        let entries = otadata(dump, od);
        for (i, e) in entries.iter().enumerate() {
            match e {
                Some(e) => {
                    let crc = if e.crc_ok { "CRC ok" } else { "CRC BAD" };
                    let _ = writeln!(
                        r,
                        "otadata[{i}]: seq {}, state {}, {crc}",
                        e.seq,
                        state_name(e.state)
                    );
                }
                None => {
                    let _ = writeln!(r, "otadata[{i}]: blank");
                }
            }
        }
        let best = entries
            .iter()
            .flatten()
            .filter(|e| e.crc_ok && e.state != 3 && e.state != 4)
            .max_by_key(|e| e.seq);
        match (best, ota_slots) {
            (Some(e), n) if n > 0 => {
                let _ = writeln!(
                    r,
                    "Boots: ota_{} (seq {} mod {n} slots)",
                    (e.seq - 1) % n,
                    e.seq
                );
            }
            _ => {
                let _ = writeln!(r, "Boots: first app (otadata blank: factory, else ota_0)");
            }
        }
    }
    r
}

const OTADATA_SIZE: usize = x3_core::otadata::PARTITION_LEN;

/// A replacement otadata partition (both 4 KiB sectors) that boots app slot
/// `slot`, built from the dump's own otadata. The active entry is kept, so the
/// old slot stays one entry away; the other entry gets the next sequence
/// number that selects `slot`, with state "undefined", which ESP-IDF's
/// bootloader treats as bootable with or without rollback.
pub fn otadata_select(dump: &[u8], slot: u32) -> Result<Vec<u8>, String> {
    let (parts, _) = partitions(dump)?;
    let od = parts
        .iter()
        .find(|p| p.kind == 1 && p.subtype == 0)
        .ok_or("no otadata partition")?;
    if od.size as usize != OTADATA_SIZE {
        return Err(format!("otadata is {:#x} bytes, expected 0x2000", od.size));
    }
    let slots = parts.iter().filter_map(|p| p.ota_index()).count() as u32;
    if slot >= slots {
        return Err(format!("slot {slot} does not exist ({slots} OTA slots)"));
    }
    let start = od.offset as usize;
    let mut out = dump
        .get(start..start + OTADATA_SIZE)
        .ok_or("dump ends before otadata")?
        .to_vec();
    let entries = otadata(dump, od).map(|e| {
        e.map(|e| x3_core::otadata::Entry {
            seq: e.seq,
            state: e.state,
            crc_ok: e.crc_ok,
        })
    });
    let w = x3_core::otadata::select(&entries, slots, slot)
        .map_err(|e| format!("cannot select slot {slot}: {e:?}"))?;
    let at = w.sector * x3_core::otadata::SECTOR_LEN;
    out[at..at + x3_core::otadata::SECTOR_LEN].fill(0xFF);
    out[at..at + w.entry.len()].copy_from_slice(&w.entry);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: u8, sub: u8, offset: u32, size: u32, label: &str) -> [u8; 32] {
        let mut e = [0u8; 32];
        e[0] = 0xAA;
        e[1] = 0x50;
        e[2] = kind;
        e[3] = sub;
        e[4..8].copy_from_slice(&offset.to_le_bytes());
        e[8..12].copy_from_slice(&size.to_le_bytes());
        e[12..12 + label.len()].copy_from_slice(label.as_bytes());
        e
    }
    fn app(dump: &mut [u8], at: usize, project: &str, version: &str) {
        dump[at] = IMAGE_MAGIC;
        dump[at + 1] = 4;
        dump[at + 12..at + 14].copy_from_slice(&CHIP_ESP32C3.to_le_bytes());
        let d = at + 32;
        dump[d..d + 256].fill(0);
        dump[d..d + 4].copy_from_slice(&APP_DESC_MAGIC.to_le_bytes());
        dump[d + 16..d + 16 + version.len()].copy_from_slice(version.as_bytes());
        dump[d + 48..d + 48 + project.len()].copy_from_slice(project.as_bytes());
        dump[d + 112..d + 117].copy_from_slice(b"v5.4\0");
    }
    fn ota(dump: &mut [u8], at: usize, seq: u32, state: u32) {
        dump[at..at + 4].copy_from_slice(&seq.to_le_bytes());
        dump[at + 24..at + 28].copy_from_slice(&state.to_le_bytes());
        let crc = otadata_crc(&seq.to_le_bytes());
        dump[at + 28..at + 32].copy_from_slice(&crc.to_le_bytes());
    }
    /// The candidate CrossPoint layout from docs/HARDWARE.md, in erased flash.
    fn crosspoint_like() -> Vec<u8> {
        let mut d = vec![0xFF; 0x100_0000];
        d[0] = IMAGE_MAGIC;
        d[12..14].copy_from_slice(&CHIP_ESP32C3.to_le_bytes());
        let table = [
            entry(1, 0x02, 0x9000, 0x5000, "nvs"),
            entry(1, 0x00, 0xE000, 0x2000, "otadata"),
            entry(0, 0x10, 0x10000, 0x640000, "app0"),
            entry(0, 0x11, 0x650000, 0x640000, "app1"),
            entry(1, 0x82, 0xC90000, 0x360000, "spiffs"),
            entry(1, 0x03, 0xFF0000, 0x10000, "coredump"),
        ];
        for (i, e) in table.iter().enumerate() {
            d[TABLE_OFFSET + i * 32..TABLE_OFFSET + i * 32 + 32].copy_from_slice(e);
        }
        app(&mut d, 0x10000, "crosspoint", "1.6.4");
        app(&mut d, 0x650000, "crosspoint", "1.6.5");
        d
    }

    #[test]
    fn otadata_crc_matches_known_value() {
        // seq 1 as written by ESP-IDF: 9a 98 43 47.
        assert_eq!(otadata_crc(&1u32.to_le_bytes()), 0x4743_989A);
    }

    #[test]
    fn reads_candidate_layout_and_active_slot() {
        let mut d = crosspoint_like();
        ota(&mut d, 0xE000, 1, 2);
        ota(&mut d, 0xF000, 2, 2);
        let (parts, _) = partitions(&d).unwrap();
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[3].label, "app1");
        let r = report(&d);
        assert!(r.contains("App app1: crosspoint 1.6.5"), "{r}");
        assert!(r.contains("Boots: ota_1"), "{r}");
        assert!(!r.contains("WARNING"), "{r}");
    }

    #[test]
    fn bad_crc_and_invalid_slots_are_skipped() {
        let mut d = crosspoint_like();
        ota(&mut d, 0xE000, 1, 2);
        ota(&mut d, 0xF000, 2, 2);
        d[0xF000 + 28] ^= 1;
        assert!(report(&d).contains("Boots: ota_0"));
        let mut d = crosspoint_like();
        ota(&mut d, 0xE000, 1, 2);
        ota(&mut d, 0xF000, 2, 3);
        assert!(report(&d).contains("Boots: ota_0"));
    }

    #[test]
    fn blank_otadata_and_empty_slot() {
        let mut d = crosspoint_like();
        d[0x650000..0x650100].fill(0xFF);
        let r = report(&d);
        assert!(r.contains("otadata[0]: blank"));
        assert!(r.contains("App app1: empty"));
        assert!(r.contains("Boots: first app"));
    }

    #[test]
    fn hostile_and_short_dumps_do_not_panic() {
        assert!(report(&[]).contains("dump ends before"));
        let mut d = vec![0u8; 0x9000];
        assert!(report(&d).contains("unknown entry"));
        d[TABLE_OFFSET..TABLE_OFFSET + 32].copy_from_slice(&entry(
            0,
            0x10,
            0xFFFF_FF00,
            0xFFFF_FFFF,
            "huge",
        ));
        d[TABLE_OFFSET + 32] = 0xFF;
        d[TABLE_OFFSET + 33] = 0xFF;
        let r = report(&d);
        assert!(r.contains("runs past the end"));
        assert!(r.contains("App huge: empty"));
        let mut d = crosspoint_like();
        d.truncate(0x10020);
        let r = report(&d);
        assert!(r.contains("not 16 MiB"));
        assert!(r.contains("App app0: image header"));
    }

    #[test]
    fn encrypted_looking_table_is_reported() {
        let mut d = crosspoint_like();
        d[TABLE_OFFSET..TABLE_OFFSET + 64].copy_from_slice(&[0x5A; 64]);
        assert!(report(&d).contains("unknown entry 5a 5a"));
    }

    fn with_otadata(dump: &[u8], ota: &[u8]) -> Vec<u8> {
        let mut d = dump.to_vec();
        d[0xE000..0x10000].copy_from_slice(ota);
        d
    }

    #[test]
    fn otadata_select_switches_slot_and_keeps_active_entry() {
        let mut d = crosspoint_like();
        ota(&mut d, 0xE000, 7, 2);
        ota(&mut d, 0xF000, 6, 2);
        let new = otadata_select(&d, 1).unwrap();
        assert_eq!(new.len(), 0x2000);
        assert_eq!(new[..0x1000], d[0xE000..0xF000]);
        let r = report(&with_otadata(&d, &new));
        assert!(
            r.contains("otadata[1]: seq 8, state undefined, CRC ok"),
            "{r}"
        );
        assert!(r.contains("Boots: ota_1"), "{r}");
        // Selecting the slot that already boots still moves forward.
        let again = otadata_select(&d, 0).unwrap();
        assert!(report(&with_otadata(&d, &again)).contains("seq 9"));
        // The original otadata is the way back.
        assert!(report(&d).contains("Boots: ota_0"));
    }

    #[test]
    fn otadata_select_handles_blank_and_bad_entries() {
        let mut d = crosspoint_like();
        let new = otadata_select(&d, 1).unwrap();
        assert!(report(&with_otadata(&d, &new)).contains("Boots: ota_1 (seq 2"));
        ota(&mut d, 0xE000, 9, 2);
        d[0xE000 + 28] ^= 1; // bad CRC: ignored
        ota(&mut d, 0xF000, 3, 4); // aborted: ignored
        let new = otadata_select(&d, 1).unwrap();
        assert!(report(&with_otadata(&d, &new)).contains("Boots: ota_1 (seq 2"));
    }

    #[test]
    fn otadata_select_rejects_bad_input() {
        let d = crosspoint_like();
        assert!(otadata_select(&d, 2).is_err());
        assert!(otadata_select(&d[..0xE100], 1).is_err());
        assert!(otadata_select(&[0u8; 16], 1).is_err());
        let mut d2 = d.clone();
        ota(&mut d2, 0xE000, u32::MAX - 1, 2);
        assert!(otadata_select(&d2, 1).is_err());
    }
}
