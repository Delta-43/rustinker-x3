# Hardware

What RustInker-X3 knows about the Xteink X3. **Measured** means checked on the test unit
(ESP32-C3 revision 4, 16 MB flash, UC8253 panel). **Source** means stated by the named
upstream project and used as given. Other units may differ.

## Overview

| Part | What | Status |
| --- | --- | --- |
| MCU | ESP32-C3, single-core RISC-V, 160 MHz, ~400 KB SRAM, no PSRAM | measured |
| Flash | external 16 MB SPI NOR | measured |
| Display | 3.7 in e-ink, 792 × 528, UC8253 controller | measured |
| Storage | microSD (FAT32) on the display's SPI bus | measured |
| Battery gauge | TI BQ27220 at I²C 0x55, 650 mAh cell | measured |
| Clock | DS3231 RTC at I²C 0x68 | measured |
| Motion | QMI8658 IMU at I²C 0x6B (unused by RustInker-X3) | measured |
| Keys | 4 bottom, 2 side, Power, and a hardware Reset | measured |
| Radio | 2.4 GHz Wi-Fi (BLE unused) | measured |
| NFC | a passive tag, **not connected to the MCU** | source |

## Pins

| Function | Pin | Status |
| --- | --- | --- |
| Display SPI clock / MOSI / CS | GPIO8 / 10 / 21 | measured |
| Display DC / reset / BUSY | GPIO4 / 5 / 6 | measured |
| SD card MISO / CS (clock, MOSI shared with the display) | GPIO7 / 12 | measured |
| I²C SDA / SCL | GPIO20 / 0 | measured |
| Bottom key ladder | GPIO1, ADC | measured |
| Side key ladder | GPIO2, ADC | measured |
| Power key | GPIO3, pull-up, low = pressed; deep-sleep wake source | measured |
| Reset key | chip enable (EN) | measured |
| GPIO13 | no observed effect on the SD card | measured |

## Keys

The bottom and side keys are resistor ladders, read with ADC1 at 11 dB attenuation **with
esp-hal's basic offset calibration** (without it the readings carry a large offset).

| Physical key (upright, keys at the bottom) | Pin | Reading | RustInker-X3 function |
| --- | --- | --- | --- |
| Left bar, left end | GPIO1 | 3518 | Left |
| Left bar, right end | GPIO1 | 2685 | Right |
| Right bar, left end | GPIO1 | 1486 | Back |
| Right bar, right end | GPIO1 | 4 | Confirm |
| Left side key | GPIO2 | 2236 | Up |
| Right side key | GPIO2 | 3 | Down |
| no key | both | 4095 | |

Each side key is one switch: pressing its top or bottom reads the same. Two keys on one
ladder cannot be told apart. Decoding uses the midpoints between these readings
(`crates/core/src/buttons.rs`).

The key on the top edge, opposite Power, is a **hardware reset** on the chip's enable pin.
Each press restarts the chip (reset reason `ChipPowerOn`); firmware cannot read it as a key.

## Display

- UC8253, SPI mode 0 at 16 MHz. Init bytes and waveform banks: see
  [THIRD_PARTY.md](../THIRD_PARTY.md) for their source.
- **Orientation:** the controller's raw rows are portrait columns. Raw byte 0, bit 7 is
  the top-left pixel with the X3 upright. Portrait pixel (column x, row y) is raw row x,
  bit y. No mirroring.
- Full refresh about 1.7–1.9 s including power-on; fast (differential) refresh about
  0.31 s; a reader page turn about 0.41–0.43 s from key press to page.
- **Four greys** take three passes: a black-and-white base, a settle bank, then one
  plane for dark grey and one for light grey and a short nudge. A grey picture takes
  about 5 s.
- **Retention:** hours with the charge pump on under a still image leave a ghost that
  single refreshes do not clear; six black/white full refreshes do. RustInker-X3 turns
  the pump off after 3 s idle, and **Clean screen** runs the six cycles.

## SD card

Shared SPI bus, initialised at 400 kHz, then 16 MHz. Measured on a 32 GB card: write about
180–230 KiB/s, read about 500 KiB/s. The FAT driver has no rename, so storage uses
numbered generations instead of temp-file-and-rename (see [Protocol](PROTOCOL.md#files-on-the-card)).

## I²C devices

- **BQ27220** (0x55): voltage 0x08, state of charge 0x2C, current 0x0C and average current
  0x14 (both valid, positive while charging), design capacity 642 mAh.
- **DS3231** (0x68): BCD time at 0x00. RustInker-X3 keeps it in UTC and sets it from the
  companion when they differ by more than 60 s.
- **QMI8658** (0x6B, WHO_AM_I 0x05): other firmware can leave it in a mode where data
  stops updating; a soft reset (0x60 ← 0xB0) restores it. RustInker-X3 switches it off.

## Power

- Deep sleep with the Power key (GPIO3 low) as the wake source works on battery.
- **With a USB host attached, deep sleep never completes** and nothing wakes the chip
  until USB is unplugged. RustInker-X3 therefore stands by on USB and deep-sleeps only on
  battery.
- The USB serial port disappears while the chip deep-sleeps.

## Wi-Fi and memory

- Scan about 0.2 s, join and DHCP about 2–5 s on a home WPA2 network.
- The radio takes a 72 KB heap. With Wi-Fi up about 26 KB of that heap is left. The
  reader holds one 52 KB frame and a 48 KiB text window; Receive and reading never run at
  the same time. The serial console prints the untouched stack after each Receive and reading session.

## Flash layout

Measured on the test unit (`x3-tools flash-info` on a full backup):

| Partition | Type | Offset | Size |
| --- | --- | --- | --- |
| nvs | data nvs | 0x9000 | 0x5000 |
| otadata | data ota | 0xE000 | 0x2000 |
| app0 | app ota_0 | 0x10000 | 0x770000 |
| app1 | app ota_1 | 0x780000 | 0x770000 |
| spiffs | data spiffs | 0xEF0000 | 0x100000 |
| coredump | data coredump | 0xFF0000 | 0x10000 |

Bootloader at 0x0, partition table at 0x8000. The bootloader picks the slot from otadata
and has no "hold a key to choose" option. Secure Boot and Flash Encryption were disabled.

## Not known

- Whether other units use the UC8279 controller (not supported) or other flash chips.
- Sleep current (not measured) and a full week of battery life.
- Behaviour of SD cards when power is cut in the middle of a write.

## Sources

Hardware facts were cross-checked against these projects (all MIT unless noted); see
[THIRD_PARTY.md](../THIRD_PARTY.md):

- [MarigoldOS](https://github.com/chongfun/MarigoldOS): Rust X3 firmware.
- [CrossPoint Reader](https://github.com/crosspoint-reader/crosspoint-reader): C++ reader
  firmware with X3 support.
- [CrossPlant](https://github.com/0xknowles/crossplant): CrossPoint fork with the FreeInk
  board profile.
- [CrazyCoder's X3 GPIO notes](https://gist.github.com/CrazyCoder/1c5f846adee18e21f91e264601a6ddce)
  (no licence stated; facts cited only).
- [Locked-unit rescue report](https://juyue.tw/en/blog/xteink-x3-rescue).
