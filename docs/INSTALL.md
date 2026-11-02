# Install guide

How to put this firmware on an Xteink X3 next to CrossPoint, and how to go back.

**Tested on one unit only:** CrossPoint 1.6.5, ESP32-C3 revision 4, 16 MB flash,
UC8253 panel. Your unit may differ. Every step below says what to check and when to
stop. If a check fails, stop and ask. Do not try again blindly.

## What this does and does not do

- It writes **one app slot** (`app1`, at 0x780000) and the small **otadata**
  partition that says which slot boots.
- It does **not** touch the bootloader, the partition table, or the CrossPoint slot
  (`app0`). CrossPoint stays where it is.
- It overwrites whatever is in `app1` now. That is why you take a full backup first.
- It does not unlock anything. Some units are locked against custom firmware and
  cannot take this. See [Locked units](#locked-units).

You can go back to CrossPoint at any time ([Go back](#go-back-to-crosspoint)).

## What you need

- An X3 that runs CrossPoint and has a **data-capable** USB cable. The cable that
  came with some units only charges. If no serial port appears when you plug in, try
  another cable.
- A computer with Python 3.10 or newer and `esptool` v5 (`pip install esptool`).
- Rust (`rustup`; the repository pins the toolchain): to build the firmware, or with a
  downloaded image only for the boot selector in step 4.
- A microSD card formatted FAT32.
- The device charged.

Close any serial monitor before you start. Keep the X3 awake on a CrossPoint
screen while you connect: in deep sleep its USB port disappears.

## 1. Back up the whole flash

Follow [BACKUP.md](BACKUP.md). It reads the flash and writes nothing. Do the full
16 MiB read twice and compare the two SHA-256 hashes. Keep the file somewhere safe.
**Never publish it:** it can hold your Wi-Fi details.

This backup is your way back. Without it, do not continue.

## 2. Check that your layout matches

The firmware is built for this layout:

| Partition | Offset | Size |
| --- | --- | --- |
| otadata | 0xE000 | 8 KiB |
| app0 (CrossPoint) | 0x10000 | 0x770000 |
| app1 (this firmware) | 0x780000 | 0x770000 |

Check your backup:

```text
cargo run -p x3-tools -- flash-info backup.bin
```

**Stop** unless `app1` starts at 0x780000 and is 0x770000 bytes. A different table
needs a different image and a decision recorded in [DECISIONS.md](DECISIONS.md).
The built image must fit: it is about 0.9 MB, far under 0x770000.

## 3. Build the image

Or skip this step: download `rustinker-x3-vX.Y.Z-app1.bin` and `SHA256SUMS` from the
project's GitHub releases, check the hash (`sha256sum -c SHA256SUMS`), and use the file
as `fw-app1.bin`. `NOTICES.txt` on the release lists the licences of what is inside it.

```text
cargo build -p fw --release --target riscv32imc-unknown-none-elf
esptool --chip esp32c3 elf2image --flash-mode dio --flash-size 16MB -o fw-app1.bin target/riscv32imc-unknown-none-elf/release/fw
```

## 4. Make the boot selector files

The otadata partition holds two entries. These commands build a copy that selects
the slot you choose, from **your own** backup, so the rest of your flash state is
kept. Slot 0 is `app0` (CrossPoint) and slot 1 is `app1` (this firmware).

```text
cargo run -p x3-tools -- otadata-select backup.bin 1 otadata-app1.bin
cargo run -p x3-tools -- otadata-select backup.bin 0 otadata-crosspoint.bin
```

Keep `otadata-crosspoint.bin`. It is the quickest way back.

## 5. Prepare the SD card

On the card, create a folder `X3` with a folder `DATA` inside it, and two small
text files in `X3`. Samples are in [config/](../config).

`X3/WIFI.CFG` is optional. You can add networks later from your phone (Device, then
Left: [USER_GUIDE.md](USER_GUIDE.md)). To start with one, or to add up to four, repeat
the pair of lines:

```text
ssid=YourNetwork
psk=YourPassword
ssid=SecondNetwork
psk=SecondPassword
```

`X3/SERVER.CFG` is only needed for Receive, and only once you run the companion
([COMPANION_SETUP.md](COMPANION_SETUP.md)):

```text
url=http://192.168.1.20:8788
token=<43 characters from: x3-companion device add NAME>
```

To receive away from home as well, add the internet lines described in
[COMPANION_SETUP.md](COMPANION_SETUP.md) ("Reaching the companion from any network").

The reader works without either file. Without them, Receive says what is missing.
Keep the card's files private: they hold your Wi-Fi password and the device token.

## 6. Flash

Put the SD card in the X3, connect it, wake it, and find its port (`COMx` on
Windows, `/dev/ttyACM0` on Linux). Then, **one command at a time**:

```text
esptool --port PORT write-flash 0x780000 fw-app1.bin
esptool --port PORT write-flash 0xe000 otadata-app1.bin
```

The first writes the firmware. Each command ends with `Hash of data verified.` The
second command switches the boot slot. **If either command reports an error, stop.**
Some units are locked (below), and a failed flash must not be retried blindly. Write
down the exact error text.

The device restarts. After a few seconds you should see the Home screen: a
"Continue reading" card and four tiles.

## Go back to CrossPoint

Any of these:

1. **On the device:** Home, then Device. Press Confirm, read the question, and press
   Confirm again to switch. Back cancels. The firmware writes the boot selector
   itself, checks it, and restarts into CrossPoint. To come back to this firmware
   you then need a computer: write `otadata-app1.bin` (step 6).
2. **From a computer:** `esptool --port PORT write-flash 0xe000 otadata-crosspoint.bin`.
3. **Full restore:** write your backup back (see [BACKUP.md](BACKUP.md)). This is the
   last resort. It needs the same data-capable cable.

Do not update CrossPoint while this firmware is installed in `app1`: CrossPoint's own
updater could overwrite the wrong slot.

## Update this firmware

Repeat steps 3 and 6 (build, then write `app1` and `otadata-app1.bin`). Your SD card
and your articles stay as they are.

## Locked units

Some units reject custom firmware. Xteink has said units from some Chinese domestic
retailers can be locked, and some units have an eFuse lock on USB download. A locked
unit fails the write, or the port never appears. This project does not unlock
anything and does not give advice on doing so. Community notes:
[pocketink.io firmware FAQ](https://pocketink.io/firmware/faq).

## If it does not boot

- No Home screen after a flash: write `otadata-crosspoint.bin` (step 2 of "Go back").
- No serial port, or the write fails at once: wrong cable, or a locked unit.
- Blank or garbled screen: the panel controller may not be a UC8253. This firmware
  only drives that one. Go back to CrossPoint and open an issue with your
  CrossPoint "About" screen.
