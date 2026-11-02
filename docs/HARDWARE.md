# Hardware research — 2026-10-21

**Owner unit (2026-10-22, owner report):** works, charger available, runs
CrossPoint and joins the home Wi-Fi, so it accepts custom firmware. Its panel
controller is still unidentified; CrossPoint supports it, which narrows it to
CrossPoint's X3 paths (UC8253 / UC8279 variants). Read it before driver work
from CrossPoint's Settings → About screen, row "Display Controller". Current
CrossPoint fills that row from runtime detection: the OEM hw_calib/screenType
value first, then a two-pass display-bus probe (crosspoint-reader facaa87,
`lib/hal/HalGPIO.cpp` and `src/activities/settings/AboutActivity.cpp`; read
only, nothing copied). Older CrossPoint builds may lack the row; then use the
serial boot log or analyse the flash backup offline.

**No owner-device tests of this project's code have run.** “Verified (source)” below means the named
source explicitly contains the fact. It does not prove the owner's revision.
Confidence describes agreement between upstream implementations.

## Sources and licences

Pinned trees inspected through GitHub's API and raw source:

| Source | Revision | Root licence | Use |
| --- | --- | --- | --- |
| [MarigoldOS](https://github.com/chongfun/MarigoldOS/tree/4778b2e5b033b3b4054647a388e7a6ee202eca98) | 4778b2e | MIT | Rust HAL, display, gauge, partitions, workspace |
| [Rustmix X4](https://github.com/aimindseye/rustmix-x4-firmware/tree/46a169e42234eeedf1736974b80f1d34bc63a6cd) | 46a169e | **Unverified: no root licence** | README and Cargo manifest only; X4, not an X3 pin authority |
| [TrustyReader](https://zff.dev/behemoth/TrustyReader) | unavailable | **Unverified** | Site and raw licence/manifest requests failed (403); no code used |
| [CrossPoint](https://github.com/crosspoint-reader/crosspoint-reader/tree/f8936ba67c7d201077ce6add5fd5355d3a273667) | f8936ba | MIT | HAL, partitions, PlatformIO, hardware guidance |
| [CrossPlant](https://github.com/0xknowles/crossplant/tree/98a528f8b4612a55ca715cca56fce894333668ed) | 98a528f | MIT | FreeInk board profile, X3 controller, LUT names, button ladder |
| [CrossInk](https://github.com/binhex/CrossInk/tree/25c237ce1094edb768b1e99ec5381bafccbf9cb0) | 25c237c | MIT | Tree, README and root licence; no independent device evidence |

Root licences do not cover every nested asset automatically. No upstream source
code or waveform arrays have been incorporated; see ADR 001 and THIRD_PARTY.md.

## Pin and bus map

References: [CrossPoint HalGPIO.h](https://github.com/crosspoint-reader/crosspoint-reader/blob/f8936ba67c7d201077ce6add5fd5355d3a273667/lib/hal/HalGPIO.h),
[CrossPlant BoardConfig.h](https://github.com/0xknowles/crossplant/blob/98a528f8b4612a55ca715cca56fce894333668ed/freeink-sdk/libs/hardware/BoardConfig/include/BoardConfig.h),
[MarigoldOS main.rs](https://github.com/chongfun/MarigoldOS/blob/4778b2e5b033b3b4054647a388e7a6ee202eca98/fw/src/main.rs).

| Function | Pin/value | Status, confidence |
| --- | --- | --- |
| Display SPI clock / MOSI / CS | GPIO8 / 10 / 21 | Verified (three sources), high |
| Display DC / reset / BUSY | GPIO4 / 5 / 6 | Verified (three sources), high; BUSY polarity controller-dependent |
| SD MISO / CS | GPIO7 / 12 | Verified (three sources), high |
| SD bus | SPI, shared clock/MOSI with display | Verified (MarigoldOS SPI2 setup and CrossPoint SPI.begin), high |
| I2C SDA / SCL | GPIO20 / 0, 400 kHz | Verified (three sources), high |
| Bottom button ladder | GPIO1 ADC: Back, Confirm, Left, Right | Verified (board/input sources), high |
| Side button ladder | GPIO2 ADC: Up, Down | Verified (board/input sources), high; physical orientation unverified |
| Power / wake | GPIO3, pulled-up digital input, low = pressed | Verified on owner unit: deep-sleep wake works (Phase 2.8) |
| BQ27220 | 7-bit 0x55; voltage 0x08, SOC 0x2C | Verified (two implementations), high |
| BQ current command | 0x0C Current and 0x14 AverageCurrent both valid, positive = charging | Verified on owner unit (Phase 2.6) |
| DS3231 RTC | 7-bit 0x68, BCD seconds at 0x00 | Verified (CrossPoint / BoardConfig), high |
| QMI8658 IMU | 0x6B, WHO_AM_I 0x05; needs soft reset after CrossPoint | Verified on owner unit (Phase 2.7) |
| Battery sensing | Gauge on X3; GPIO0 ADC divider is X4-specific | Verified (MarigoldOS / profile), high |
| USB presence | X4 GPIO20 logic cannot be reused on X3 I2C SDA | Verified (HalGPIO.cpp), high |

Buttons are resistor ladders, not an independently scanned digital matrix.
[InputManager.cpp](https://github.com/0xknowles/crossplant/blob/98a528f8b4612a55ca715cca56fce894333668ed/freeink-sdk/libs/hardware/InputManager/src/InputManager.cpp)
uses ladder boundaries 3100, 2090, 750 for the four bottom buttons and 1120 for
the two side buttons, below its no-button threshold. These are reference ADC
counts, **unverified on the owner's unit**; calibrate idle/presses and debounce.
Simultaneous keys on one ladder cannot be assumed distinguishable.

### Owner unit buttons, verified 2026-10-27 (Phase 2.3)

ADC1, 11 dB, **with esp-hal basic offset calibration** (`AdcCalBasic`).
Uncalibrated reads were ~1636 counts high, which pushed Back and Confirm past
full scale. Calibrated, idle 4095:

| Physical (portrait, buttons at bottom) | Pin | Reading | CrossPoint name | **Our function** (ADR 024) |
| --- | --- | --- | --- | --- |
| Left bar, left end | GPIO1 | 3518 | Back | **Left** |
| Left bar, right end | GPIO1 | 2685 | Confirm | **Right** |
| Right bar, left end | GPIO1 | 1486 | Left | **Back** |
| Right bar, right end | GPIO1 | 4 | Right | **Confirm** |
| Left side key (top or bottom) | GPIO2 | 2236 | Up | Up (drawn `<`) |
| Right side key (top or bottom) | GPIO2 | 3 | Down | Down (drawn `>`) |
| Power | GPIO3, pull-up, low = pressed | — | Power | Power |
| Top edge, left (opposite Power) | chip enable (reset) | — | — | **Reset only** (verified 2026-11-02, below) |

Bottom button centres on screen, measured on the owner's photo (2026-10-31),
in the 528 × 792 portrait frame: x = 88, 217 | 308, 436 (two pill-shaped bars,
a button at each end). Each side key is a raised bar about 160 px tall on
screen scale, centred near y = 199. Functions verified on the unit with
`x3cmd keys`: left to right Left, Right, Back, Confirm; left side key Up,
right side key Down.

Each side key is one switch: top and bottom read the same. Readings are
within 10 counts of CrossPlant's recorded values; decoding uses its midpoints
(`x3-core::buttons`).

### Top-edge reset key, verified 2026-11-02

A small key on the top edge, opposite Power, is a **hard reset** (chip enable),
not an input. Three presses gave three USB disconnects and reconnects within
3 s; after them the chip reported reset reason `ChipPowerOn`. GPIO1, GPIO2,
GPIO3 and GPIO9 (pulled up, probed with a temporary build) did not change.
Firmware cannot read it as a key. It restarts whichever app the bootloader
selects; the bootloader (CrossPoint's, not ours to change) has no hold-a-pin
app select (no such strings in the backup's first 32 KB).

### Owner unit SD card, verified 2026-10-27 (Phase 2.4)

SanDisk-class SDHC/SDXC, 31,268,536,320 bytes, MBR + FAT32, on the shared
SPI bus (CS GPIO12, MISO GPIO7), init at 400 kHz then 16 MHz, embedded-sdmmc
0.10. **GPIO13 has no observed effect**: the card initialises and lists with
GPIO13 floating, high and low (50 ms settle), so it is not the card's power
switch on this unit, contrary to the stock-firmware notes. Firmware leaves it
unconfigured.

- 1 MiB: write 228 KiB/s, read 504 KiB/s, 0 mismatched bytes.
- 100 MiB: write 181 KiB/s (565 s), read 495 KiB/s (207 s), 0 of
  104,857,600 bytes mismatched; file and folder deleted afterwards.
  Write slows as the file grows (cluster-chain walk per write); irrelevant
  for 50–105 KB payloads.
- Card removed: clean `card did not initialise` error, but after ~70 s
  (default 50 acquire retries); cut retries in the real firmware. Card
  reinserted: lists again with no restart.
- embedded-sdmmc has no rename: storage keeps the generation scheme
  (`x3-core::storage`) instead of temp-file-and-rename.

### Owner unit I²C and RTC, verified 2026-10-27 (Phase 2.5)

I²C at 100 kHz (SDA GPIO20, SCL GPIO0) acknowledges exactly 0x55 (BQ27220),
0x68 (DS3231) and 0x6B (QMI8658, primary address). **CrossPoint keeps the RTC
in UTC** (read 02:06:05 against server UTC 02:06:12; local time was 03:06 CET).
Set to server UTC, read back exact, oscillator-stop flag clear, battery
oscillator enabled. After a CrossPoint shutdown and more than a minute off
USB, the clock agreed with the server within about 1 s and OSF stayed clear.

### Owner unit battery gauge, verified 2026-10-27 (Phase 2.6)

BQ27220 at 0x55, on USB power: voltage 4348 mV, state of charge 97%,
remaining 620 mAh of full 642 mAh (design 642, health 100%), temperature
3028 (0.1 K) = 29.6 °C, status 0x4028. **Both current registers are valid:**
0x0C Current +160 mA and 0x14 AverageCurrent +165 mA, positive while
charging. This settles the CrossPoint/MarigoldOS disagreement; display 0x14.
Discharge sign not observed yet (unplugging also drops the serial link).

### Owner unit motion sensor, verified 2026-10-27 (Phase 2.7)

QMI8658 at 0x6B, WHO_AM_I 0x05. **Soft reset first** (0x60 ← 0xB0, 50 ms):
after CrossPoint the control registers accept writes but the data and the
sample timestamp stay frozen (likely its wake-on-motion mode). Then CTRL1
0x40 (auto-increment), CTRL2 0x26 (±8 g, 125 Hz, 4096 LSB/g), CTRL7 0x01.
Timestamp then rises 12–13 per 100 ms, noise 1–2 mg.

| Position | Gravity reads |
| --- | --- |
| Flat, screen up | z ≈ −990 mg |
| Portrait upright, buttons at the bottom | x ≈ −980 mg |
| Landscape upright, buttons on the right | y ≈ −931 mg |

### Deep sleep with USB connected, verified 2026-11-02

With a USB host attached, `sleep_deep` never completes: the port stays
enumerated, no serial output, and neither the power button nor the timer
wakes the chip until USB is unplugged (then Power wakes it at once). Seen on
31 Oct and 2 Nov. On battery deep sleep and power-button wake work. The
firmware therefore stands by on USB (panel controller asleep, Power polled
every 100 ms, software reset on a press) and deep-sleeps only on battery.

### Panel retention, verified 2026-11-02

Hours on USB with the charge pump left on under a static Home screen left a
retained ghost and fine speckle that single full refreshes, the half bank and
CDI changes could not clear. Six black/white full-refresh cycles cleared it
(owner photos). After that, images held clean with the pump off for over a
minute whether drawn by full or fast refresh. The pump now goes off after 3 s
idle on USB as on battery, and Device → Clean screen runs six cycles.

### Owner unit deep sleep, verified 2026-10-27 (Phase 2.8)

Panel controller to deep sleep (image kept), then ESP32-C3 deep sleep with
`RtcioWakeupSource` on GPIO3 low plus a 120 s timer. Power button wake
reports reset `CoreDeepSleep`, cause `Gpio`; untouched, the timer wakes it
with cause `Timer`. Waking restarts the chip into app1. USB disappears while
asleep. Sleep current not measured (no meter).

### Reader page-turn latency, measured 2026-10-27 (Phase 3)

Literata 24/28 px, built-in sample, owner unit. Fast turn: layout and render
20–34 ms, plane write 82–84 ms (16 MHz), refresh 307–308 ms, so **~411–426 ms**
from key to page (yardstick MarigoldOS ~424 ms). Previous-page search by
layout only: 4–8 ms. First page (full refresh with power-on): 1894 ms.
DTM1 staging (~84 ms) runs after the new page is visible.

### Owner unit Wi-Fi and RAM, measured 2026-10-27 (Phase 4 spike)

esp-radio 0.18 + embassy-net 0.9.1, `main` on the esp-rtos executor
(`#[esp_rtos::main]`; `block_on` from a blocking main stalled). Scan 175 ms
(home AP on 2.4 GHz ch 1, −62 dBm, WPA2). Join 1738 ms, DHCP address after
2262 ms, RSSI −61 dBm.

RAM: static stack 178,600 B after the 72 KB radio heap (was 311 KB without).
With Wi-Fi up the radio uses 47,140 B of heap, 26,588 B free. Stack never
touched since boot: 169,908 B (without a library session). The library
(80 KiB text + one 52 KB frame + list) needed about 140 KB, leaving ~38 KB;
since ADR 027 the text is a 48 KiB window, so about 32 KB more is left (unverified);
Receive and reading never run together.

### Stock-firmware analysis (secondary source, 2026-10-27)

[CrazyCoder's X3 GPIO notes](https://gist.github.com/CrazyCoder/1c5f846adee18e21f91e264601a6ddce),
from reverse engineering the stock firmware. No licence stated: facts are
cited, nothing is copied. Unverified unless marked.

- GPIO1 ladder, 4 buttons, stock bands 0–1194, 1194–2408, 2408–3260,
  3260–3999; above 3999 = no press. GPIO2 ladder, 2 buttons, 0–1944 and
  1945–2583. Stock firmware stores per-unit calibration in NVS namespace
  `eepuser` (±319 counts). Consistent with the owner unit once ADC offset
  calibration is on (Phase 2.3).
- GPIO13: SD card power control (output). **Not confirmed on the owner
  unit**: no observed effect (Phase 2.4).
- GPIO9 unused by the application; GPIO11 and 14–17 unavailable.
- Power GPIO3 active low, wake mask 0x08. On shutdown the stock firmware
  sets GPIO18/19 (USB) to open-drain low.
- I2C 400 kHz: QMI8658 0x6B (0x6A), DS3231 0x68, BQ27220 0x55 (device ID
  0x0220; temperature 0x06, voltage 0x08, current 0x0C signed, SOC 0x2C,
  SOH 0x2E, design capacity 0x3C; 642 mAh; charging when current > 0).
- It names the display SSD1677 at 10 MHz. **Not true for the owner unit**
  (UC8253 verified three ways); treat as batch-dependent.

## Display: initial assumption needs correction before bring-up

792×528 geometry is verified in current sources (high confidence). Controller
identity on the owner's unit is **unverified**. Initial project notes say SSD1677;
current CrossPlant's default X3 profile says UC8253, and current CrossPoint also
selects an X3 UC8279 variant. MarigoldOS separates SSD1677 and UC8253 paths and
mentions UC8279d probing. Do not send an SSD1677 init merely because it appears
in an older description. No driver selection is made in this session.

Source-only SSD1677 reference: [command sequence](https://github.com/chongfun/MarigoldOS/blob/4778b2e5b033b3b4054647a388e7a6ee202eca98/display/src/epd/ssd1677.rs).
It resets, waits BUSY, selects temperature source (0x18), configures booster
(0x0C), output (0x01), border (0x3C), entry mode (0x11), RAM ranges
(0x44/0x45), auto-clears planes (0x46/0x47), then update controls
(0x21/0x22). Frame planes use 0x24/0x26; activation 0x20. This path uses
OTP waveforms, not an X3-safe generic LUT. Confidence: high for this source's
behaviour, low for applicability to the owner's X3.

Current X3 reference: [Uc8253X3Driver.cpp](https://github.com/0xknowles/crossplant/blob/98a528f8b4612a55ca715cca56fce894333668ed/freeink-sdk/libs/display/FreeInkDisplay/src/driver/Uc8253X3Driver.cpp).
The init order is panel (0x00), resolution (0x61), gate/source start (0x65),
power-off sequence (0x03), power rails (0x01), VCOM (0x82), booster (0x06),
PLL (0x30), LV selection (0xE1), then clearing both planes (0x10/0x13,
terminated with 0x11). The resolution command uses 792×600 controller RAM;
the profile exposes 792×528 visible pixels. **Do not substitute visible height
in controller commands without validating the panel.** Power on is 0x04,
refresh 0x12, power off 0x02. The profile defaults to the driver's 16 MHz SPI.
Confidence: medium for applicability; no timing or ghosting test here.

[LUT source](https://github.com/0xknowles/crossplant/blob/98a528f8b4612a55ca715cca56fce894333668ed/freeink-sdk/libs/display/FreeInkDisplay/src/lut/Uc8253X3Luts.h):
normal, half, fast, full, greyscale (`gc`), and anti-alias prepass banks, each
with VCOM/WW/BW/WB/BB tables. The driver transfers 42 bytes from each 43-byte
array using commands 0x20–0x24. No waveform bytes copied. Valid controller,
board revision, temperature and full/partial synchronisation must be established
before reuse. Grey pixel storage alone does not demonstrate physical greyscale.

### Owner unit display, verified 2026-10-27 (Phase 2.2)

UC8253 with the ADR 019 init and waveforms, SPI2 16 MHz mode 0. Full refresh:
power on 126 ms, refresh 859 ms, settle pass 307–308 ms, plane write 122 ms
(solid) to 177 ms (generated pattern), 1.7–1.9 s end to end. White, black and
a 33 px checker render cleanly to all edges (owner).

**Orientation (owner photo of `display corners`).** Raw byte 0, bit 7 (MSB
first) is the top-left pixel with the device in portrait, buttons at the
bottom. Raw x (the 792-pixel rows) runs top to bottom; raw y (528 rows) runs
left to right. Portrait pixel (column px, row py) → raw (x = py, y = px).
No mirroring, no bit reversal at the raw level. 528 rows fill the glass; the
controller's 600-row resolution leaves nothing visible past row 527.

Fast (differential) refresh: 308 ms busy, 457 ms per step with both planes
written at 16 MHz from generated patterns. Eight steps rendered in place;
the corners frame was erased with no visible ghost; a slight ghost of the
previous block remains (owner photo). Expected for the fast bank: clear it
with a periodic full refresh (`Settings::full_refresh_pages`).

## Flash and boot expectations

**Owner unit, verified 2026-10-26** (`x3-tools flash-info` on the full flash
backup; SHA-256 in MEMORY.md). This is the table that counts.

| Partition | Type | Offset | Size |
| --- | --- | --- | --- |
| nvs | data nvs | 0x9000 | 0x5000 |
| otadata | data ota | 0xE000 | 0x2000 |
| app0 | app ota_0 | 0x10000 | 0x770000 (7.44 MiB) |
| app1 | app ota_1 | 0x780000 | 0x770000 (7.44 MiB) |
| spiffs | data spiffs | 0xEF0000 | 0x100000 |
| coredump | data coredump | 0xFF0000 | 0x10000 |

Bootloader image at 0x0 (chip id 5, ESP32-C3); partition table at 0x8000,
6 entries, MD5 entry present. Slot contents:

- app0: `crosspoint-reader 1.6.5` (IDF 5.5.5, built 27 Sep 2026). **Active.**
- app1: `arduino-lib-builder 8cabf2c` (IDF v5.5.2, built 11 Feb 2026). An older
  Arduino-based image, possibly the factory firmware; unverified which.
- otadata[0] seq 7 valid, otadata[1] seq 6 valid, CRCs ok; boots ota_0.

The source candidate below does **not** match the owner unit: app slots are
larger, app1 starts at 0x780000, and spiffs is 1 MiB at 0xEF0000. Keep it only
as history.

Source candidate (superseded): [CrossPoint partitions.csv](https://github.com/crosspoint-reader/crosspoint-reader/blob/f8936ba67c7d201077ce6add5fd5355d3a273667/partitions.csv)
and [MarigoldOS partitions.csv](https://github.com/chongfun/MarigoldOS/blob/4778b2e5b033b3b4054647a388e7a6ee202eca98/partitions.csv)
agree on this layout. MarigoldOS labels its table X4.

| Partition | Offset | Size |
| --- | --- | --- |
| nvs | 0x9000 | 0x5000 |
| otadata | 0xE000 | 0x2000 |
| app0 (ota_0) | 0x10000 | 0x640000 |
| app1 (ota_1) | 0x650000 | 0x640000 |
| spiffs | 0xC90000 | 0x360000 |
| coredump | 0xFF0000 | 0x10000 |

ESP32-C3 convention places the bootloader at 0 and partition table at 0x8000;
MarigoldOS expects the stock bootloader to honour otadata and boots an app at
0x10000. This is source evidence, not a safe flashing recipe. Read the owner's
complete flash first and inspect its table and security state. No table or
bootloader is created or changed here. The stub cannot boot a device.

Lock and restore context: [community FAQ](https://pocketink.io/firmware/faq),
[reported locked-unit rescue](https://juyue.tw/en/blog/xteink-x3-rescue).
These were provided in MEMORY.md; device lock state remains unverified. No
unlock, eFuse, programmer, erase, restore or flash operation is authorised here.
Use BACKUP.md for human-run read-only checks; stop on a read/security failure.

## Paper RAM budget (not a measurement)

Assume 380 KiB (389,120 bytes) provisionally. Actual free RAM is **unverified**.
All entries except framebuffer arithmetic are design allowances, not crate claims.

| Resident allocation | Bytes allowed |
| --- | ---: |
| Current 1-bit framebuffer | 52,272 |
| Previous 1-bit framebuffer if required | 52,272 |
| Radio firmware/driver, network stack and packets | 100,000 |
| Task stacks and executor | 24,576 |
| HTTP headers/body stream/TCP buffers | 16,384 |
| FAT sectors and metadata | 4,096 |
| Display DMA scratch | 8,192 |
| Text window, glyph/layout scratch | 8,192 |
| Small active cache index and settings | 4,096 |
| Subtotal | 270,080 |
| Remaining allowance | 119,040 |
| Required runtime headroom | at least 40,960 |

TLS: zero. No whole 98 KiB JSON request or decoded 64 KiB text buffer may coexist
with this budget; use bounded incremental decoding/SD staging in Phase 4.
A 500-entry host index uses roughly 40 KiB and must not live on a device stack.
Two-bit tokens stream from SD as planes; no 418 KiB greyscale buffer on device.
If a driver needs a third plane, radio memory exceeds the allowance or full
JSON deserialisation is chosen, recompute before coding. Measure linked .bss,
.data, stack peaks, minimum free heap and largest free block with Wi-Fi active.
This paper budget cannot satisfy the measured-RAM exit criterion.
