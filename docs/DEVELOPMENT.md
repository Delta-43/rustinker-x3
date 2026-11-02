# Development

How the code is laid out, how to build and test it, and the tools that help on the device.

## Layout

```text
crates/core      no_std logic shared by firmware, simulator and companion: screens, text
                 layout, payload codecs, storage generations, Receive session, Wi-Fi setup
crates/proto     no_std wire formats: HTTP framing, device text bodies, SD config files, signing
crates/fw        the ESP32-C3 binary: drivers (display, SD, keys, I²C, Wi-Fi, TLS) and glue
crates/sim       desktop simulator: draws every screen to PNG
companion/       x3-companion: axum server, SQLite inbox, conversion, web page
tools/           x3-tools CLI and Python helpers for the device
```

Hardware access stays in `crates/fw`. Everything in `crates/core` and `crates/proto` builds
and is tested on the host.

## Build and test

The toolchain is pinned in [`rust-toolchain.toml`](../rust-toolchain.toml) (Rust 1.99 with
the `riscv32imc-unknown-none-elf` target).

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude fw
cargo build -p fw --release --target riscv32imc-unknown-none-elf
cargo deny check
```

The host suite has 220 tests, including hostile-input tests for every parser, a sweep that
cuts power at every write of a Receive, and acceptance tests of the companion over HTTP.

## Simulator

```bash
cargo run -p sim -- gui samples/gui
```

Draws every device screen with sample data into [`samples/gui`](../samples/gui), using the
same drawing code as the firmware. A test fails if the checked-in pictures differ from the
code, so update them in the same commit as a screen change.

## Host tools

```text
cargo run -p x3-tools -- token INPUT.png OUT.tk1 contain|cover auto|landscape|portrait
cargo run -p x3-tools -- preview IN.tk1 OUT.png
cargo run -p x3-tools -- article INPUT.txt OUT.ar1 TITLE URL article|summary
cargo run -p x3-tools -- put-article CARD_DIR ITEM_ID FILE.ar1
cargo run -p x3-tools -- font FONT.ttf PX OUT.x3f
cargo run -p x3-tools -- flash-info BACKUP.bin
cargo run -p x3-tools -- otadata-select BACKUP.bin SLOT OUT.bin
```

`put-article` writes an article straight onto a mounted card, in the same format a Receive
uses.

## Serial console

The firmware listens on USB serial. [`tools/x3cmd.py`](../tools/x3cmd.py) sends one command
and prints the replies (needs `pyserial`):

```bash
python3 tools/x3cmd.py status
python3 tools/x3cmd.py receive 40
```

Useful commands: `status`, `receive`, `library`, `tokens`, `check` (verify every item on the
card), `log`, `battery`, `time`, `i2c scan`, `sd probe`, `buttons` and `keys` (raw and
decoded key presses), `display white|black|checker|corners`, `sleep`, `switch` (start the
other slot). `help` lists them all.

### Screen capture

```bash
python3 tools/capture.py screens/ 300
```

Turns on `capture on` and saves every frame the device draws as a PNG, grey pictures
included, while you use the X3. The screenshots in this repository were made this way.

### Device tests

- [`tools/soak.py`](../tools/soak.py) runs many Receives in a row and watches for failures
  and stack loss.
- [`tools/cutdrill.py`](../tools/cutdrill.py) with
  [`tools/drill-companion.sh`](../tools/drill-companion.sh) resets the X3 in the middle of
  downloads and checks that the card recovers.

## Releases

A release image is built from a tagged commit with `cargo build --release` and
`esptool elf2image` ([Install](INSTALL.md#4-get-the-firmware-image)).
[`tools/notices.py`](../tools/notices.py) writes `NOTICES.txt` with the licence of every
crate linked into the firmware, Espressif's radio libraries, the display constants and the
font; it fails if a crate has no licence text. Ship it next to the image.

## Rules of the code

- `#![no_std]` in `core`, `proto` and `fw`; no heap on hot paths.
- No `unwrap()` or `expect()` where external input is handled; return errors.
- Every external input has a size cap; a new parser needs malformed, truncated, oversized
  and hostile-input tests.
- Writes to the card are atomic (a new generation, then a record); a power cut never leaves
  something that looks valid but is not.
- No frame buffer (52 KB) inside an async function across an `await`; draw in a plain
  function.
- Hardware claims need output from a real device. Never commit a flash backup, Wi-Fi
  details or a device token.
- Docs and UI text: British English, plain words, short sentences.
