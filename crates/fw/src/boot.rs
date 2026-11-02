//! Boot slot switching (ADR 017): "Back to CrossPoint" writes one otadata
//! entry so the bootloader starts CrossPoint's slot, then the caller restarts.
use embedded_storage::{ReadStorage, Storage};
use esp_bootloader_esp_idf::partitions::{
    self, AppPartitionSubType, DataPartitionSubType, PartitionType,
};
use esp_storage::FlashStorage;
use x3_core::otadata::{self, ENTRY_LEN, Entry, PARTITION_LEN, SECTOR_LEN};

/// CrossPoint's slot on the owner unit (HARDWARE.md, verified 2026-10-26).
pub const CROSSPOINT_SLOT: u32 = 0;
const IMAGE_MAGIC: u8 = 0xE9;
const MAX_SLOTS: usize = 16;
const OTA0_SUBTYPE: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No readable partition table (or its MD5 does not match).
    Table,
    /// No otadata partition of 0x2000 bytes, or fewer than two OTA slots.
    Layout,
    Flash,
    /// The target slot does not start with an app image header.
    NoImage,
    Select,
    /// The entry read back does not select the target slot.
    Verify,
}

pub struct Layout {
    otadata: u32,
    slots: u32,
    apps: [Option<u32>; MAX_SLOTS],
}

impl Layout {
    pub fn read(flash: &mut FlashStorage<'_>) -> Result<Self, Error> {
        let mut buf = [0u8; partitions::PARTITION_TABLE_MAX_LEN];
        let table = partitions::read_partition_table(flash, &mut buf).map_err(|_| Error::Table)?;
        let mut otadata = None;
        let mut slots = 0;
        let mut apps = [None; MAX_SLOTS];
        for p in table.iter() {
            match p.partition_type() {
                PartitionType::Data(DataPartitionSubType::Ota) => {
                    if p.len() as usize != PARTITION_LEN {
                        return Err(Error::Layout);
                    }
                    otadata = Some(p.offset());
                }
                PartitionType::App(s)
                    if s != AppPartitionSubType::Factory && s != AppPartitionSubType::Test =>
                {
                    let n = (s as u8).wrapping_sub(OTA0_SUBTYPE) as usize;
                    let slot = apps.get_mut(n).ok_or(Error::Layout)?;
                    *slot = Some(p.offset());
                    slots += 1;
                }
                _ => {}
            }
        }
        if slots < 2 {
            return Err(Error::Layout);
        }
        Ok(Self {
            otadata: otadata.ok_or(Error::Layout)?,
            slots,
            apps,
        })
    }

    pub fn entries(&self, flash: &mut FlashStorage<'_>) -> Result<[Option<Entry>; 2], Error> {
        let mut out = [None; 2];
        for (i, e) in out.iter_mut().enumerate() {
            let mut raw = [0u8; ENTRY_LEN];
            flash
                .read(self.otadata + (i * SECTOR_LEN) as u32, &mut raw)
                .map_err(|_| Error::Flash)?;
            *e = Entry::parse(&raw);
        }
        Ok(out)
    }

    pub fn booted_slot(&self, entries: &[Option<Entry>; 2]) -> Option<u32> {
        otadata::booted_slot(entries, self.slots)
    }

    /// Writes the otadata entry that boots `slot` and checks it by reading back.
    pub fn switch_to(&self, flash: &mut FlashStorage<'_>, slot: u32) -> Result<(), Error> {
        let app = self
            .apps
            .get(slot as usize)
            .copied()
            .flatten()
            .ok_or(Error::Select)?;
        let mut magic = [0u8; 4];
        flash.read(app, &mut magic).map_err(|_| Error::Flash)?;
        if magic[0] != IMAGE_MAGIC {
            return Err(Error::NoImage);
        }
        let entries = self.entries(flash)?;
        let w = otadata::select(&entries, self.slots, slot).map_err(|_| Error::Select)?;
        flash
            .write(self.otadata + (w.sector * SECTOR_LEN) as u32, &w.entry)
            .map_err(|_| Error::Flash)?;
        let after = self.entries(flash)?;
        if after[w.sector] != Entry::parse(&w.entry) || self.booted_slot(&after) != Some(slot) {
            return Err(Error::Verify);
        }
        Ok(())
    }
}
