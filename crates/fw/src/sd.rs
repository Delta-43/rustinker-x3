//! microSD on the shared SPI bus (CS GPIO12, MISO GPIO7; clock and MOSI are
//! the display's). Phase 2.4 bring-up: a read-only probe that initialises the
//! card, reports size and type, and lists the root directory.
use core::fmt::Write;
use embedded_hal::spi::{ErrorType, Operation, SpiBus, SpiDevice};
use embedded_sdmmc::{Error, Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::Output,
    spi::master::{Config, Spi},
    time::{Instant, Rate},
};

/// The SD card's chip select on a borrowed bus. Exists only while the
/// display is idle, so the two never interleave.
pub struct Bus<'a, 'd> {
    spi: &'a mut Spi<'d, Blocking>,
    cs: &'a mut Output<'d>,
}

impl ErrorType for Bus<'_, '_> {
    type Error = esp_hal::spi::Error;
}

impl SpiDevice for Bus<'_, '_> {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        self.cs.set_low();
        let mut r = Ok(());
        for op in ops {
            r = match op {
                Operation::Read(buf) => SpiBus::read(self.spi, buf),
                Operation::Write(buf) => SpiBus::write(self.spi, buf),
                Operation::Transfer(read, write) => SpiBus::transfer(self.spi, read, write),
                Operation::TransferInPlace(buf) => SpiBus::transfer_in_place(self.spi, buf),
                Operation::DelayNs(ns) => {
                    Delay::new().delay_nanos(*ns);
                    Ok(())
                }
            };
            if r.is_err() {
                break;
            }
        }
        let f = SpiBus::flush(self.spi);
        self.cs.set_high();
        r.and(f)
    }
}

/// No RTC driver yet (Phase 2.5): a fixed date for any FAT timestamps.
pub struct Clock;

impl TimeSource for Clock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 26,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

const INIT_HZ: u32 = 400_000;
const RUN_HZ: u32 = 16_000_000;

fn set_rate(spi: &mut Spi<'_, Blocking>, hz: u32) -> bool {
    spi.apply_config(&Config::default().with_frequency(Rate::from_hz(hz)))
        .is_ok()
}

/// Initialise the card, print its size and type and list the root of the
/// first FAT volume. Reads only. Leaves the bus at the display's clock.
pub fn probe<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    out: &mut impl Write,
) -> Result<(), &'static str> {
    if !set_rate(spi, INIT_HZ) {
        return Err("SPI clock");
    }
    // At least 74 clocks with CS high so the card enters SPI mode.
    cs.set_high();
    let _ = SpiBus::write(spi, &[0xFF; 10]);
    let r = probe_card(
        Bus {
            spi: &mut *spi,
            cs: &mut *cs,
        },
        out,
    );
    // The display needs its clock back whatever happened.
    if !set_rate(spi, RUN_HZ) {
        return Err("SPI clock restore");
    }
    r
}

fn probe_card(bus: Bus<'_, '_>, out: &mut impl Write) -> Result<(), &'static str> {
    let card = SdCard::new(bus, Delay::new());
    let bytes = card.num_bytes().map_err(|_| "card did not initialise")?;
    let _ = writeln!(
        out,
        "sd: card {:?}, {} bytes ({} MiB)",
        card.get_card_type(),
        bytes,
        bytes / (1024 * 1024)
    );
    card.spi(|bus| {
        let _ = set_rate(bus.spi, RUN_HZ);
    });
    let volumes: VolumeManager<_, _, 2, 2, 1> = VolumeManager::new_with_limits(card, Clock, 0);
    let volume = volumes
        .open_raw_volume(VolumeIdx(0))
        .map_err(|_| "no FAT volume 0")?;
    let root = volumes
        .open_root_dir(volume)
        .map_err(|_| "cannot open root")?;
    let mut entries = 0u32;
    let listed = volumes.iterate_dir(root, |e| {
        entries += 1;
        let _ = writeln!(
            out,
            "sd:   {}{} {} bytes",
            e.name,
            if e.attributes.is_directory() { "/" } else { "" },
            e.size
        );
        core::ops::ControlFlow::Continue(())
    });
    let _ = volumes.close_dir(root);
    let _ = volumes.close_volume(volume);
    listed.map_err(|_| "cannot list root")?;
    let _ = writeln!(out, "sd: {entries} root entries");
    Ok(())
}

/// Largest write test, in MiB.
pub const TEST_MAX_MIB: u32 = 100;
const CHUNK: usize = 4096;

/// Deterministic test data: xorshift32 from a fixed seed, so the read pass
/// can regenerate and compare every byte without holding the file.
struct Stream(u32);

impl Stream {
    fn fill(&mut self, buf: &mut [u8]) {
        for word in buf.chunks_mut(4) {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.0 = x;
            word.copy_from_slice(&x.to_le_bytes()[..word.len()]);
        }
    }
}

/// Write `mib` MiB to /X3TEST/TEST.BIN, read it back, compare every byte,
/// then delete the file and the folder. Touches nothing else on the card.
pub fn write_test<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    mib: u32,
    out: &mut impl Write,
) -> Result<(), &'static str> {
    if !(1..=TEST_MAX_MIB).contains(&mib) {
        return Err("size must be 1 to 100 MiB");
    }
    if !set_rate(spi, INIT_HZ) {
        return Err("SPI clock");
    }
    cs.set_high();
    let _ = SpiBus::write(spi, &[0xFF; 10]);
    let r = test_card(
        Bus {
            spi: &mut *spi,
            cs: &mut *cs,
        },
        mib,
        out,
    );
    if !set_rate(spi, RUN_HZ) {
        return Err("SPI clock restore");
    }
    r
}

fn test_card(bus: Bus<'_, '_>, mib: u32, out: &mut impl Write) -> Result<(), &'static str> {
    let card = SdCard::new(bus, Delay::new());
    card.num_bytes().map_err(|_| "card did not initialise")?;
    card.spi(|bus| {
        let _ = set_rate(bus.spi, RUN_HZ);
    });
    let volumes: VolumeManager<_, _, 2, 1, 1> = VolumeManager::new_with_limits(card, Clock, 0);
    let volume = volumes
        .open_raw_volume(VolumeIdx(0))
        .map_err(|_| "no FAT volume 0")?;
    let root = volumes
        .open_root_dir(volume)
        .map_err(|_| "cannot open root")?;
    let r = match volumes.make_dir_in_dir(root, "X3TEST") {
        Ok(()) | Err(Error::DirAlreadyExists) => match volumes.open_dir(root, "X3TEST") {
            Ok(dir) => {
                let r = test_file(&volumes, dir, mib, out);
                let _ = volumes.close_dir(dir);
                r
            }
            Err(_) => Err("cannot open X3TEST"),
        },
        Err(_) => Err("cannot create X3TEST"),
    };
    let removed = volumes.delete_entry_in_dir(root, "X3TEST");
    let _ = writeln!(out, "sd: remove X3TEST/: {removed:?}");
    let _ = volumes.close_dir(root);
    let _ = volumes.close_volume(volume);
    r
}

fn test_file<D: embedded_sdmmc::BlockDevice>(
    volumes: &VolumeManager<D, Clock, 2, 1, 1>,
    dir: embedded_sdmmc::RawDirectory,
    mib: u32,
    out: &mut impl Write,
) -> Result<(), &'static str> {
    const SEED: u32 = 0x5EED_1234;
    let total = mib as usize * 1024 * 1024;
    let mut buf = [0u8; CHUNK];

    let file = volumes
        .open_file_in_dir(dir, "TEST.BIN", Mode::ReadWriteCreateOrTruncate)
        .map_err(|_| "cannot create TEST.BIN")?;
    let start = Instant::now();
    let mut stream = Stream(SEED);
    let mut written = 0;
    let mut w = Ok(());
    while written < total {
        stream.fill(&mut buf);
        if volumes.write(file, &buf).is_err() {
            w = Err("write failed");
            break;
        }
        written += CHUNK;
        if written % (10 * 1024 * 1024) == 0 {
            let _ = writeln!(out, "sd:   written {} MiB", written / (1024 * 1024));
        }
    }
    let flushed = volumes.flush_file(file);
    let _ = volumes.close_file(file);
    let write_ms = start.elapsed().as_millis().max(1);
    w?;
    flushed.map_err(|_| "flush failed")?;
    let _ = writeln!(
        out,
        "sd: wrote {mib} MiB in {write_ms} ms ({} KiB/s)",
        written as u64 * 1000 / 1024 / write_ms
    );

    let file = volumes
        .open_file_in_dir(dir, "TEST.BIN", Mode::ReadOnly)
        .map_err(|_| "cannot reopen TEST.BIN")?;
    let length = volumes.file_length(file).unwrap_or(0) as usize;
    let start = Instant::now();
    let mut stream = Stream(SEED);
    let mut expect = [0u8; CHUNK];
    let mut read = 0;
    let mut bad: u64 = 0;
    let mut r = Ok(());
    while read < total {
        let n = match volumes.read(file, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => {
                r = Err("read failed");
                break;
            }
        };
        stream.fill(&mut expect[..n]);
        bad += buf[..n]
            .iter()
            .zip(&expect[..n])
            .filter(|(a, b)| a != b)
            .count() as u64;
        read += n;
    }
    let _ = volumes.close_file(file);
    let read_ms = start.elapsed().as_millis().max(1);
    r?;
    let _ = writeln!(
        out,
        "sd: read {} MiB in {read_ms} ms ({} KiB/s); length {length}, mismatched bytes {bad}",
        read / (1024 * 1024),
        read as u64 * 1000 / 1024 / read_ms
    );
    let deleted = volumes.delete_entry_in_dir(dir, "TEST.BIN");
    let _ = writeln!(out, "sd: delete TEST.BIN: {deleted:?}");
    if length != total || read != total || bad != 0 {
        return Err("verify FAILED");
    }
    let _ = writeln!(out, "sd: verify ok");
    Ok(())
}

/// The card behind the shared bus, as `x3_core::storage` sees it.
pub type Card<'a, 'b> = SdCard<Bus<'a, 'b>, Delay>;
pub type Data<'v, 'a, 'b> = x3_core::fat::FatStore<'v, Card<'a, 'b>, Clock>;

/// Opens `/X3/DATA` (the article and token store, PROTOCOL.md A.6) and runs
/// `f` on it. Five acquire retries, so a missing card fails in seconds
/// rather than the default ~70 s. Leaves the bus at the display's clock.
pub fn with_data<'d, R>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    f: impl FnOnce(&mut Data<'_, '_, 'd>) -> Result<R, &'static str>,
) -> Result<R, &'static str> {
    // Nothing in here awaits, so the future is ready on its first poll.
    embassy_futures::block_on(with_data_async(spi, cs, async |s| f(s)))
}

/// `with_data` for work that awaits, such as a Receive session. The card
/// stays open across the awaits; nothing else may use the bus meanwhile.
pub async fn with_data_async<'d, R>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    f: impl AsyncFnOnce(&mut Data<'_, '_, 'd>) -> Result<R, &'static str>,
) -> Result<R, &'static str> {
    if !set_rate(spi, INIT_HZ) {
        return Err("SPI clock");
    }
    cs.set_high();
    let _ = SpiBus::write(spi, &[0xFF; 10]);
    let r = async {
        let card = SdCard::new_with_options(
            Bus {
                spi: &mut *spi,
                cs: &mut *cs,
            },
            Delay::new(),
            embedded_sdmmc::sdcard::spi::AcquireOpts {
                use_crc: true,
                acquire_retries: 5,
            },
        );
        card.num_bytes().map_err(|_| "no SD card")?;
        card.spi(|bus| {
            let _ = set_rate(bus.spi, RUN_HZ);
        });
        let volumes = VolumeManager::new(card, Clock);
        let volume = volumes
            .open_raw_volume(VolumeIdx(0))
            .map_err(|_| "no FAT volume")?;
        let root = volumes.open_root_dir(volume).map_err(|_| "no root")?;
        let r = match volumes.open_dir(root, "X3") {
            Ok(x3) => {
                let r = match volumes.open_dir(x3, "DATA") {
                    Ok(data) => {
                        let mut store = x3_core::fat::FatStore {
                            manager: &volumes,
                            directory: data,
                        };
                        let r = f(&mut store).await;
                        let _ = volumes.close_dir(data);
                        r
                    }
                    Err(_) => Err("no /X3/DATA on the card"),
                };
                let _ = volumes.close_dir(x3);
                r
            }
            Err(_) => Err("no /X3 on the card"),
        };
        let _ = volumes.close_dir(root);
        let _ = volumes.close_volume(volume);
        r
    }
    .await;
    if !set_rate(spi, RUN_HZ) {
        return Err("SPI clock restore");
    }
    r
}

/// Is a card in the slot? Returns its size in bytes (a cheap way to notice
/// a swapped card) or `None`. One acquire attempt, so a missing card answers
/// in milliseconds. The X3 has no card-detect pin, so Home asks this.
pub fn present<'d>(spi: &mut Spi<'d, Blocking>, cs: &mut Output<'d>) -> Option<u64> {
    if !set_rate(spi, INIT_HZ) {
        return None;
    }
    cs.set_high();
    let _ = SpiBus::write(spi, &[0xFF; 10]);
    let size = SdCard::new_with_options(
        Bus {
            spi: &mut *spi,
            cs: &mut *cs,
        },
        Delay::new(),
        embedded_sdmmc::sdcard::spi::AcquireOpts {
            use_crc: true,
            acquire_retries: 1,
        },
    )
    .num_bytes()
    .ok();
    let _ = set_rate(spi, RUN_HZ);
    size
}

/// Opens `/X3` and runs `f` on it. Same retries and clock handling as
/// `with_data`.
fn with_x3<'d, R>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    f: impl FnOnce(
        &VolumeManager<Card<'_, 'd>, Clock, 2, 1, 1>,
        embedded_sdmmc::RawDirectory,
    ) -> Result<R, &'static str>,
) -> Result<R, &'static str> {
    if !set_rate(spi, INIT_HZ) {
        return Err("SPI clock");
    }
    cs.set_high();
    let _ = SpiBus::write(spi, &[0xFF; 10]);
    let r = (|| {
        let card = SdCard::new_with_options(
            Bus {
                spi: &mut *spi,
                cs: &mut *cs,
            },
            Delay::new(),
            embedded_sdmmc::sdcard::spi::AcquireOpts {
                use_crc: true,
                acquire_retries: 5,
            },
        );
        card.num_bytes().map_err(|_| "no SD card")?;
        card.spi(|bus| {
            let _ = set_rate(bus.spi, RUN_HZ);
        });
        let volumes: VolumeManager<_, _, 2, 1, 1> = VolumeManager::new_with_limits(card, Clock, 0);
        let volume = volumes
            .open_raw_volume(VolumeIdx(0))
            .map_err(|_| "no FAT volume")?;
        let root = volumes.open_root_dir(volume).map_err(|_| "no root")?;
        let r = match volumes.open_dir(root, "X3") {
            Ok(x3) => {
                let r = f(&volumes, x3);
                let _ = volumes.close_dir(x3);
                r
            }
            Err(_) => Err("no /X3 on the card"),
        };
        let _ = volumes.close_dir(root);
        let _ = volumes.close_volume(volume);
        r
    })();
    if !set_rate(spi, RUN_HZ) {
        return Err("SPI clock restore");
    }
    r
}

/// Reads a small file from `/X3` (configuration, PROTOCOL.md A.6) into `out`.
/// Fails if it is larger than `out`.
pub fn read_x3_file<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    name: &str,
    out: &mut [u8],
) -> Result<usize, &'static str> {
    with_x3(spi, cs, |volumes, x3| {
        let f = volumes
            .open_file_in_dir(x3, name, Mode::ReadOnly)
            .map_err(|_| "file not found in /X3")?;
        let len = volumes.file_length(f).unwrap_or(u32::MAX) as usize;
        let r = if len > out.len() {
            Err("file too large")
        } else {
            let mut done = 0;
            let mut r = Ok(len);
            while done < len {
                match volumes.read(f, &mut out[done..len]) {
                    Ok(0) => {
                        r = Err("short read");
                        break;
                    }
                    Ok(n) => done += n,
                    Err(_) => {
                        r = Err("read failed");
                        break;
                    }
                }
            }
            r
        };
        let _ = volumes.close_file(f);
        r
    })
}

/// Replaces a small file in `/X3` with `bytes` (empty clears it). Not
/// atomic: callers keep content that is harmless if cut short (a reader
/// rejects a partial line).
pub fn write_x3_file<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    name: &str,
    bytes: &[u8],
) -> Result<(), &'static str> {
    with_x3(spi, cs, |volumes, x3| {
        let f = volumes
            .open_file_in_dir(x3, name, Mode::ReadWriteCreateOrTruncate)
            .map_err(|_| "cannot open the file")?;
        let r = volumes.write(f, bytes).map_err(|_| "write failed");
        let c = volumes.close_file(f).map_err(|_| "close failed");
        r.and(c)
    })
}

/// Deletes a file in `/X3`.
pub fn delete_x3_file<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    name: &str,
) -> Result<(), &'static str> {
    with_x3(spi, cs, |volumes, x3| {
        volumes
            .delete_entry_in_dir(x3, name)
            .map_err(|_| "cannot delete the file")
    })
}

/// Appends `bytes` to a file in `/X3`, creating it. A file already over
/// `cap` bytes is started again, so a log never fills the card.
pub fn append_x3_file<'d>(
    spi: &mut Spi<'d, Blocking>,
    cs: &mut Output<'d>,
    name: &str,
    bytes: &[u8],
    cap: usize,
) -> Result<(), &'static str> {
    with_x3(spi, cs, |volumes, x3| {
        let full = volumes
            .find_directory_entry(x3, name)
            .is_ok_and(|e| e.size as usize > cap);
        let mode = if full {
            Mode::ReadWriteTruncate
        } else {
            Mode::ReadWriteCreateOrAppend
        };
        let f = volumes
            .open_file_in_dir(x3, name, mode)
            .map_err(|_| "cannot open the file")?;
        let r = volumes.write(f, bytes).map_err(|_| "write failed");
        let c = volumes.close_file(f).map_err(|_| "close failed");
        r.and(c)
    })
}
