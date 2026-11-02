# Design decisions

The decisions that shape RustInker-X3, one entry per decision record (ADR). The code
refers to them by number. Each says what was chosen and why.

| ADR | Decision |
| --- | --- |
| 001 | **Fresh Rust implementation.** Not a fork of another X3 firmware: the delivery model differs, and restore safety matters more than sharing a user interface. Hardware facts from other projects are cited, not copied. |
| 002 | **Licence: MIT OR Apache-2.0** for original code. Dependencies keep their own licences, listed in THIRD_PARTY.md. No card art or data dumps are distributed. |
| 003 | **Device stack:** esp-hal, esp-radio, embassy-executor, embassy-net and embedded-sdmmc on `riscv32imc-unknown-none-elf`, `no_std`. A small bounded HTTP client of our own instead of a general one. |
| 004 | **Partition safety.** The bootloader and partition table are never changed. Only one app slot and the boot selector are written, after a full backup. |
| 005–007 | Early options for server access, delivery and a firmware base. Superseded by 008 and 009. |
| 008 | **The X3 pulls.** Pressing Receive joins Wi-Fi, lists pending jobs on the companion, streams each to the SD card, commits, acknowledges and turns Wi-Fi off. The device runs no server. The companion is one Rust service with SQLite. |
| 009 | **Protocol v2** (PROTOCOL.md): line-based device API, `tk1` and `ar1` payloads, ULID jobs, item IDs, SHA-256 identity, idempotent acks. Pinned device and companion crates. |
| 010 | **Device text lines are capped at 192 bytes** (the longest valid job line is 161). |
| 011 | **Immutable storage generations** on the card: every item is written as a new generation, checked by CRC and SHA-256, then made current. A power cut never leaves a half-written item that looks valid. |
| 012 | **Companion contract details:** bodyless 204 acks, no cancel once delivery has started, a non-queueing token preview, at most two conversions at a time. |
| 013 | **Companion implementation:** one process, one SQLite database in WAL mode with full sync, payload blobs in the database, a quota on pending payload space. |
| 014 | **Retention and retries:** old finished jobs are pruned, pending jobs never are; failed upstream fetches retry with growing delays. |
| 015 | **Articles from a link:** the companion fetches the page with strict limits (timeouts, redirects, size, content type) and a resolver that refuses private addresses. |
| 016 | **Token image quality:** Lanczos3 resampling, a light levels stretch, unsharp mask and a tone curve before dithering. |
| 017 | **Restore path:** the previous firmware stays in `app0`; RustInker-X3 goes in `app1`. "Switch firmware" in the Device menu switches back; the full USB backup is the fallback. |
| 018 | **Four-grey tokens:** 2 bits per pixel, Atkinson diffusion to four even tones. |
| 019 | **UC8253 panel constants** ported from MarigoldOS (MIT), black-and-white banks only, checked byte for byte. |
| 020 | **Reader text:** Literata bitmap fonts (OFL), body 24 px and headings 28 px; hyphenation is done on the companion. |
| 021 | **One Receive session in core**, shared by the device and the host tests; the item index is read once per session. |
| 022 | **Grey method:** a base refresh, a settle bank, the two grey planes, then a short grey nudge; the screen is scrubbed before the next black-and-white update. From CrossPoint and CrossPlant (MIT). |
| 023 | **Article cache:** up to 32 articles on the card; the 33rd evicts the oldest finished one. Tokens keep the last 20. |
| 024 | **Bottom buttons:** Left, Right, Back, Confirm from left to right; the side keys are Up and Down. |
| 025 | **Wi-Fi setup from the phone:** a short-lived WPA2 hotspot, a QR code on the screen and one setup page; up to four saved networks. |
| 026 | **Receive from any network:** signed requests and replies over TLS 1.3. The server certificate is not checked; the signature, keyed by the device token, protects every request and reply. |
| 027 | **Long articles:** the reader holds a 48 KiB window of the text and moves it as you read, so books of any length open. |
| 028 | **Pictures:** an `image` job kind (`im1`, laid out as `tk1`) for your own images; tokens and pictures share one Pictures list. |
| 029 | **Companion page:** an inbox of the newest 10, a saved shelf of the last 50 tokens and pictures with Send again, and a tokens-only ranked search. |
| 030 | **Scryfall rules:** per-endpoint rate limits, a 24 h cache, whole cards only (never cropped), a credit line on the page. Greys are shown on each user's own device, as a personal display. |
