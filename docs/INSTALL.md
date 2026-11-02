# Install

How to put RustInker-X3 on an Xteink X3, next to the firmware the device runs now (called
the **previous firmware** below), and how to go back.

> [!WARNING]
> RustInker-X3 has been tested on one X3 only (ESP32-C3 revision 4, 16 MB flash, UC8253
> panel). Every step below says what to check and when to stop. If a check fails, stop.
> Do not retry blindly.

## What the install changes

- It writes **one app slot**, `app1` at 0x780000, and the small **otadata** partition
  that tells the bootloader which slot to start.
- It does **not** touch the bootloader, the partition table or `app0`. The previous
  firmware stays in `app0`, and you can switch back to it at any time.
- Whatever is in `app1` now is overwritten. That is why you back up the whole flash first.
- It does not unlock anything. Some units are locked against custom firmware
  ([Locked units](#locked-units)).

## What you need

- An X3 that accepts custom firmware.
- A **data-capable** USB cable for the X3. Some cables only charge: if no serial port
  appears when you plug in, try another one.
- A computer with Python 3.10 or newer and [esptool](https://github.com/espressif/esptool)
  v5: `pip install esptool`.
- [Rust](https://rustup.rs) to make the boot selector files (step 5), and to build the
  firmware yourself if you do not download it. The repository pins the toolchain.
- A microSD card, FAT32.
- The X3 charged.

Close any serial monitor. Keep the X3 awake while you work: in deep sleep its USB port
disappears. In the commands, `PORT` is the serial port: `/dev/ttyACM0` on Linux,
`/dev/cu.usbmodem…` on macOS, `COM3` or similar on Windows.

## 1. Check the chip

```bash
esptool --chip esp32c3 --port PORT flash-id
esptool --chip esp32c3 --port PORT get-security-info
```

| Check | Expected | Stop if |
| --- | --- | --- |
| `flash-id` | Detected flash size: 16MB | the size differs, or the command fails |
| Secure Boot | Disabled | enabled |
| Flash Encryption | Disabled | enabled |
| Security flags | 0x0 | anything else you cannot explain |

These commands only read. A failure here usually means a charge-only cable or a locked unit.

## 2. Back up the whole flash

Read the full 16 MiB twice and compare. Each read takes about a minute and a half.

```bash
esptool --chip esp32c3 --port PORT read-flash 0x0 0x1000000 x3-backup-a.bin
esptool --chip esp32c3 --port PORT read-flash 0x0 0x1000000 x3-backup-b.bin
sha256sum x3-backup-a.bin x3-backup-b.bin
```

On Windows use `Get-FileHash x3-backup-a.bin, x3-backup-b.bin`; on macOS
`shasum -a 256`. **The two hashes must match.** If they do not, read again; if they still
differ, stop.

Keep the backup safe and private: it holds your Wi-Fi details and the previous firmware's
settings. **Never publish it.** Without a good backup, do not continue.

## 3. Check the partition layout

```bash
cargo run -p x3-tools -- flash-info x3-backup-a.bin
```

It prints the partition table, the app in each slot and which slot boots. It prints
nothing from NVS. RustInker-X3 is built for this layout:

| Partition | Offset | Size |
| --- | --- | --- |
| otadata | 0xE000 | 0x2000 |
| app0 (previous firmware, booting) | 0x10000 | 0x770000 |
| app1 (RustInker-X3) | 0x780000 | 0x770000 |

**Stop** unless `app1` starts at 0x780000 with 0x770000 bytes and the previous firmware
boots from `app0`. A different table needs a different build.

## 4. Get the firmware image

**Download:** from the [latest release](https://github.com/Delta-43/rustinker-x3/releases/latest)
take `rustinker-x3-vX.Y.Z-app1.bin` and `SHA256SUMS`, and check them:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

`NOTICES.txt` on the release lists the licences of everything inside the image.

**Or build it:**

```bash
cargo build -p fw --release --target riscv32imc-unknown-none-elf
esptool --chip esp32c3 elf2image --flash-mode dio --flash-size 16MB \
  -o rustinker-x3-app1.bin target/riscv32imc-unknown-none-elf/release/fw
```

The image is about 1.1 MB, far below the slot size.

## 5. Make the boot selector files

The otadata partition says which slot boots. These commands make two copies from **your
own backup**, so the rest of its state is kept:

```bash
cargo run -p x3-tools -- otadata-select x3-backup-a.bin 1 otadata-app1.bin
cargo run -p x3-tools -- otadata-select x3-backup-a.bin 0 otadata-previous.bin
```

Keep `otadata-previous.bin`. It is the quickest way back.

## 6. Prepare the SD card

On the card make a folder `X3` with a folder `DATA` inside. Two optional files go in `X3`;
samples are in [`config/`](../config):

- `X3/WIFI.CFG`: up to four networks. You can also add them later from your phone
  ([User guide](USER_GUIDE.md#wi-fi-setup)).

  ```text
  ssid=YourNetwork
  psk=YourPassword
  ```

- `X3/SERVER.CFG`: only needed for Receive, once you run the companion
  ([Companion setup](COMPANION_SETUP.md)).

  ```text
  url=http://192.168.1.20:8788
  token=<the 43-character device token>
  ```

The reader works without either file. Both hold secrets: keep the card private.

## 7. Flash

Put the card in the X3, connect it, wake it. Then, **one command at a time**:

```bash
esptool --chip esp32c3 --port PORT write-flash 0x780000 rustinker-x3-vX.Y.Z-app1.bin
esptool --chip esp32c3 --port PORT write-flash 0xe000 otadata-app1.bin
```

Each ends with `Hash of data verified.` The first writes the firmware; the second switches
the boot slot. **If either reports an error, stop** and write down the exact message.

The X3 restarts and shows the Home screen: a "Continue reading" card and four tiles.

## Go back to the previous firmware

1. **On the device:** Home → Device → **Switch firmware**, then Confirm. The firmware
   writes the boot selector, checks it and restarts into `app0`. To return to
   RustInker-X3 later you need a computer: write `otadata-app1.bin` again (step 7).
2. **From a computer:** `esptool --chip esp32c3 --port PORT write-flash 0xe000 otadata-previous.bin`.
3. **Full restore:** `esptool --chip esp32c3 --port PORT write-flash 0x0 x3-backup-a.bin`.
   This puts back the flash exactly as it was at the backup, and loses anything written
   since. Use it as the last resort.

Do not update the previous firmware over the air while RustInker-X3 is installed: an
over-the-air update normally writes the other slot, which is `app1`.

## Update RustInker-X3

Write the new image to `app1` (step 7, first command only). The card, your articles and
your pictures stay as they are.

## Locked units

Some X3 units reject custom firmware. Xteink has said units sold through some Chinese
domestic retailers can be locked, and some units have an eFuse lock on USB download. On
a locked unit the write fails or the port never appears. This project does not unlock
anything. Community notes: [pocketink.io firmware FAQ](https://pocketink.io/firmware/faq).

## Troubleshooting

| You see | Try |
| --- | --- |
| No serial port when you plug in | another cable; wake the device first |
| A write fails at once | stop: the unit may be locked |
| No Home screen after flashing | write `otadata-previous.bin` to go back, then check step 3 |
| Blank or garbled screen | the panel may not be a UC8253, which is the only one supported. Go back and open an issue with the controller name your previous firmware reports |
