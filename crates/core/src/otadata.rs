//! ESP-IDF otadata: two 32-byte entries, one at the start of each 4 KiB
//! sector. The bootloader starts OTA slot `(seq - 1) % slots` of the bootable
//! entry with the highest sequence number, or the first app when none is
//! bootable. Shared by the firmware and the host tools.
use crate::Error;

pub const ENTRY_LEN: usize = 32;
pub const SECTOR_LEN: usize = 0x1000;
pub const PARTITION_LEN: usize = 2 * SECTOR_LEN;

pub const STATE_INVALID: u32 = 3;
pub const STATE_ABORTED: u32 = 4;
/// What ESP-IDF writes without rollback; bootable with or without it.
pub const STATE_UNDEFINED: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub seq: u32,
    pub state: u32,
    pub crc_ok: bool,
}

impl Entry {
    /// `None` for an erased entry.
    pub fn parse(e: &[u8; ENTRY_LEN]) -> Option<Self> {
        let word = |i: usize| u32::from_le_bytes([e[i], e[i + 1], e[i + 2], e[i + 3]]);
        let seq = word(0);
        if seq == u32::MAX {
            return None;
        }
        Some(Self {
            seq,
            state: word(24),
            crc_ok: word(28) == crc(&e[0..4]),
        })
    }

    pub fn bootable(&self) -> bool {
        self.crc_ok && self.state != STATE_INVALID && self.state != STATE_ABORTED
    }
}

/// The ROM's `esp_rom_crc32_le(UINT32_MAX, …)` that otadata uses: CRC-32
/// (reflected, 0xEDB88320) starting from a zero register, inverted at the end.
pub fn crc(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Index and entry of the bootable entry with the highest sequence number.
pub fn active(entries: &[Option<Entry>; 2]) -> Option<(usize, Entry)> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| e.map(|e| (i, e)))
        .filter(|(_, e)| e.bootable())
        .max_by_key(|(_, e)| e.seq)
}

/// The OTA slot the bootloader starts, or `None` for "first app".
pub fn booted_slot(entries: &[Option<Entry>; 2], slots: u32) -> Option<u32> {
    let (_, e) = active(entries)?;
    (slots > 0).then(|| (e.seq - 1) % slots)
}

/// One entry to write at the start of `sector`, leaving the other alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Write {
    pub sector: usize,
    pub entry: [u8; ENTRY_LEN],
}

/// The write that makes the bootloader start `slot`. The active entry is kept,
/// so the previous slot stays one write away; the other entry gets the next
/// sequence number that selects `slot`, with state "undefined".
pub fn select(entries: &[Option<Entry>; 2], slots: u32, slot: u32) -> Result<Write, Error> {
    if slot >= slots {
        return Err(Error::Invalid);
    }
    let (sector, mut seq) = match active(entries) {
        Some((i, e)) => (1 - i, e.seq.checked_add(1).ok_or(Error::TooLarge)?),
        None => (0, 1),
    };
    while (seq - 1) % slots != slot {
        seq = seq.checked_add(1).ok_or(Error::TooLarge)?;
    }
    if seq == u32::MAX {
        return Err(Error::TooLarge);
    }
    let mut entry = [0xFF; ENTRY_LEN];
    entry[0..4].copy_from_slice(&seq.to_le_bytes());
    entry[24..28].copy_from_slice(&STATE_UNDEFINED.to_le_bytes());
    entry[28..32].copy_from_slice(&crc(&seq.to_le_bytes()).to_le_bytes());
    Ok(Write { sector, entry })
}
