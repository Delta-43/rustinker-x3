//! Link real radio, network and executor symbols. This function is never called.
extern crate alloc;

use alloc::boxed::Box;
use core::hint::black_box;

pub fn check() -> ! {
    esp_alloc::heap_allocator!(size: 100 * 1024);
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let timers = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    let interrupts =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timers.timer0, interrupts.software_interrupt0);
    if let Ok((controller, interfaces)) = esp_radio::wifi::new(peripherals.WIFI, Default::default())
    {
        let resources = Box::leak(Box::new(embassy_net::StackResources::<3>::new()));
        let (stack, runner) = embassy_net::new(
            interfaces.station,
            embassy_net::Config::dhcpv4(Default::default()),
            resources,
            black_box(1),
        );
        black_box((controller, stack, runner));
    }
    let volume = embedded_sdmmc::VolumeManager::new(Blocks, Blocks);
    black_box(volume.open_raw_volume(embedded_sdmmc::VolumeIdx(0))).ok();
    let executor = Box::leak(Box::new(esp_rtos::embassy::Executor::new()));
    executor.run(|spawner: embassy_executor::Spawner| {
        black_box(spawner);
    })
}

// Synthetic block source: retain the FAT parser without selecting any X3 pins.
struct Blocks;
impl embedded_sdmmc::BlockDevice for Blocks {
    type Error = core::convert::Infallible;
    fn read(
        &self,
        blocks: &mut [embedded_sdmmc::Block],
        _: embedded_sdmmc::BlockIdx,
    ) -> Result<(), Self::Error> {
        for block in blocks {
            block.contents.fill(black_box(0));
        }
        Ok(())
    }
    fn write(
        &self,
        _: &[embedded_sdmmc::Block],
        _: embedded_sdmmc::BlockIdx,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
    fn num_blocks(&self) -> Result<embedded_sdmmc::BlockCount, Self::Error> {
        Ok(embedded_sdmmc::BlockCount(8192))
    }
}
impl embedded_sdmmc::TimeSource for Blocks {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 21,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}
