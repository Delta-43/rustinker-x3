//! Host disk image only. This module never opens a physical device.
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp};
use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
pub struct Disk(RefCell<File>);
impl Disk {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if !path.exists() {
            let mut f = OpenOptions::new().create_new(true).write(true).open(path)?;
            f.write_all(&blank())?;
            f.sync_all()?;
        }
        if !path.metadata()?.is_file() {
            return Err(std::io::Error::other("Expected a regular disk-image file"));
        }
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        if file.metadata()?.len() % 512 != 0 {
            return Err(std::io::Error::other("Invalid image length"));
        }
        Ok(Self(RefCell::new(file)))
    }
}
impl BlockDevice for Disk {
    type Error = std::io::Error;
    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> std::io::Result<()> {
        let mut file = self.0.borrow_mut();
        file.seek(SeekFrom::Start(u64::from(start.0) * 512))?;
        for b in blocks {
            file.read_exact(&mut b.contents)?;
        }
        Ok(())
    }
    fn write(&self, blocks: &[Block], start: BlockIdx) -> std::io::Result<()> {
        let mut file = self.0.borrow_mut();
        file.seek(SeekFrom::Start(u64::from(start.0) * 512))?;
        for b in blocks {
            file.write_all(&b.contents)?;
        }
        file.sync_all()
    }
    fn num_blocks(&self) -> std::io::Result<BlockCount> {
        Ok(BlockCount(
            (self.0.borrow().metadata()?.len() / 512)
                .try_into()
                .map_err(|_| std::io::Error::other("Image too large"))?,
        ))
    }
}
pub struct Clock;
impl TimeSource for Clock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 21,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}
/// Original synthetic FAT16 image, shared in layout with the storage tests.
pub fn blank() -> Vec<u8> {
    let mut d = vec![0; 8193 * 512];
    d[446 + 4] = 0x06;
    d[454..458].copy_from_slice(&1u32.to_le_bytes());
    d[458..462].copy_from_slice(&8192u32.to_le_bytes());
    d[510..512].copy_from_slice(&[0x55, 0xaa]);
    let b = &mut d[512..1024];
    b[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    b[3..11].copy_from_slice(b"X3TEST  ");
    b[11..13].copy_from_slice(&512u16.to_le_bytes());
    b[13] = 1;
    b[14..16].copy_from_slice(&1u16.to_le_bytes());
    b[16] = 2;
    b[17..19].copy_from_slice(&512u16.to_le_bytes());
    b[19..21].copy_from_slice(&8192u16.to_le_bytes());
    b[21] = 0xf8;
    b[22..24].copy_from_slice(&32u16.to_le_bytes());
    b[24..26].copy_from_slice(&32u16.to_le_bytes());
    b[26..28].copy_from_slice(&64u16.to_le_bytes());
    b[28..32].copy_from_slice(&1u32.to_le_bytes());
    b[36] = 0x80;
    b[38] = 0x29;
    b[43..54].copy_from_slice(b"X3 TEST    ");
    b[54..62].copy_from_slice(b"FAT16   ");
    b[510..512].copy_from_slice(&[0x55, 0xaa]);
    for sector in [2, 34] {
        d[sector * 512..sector * 512 + 4].copy_from_slice(&[0xf8, 0xff, 0xff, 0xff]);
    }
    d
}
