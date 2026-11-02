use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp};
use std::{cell::RefCell, rc::Rc};
#[derive(Debug, Clone, Copy)]
pub struct PowerCut;
impl std::fmt::Display for PowerCut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("injected power cut")
    }
}
impl std::error::Error for PowerCut {}
#[derive(Clone)]
pub struct Disk(pub Rc<RefCell<State>>);
pub struct State {
    pub bytes: Vec<u8>,
    pub writes: usize,
    pub cut: Option<usize>,
    pub prefix: usize,
    pub dead: bool,
}
impl Disk {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Rc::new(RefCell::new(State {
            bytes,
            writes: 0,
            cut: None,
            prefix: 0,
            dead: false,
        })))
    }
}
impl BlockDevice for Disk {
    type Error = PowerCut;
    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), PowerCut> {
        let d = self.0.borrow();
        if d.dead {
            return Err(PowerCut);
        }
        for (i, b) in blocks.iter_mut().enumerate() {
            let offset = (start.0 as usize + i) * 512;
            b.contents
                .copy_from_slice(d.bytes.get(offset..offset + 512).ok_or(PowerCut)?);
        }
        Ok(())
    }
    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), PowerCut> {
        let mut d = self.0.borrow_mut();
        if d.dead {
            return Err(PowerCut);
        }
        for (i, b) in blocks.iter().enumerate() {
            let offset = (start.0 as usize + i) * 512;
            let cut = d.cut == Some(d.writes);
            d.writes += 1;
            let n = if cut { d.prefix } else { 512 };
            d.bytes
                .get_mut(offset..offset + n)
                .ok_or(PowerCut)?
                .copy_from_slice(&b.contents[..n]);
            if cut {
                d.dead = true;
                return Err(PowerCut);
            }
        }
        Ok(())
    }
    fn num_blocks(&self) -> Result<BlockCount, PowerCut> {
        Ok(BlockCount((self.0.borrow().bytes.len() / 512) as u32))
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
/// Original synthetic FAT16 fixture: 4 MiB, MBR + one partition. No borrowed
/// disk image, private data, external formatter or hardware access.
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
