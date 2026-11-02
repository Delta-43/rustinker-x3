# Third-party material

No MTG art, Scryfall dump or private backup is included. One upstream firmware
port is included: the UC8253 panel constants below (ADR 019). Original sample art and text are dual MIT OR Apache-2.0.

## Release audit (2026-11-01)

Dependency licences were read from `Cargo.lock` with `cargo metadata` and checked
with `cargo deny check` (policy: `deny.toml`; all four checks pass). `cargo audit`
agrees (`.cargo/audit.toml`).

- **Third-party crates:** 431 distinct crates, none without a licence.
- **Device firmware (`fw`):** 148 crates, every one permissive: MIT, Apache-2.0, ISC,
  Zlib, 0BSD, Unlicense, or a combination with Unicode-3.0.
- **Companion only (a separate program on your server, not in the device image):**
  `cssparser`, `cssparser-macros`, `dtoa-short` and `selectors` are **MPL-2.0**
  (file-level copyleft). They are used unmodified from crates.io, so their source is
  available from there; keep their licence notices in the Docker image.
  `webpki-root-certs` is **CDLA-Permissive-2.0** (root certificate data).
  `r-efi` offers LGPL-2.1-or-later only as an alternative to MIT OR Apache-2.0; the
  permissive choice is the one used.
- **Prebuilt radio libraries.** `esp-wifi-sys-esp32c3` ships Espressif's prebuilt
  Wi-Fi and PHY libraries (`libnet80211.a`, `libphy.a`, `libpp.a`, `libcoexist.a`
  and others). The crate declares MIT OR Apache-2.0 but its archive has no separate
  notice for the libraries. **Checked 2026-11-02:** the libraries come from
  espressif/esp32-wifi-lib, esp-phy-lib and esp-coex-lib, whose READMEs say they are
  provided under ESP-IDF's licence, Apache-2.0, in object form; none publishes a
  NOTICE file. A released firmware file therefore ships with `NOTICES.txt` from
  `tools/notices.py`: the Apache-2.0 text, Espressif's copyright line, and the licence
  files of every crate linked into the firmware (fallback copyright lines for the
  seven MIT/ISC crates that publish none are recorded in the script).
- **Banner and icon** (`assets/brand/`): the word "RustInker" is rendered in
  Dancing Script (SIL OFL 1.1, Copyright 2016 The Dancing Script Project Authors,
  google/fonts). Images made with an OFL font are not bound by the OFL; the font file
  is not in the repository. `assets/brand/brand.py` redraws it and the icon
  (`icon.png`, `icon-256.png`); the X3 drawing in both is original line work, not a
  product photo.
- **Not in the repository:** MTG card art, Scryfall data, General Sans (its licence
  forbids redistribution), flash dumps, credentials.
- **Advisories:** two reach the companion (`crypto-bigint`, `rsa`, through
  `jsonwebtoken`) and are accepted with reasons in `deny.toml` and
  [docs/SECURITY.md](docs/SECURITY.md). The device firmware has none.

## Firmware references (not incorporated)

Root licence texts inspected at the revisions linked in docs/HARDWARE.md:
MarigoldOS: MIT (2026 Jon-Vii); CrossPoint: MIT (2025 CrossPoint Reader
organization); CrossPlant and CrossInk: MIT (2025 Dave Allie). Check file-level notices
before any future copying. Rustmix has no detected root licence in the inspected
tree; TrustyReader access returned 403 and its licence remains unverified.
No permission to copy either is inferred. ADR 001 records the fresh implementation.

## UC8253 four-grey method (incorporated, ADR 022)

`crates/fw/src/epd.rs` contains the X3 grey nudge (`GC`) and settle
(`PRE_BW_MID`) waveform tables, their CDI values (0x29/0xA9, 0x07) and the
grey sequence (base, settle, low plane to DTM1, high plane to DTM2, nudge,
white scrub with the half bank), from freeink-sdk as vendored by CrossPlant
[98a528f](https://github.com/0xknowles/crossplant/tree/98a528f8b4612a55ca715cca56fce894333668ed)
(`freeink-sdk/libs/display/FreeInkDisplay/src/lut/Uc8253X3Luts.h`,
`src/driver/Uc8253X3Driver.cpp`); the level-to-plane mapping from CrossPoint's
`lib/GfxRenderer/GlyphBitmap.h`. The header says the banks are relocated
from the community-sdk lineage; `aa_pre_bw_mid` is described there as
extracted from Xteink's OEM update V5.6.33. No file-level notices. MIT:

    Copyright (c) 2025 Dave Allie
    Copyright (c) 2025 CrossPoint Reader organization

(full MIT text as in the section below).

## UC8253 panel driver constants (incorporated, ADR 019)

`crates/fw/src/epd.rs` contains the X3 UC8253 init bytes, CDI values, the
HALF, FAST and FULL waveform tables, the refresh sequences, reset timing and
the two-phase BUSY handshake, ported from MarigoldOS
[4778b2e](https://github.com/chongfun/MarigoldOS/tree/4778b2e5b033b3b4054647a388e7a6ee202eca98)
(`display/src/epd/uc8253.rs`, `fw/src/display_flush/uc8253.rs`,
`hal-ext/src/spi_dma.rs`). MarigoldOS ports them from CrossPoint's
`Uc8253X3Driver`. No file-level notices in those files. Both are MIT:

    Copyright (c) 2026 Jon-Vii
    Copyright (c) 2025 CrossPoint Reader organization

    Permission is hereby granted, free of charge, to any person obtaining a copy
    of this software and associated documentation files (the "Software"), to deal
    in the Software without restriction, including without limitation the rights
    to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
    copies of the Software, and to permit persons to whom the Software is
    furnished to do so, subject to the following conditions:

    The above copyright notice and this permission notice shall be included in all
    copies or substantial portions of the Software.

    THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
    IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
    FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
    AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
    LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
    OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
    SOFTWARE.

## Additional architecture references (not incorporated)

- [ESPHome Tailscale licence](https://raw.githubusercontent.com/Csontikka/esphome-tailscale/main/LICENSE):
  MIT, copyright 2026 Csontikka; its dependency notices need separate review if used.
- [MicroLink licence](https://raw.githubusercontent.com/CamM2325/microlink/main/LICENSE):
  MIT, copyright 2025–2026 Cameron Malone, with separate X25519 notice.
- [Mozilla Readability licence](https://raw.githubusercontent.com/mozilla/readability/main/LICENSE.md):
  Apache-2.0; candidate companion extractor only, not a dependency.
- chongfun/MarigoldOS now describes itself as CalendulaOS on its main README.
  Its current root licence still credits 2026 Jon-Vii under MIT. No new source,
  font, waveform or implementation is imported. Pin and audit a chosen revision
  and its notices before any adoption.

## Fonts

The simulator uses `embedded-graphics` 0.8.2 `iso_8859_2::FONT_10X20`.
The distributed crate's `fonts/src/README.md` states that its source fonts are
public domain from [X.Org misc-misc](https://gitlab.freedesktop.org/xorg/font/misc-misc).
The distributed `fonts/src/10x20.bdf` and converted data stay inside the dependency;
no loose font files were copied. The renderer crate is MIT OR Apache-2.0.
Its [font source README](https://docs.rs/crate/embedded-graphics/0.8.2/source/fonts/src/README.md)
and shipped crate files were inspected locally. Latin-2 includes Polish letters;
unsupported emoji uses the fallback glyph. No font with unclear terms is bundled.

Device fonts (2026-10-27, ADR 020):

- **Literata 3.103** rasterised to `assets/fonts/*.x3f` (Regular 24 px, Bold
  28 px). SIL Open Font License 1.1, copyright 2017 The Literata Project
  Authors; no Reserved Font Name. Licence text `assets/fonts/Literata-OFL.txt`
  (identical to the companion copy). Source release and hashes in
  `assets/fonts/README.md`. TTFs are not stored in the repo.
- **fontdue 0.9.4** (host tool only, MIT OR Apache-2.0 OR Zlib) rasterises them.

Companion web page (2026-10-24):

- **Literata** (variable, latin and latin-ext, normal style) is embedded in the
  companion and served at `/fonts/`. SIL Open Font License 1.1, copyright 2017
  The Literata Project Authors. Licence text: `companion/web/fonts/Literata-OFL.txt`.
  Files from fontsource via the owner's design library.
- **General Sans** (Indian Type Foundry, Fontshare) is **not** in this repository.
  The ITF Free Font License forbids redistribution through a repository or an
  application. The companion serves it only from an optional private folder
  (`X3_FONT_DIR`) on the owner's own server; without it the system sans is used.
- **Tabler Icons** (MIT): eight outline icons inlined as SVG in
  `companion/web/index.html` (link, file-text, cards, search, eye, refresh,
  device-tablet). https://github.com/tabler/tabler-icons
- Colour, type and spacing values come from the owner's own Field Journal design
  system (`companion/web/tokens.css`); no third-party design assets.

## Locked Rust dependencies

Licence expressions below were read from each downloaded crate manifest. This
is an inventory, not a completed release audit. Cargo.lock records checksums.
For distributed binaries, retain applicable upstream notices and licence texts;
release assembly and cargo-deny review remain Phase 8 work.

| Crate | Version | Manifest licence |
| --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| az | 1.2.1 | MIT/Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 |
| embedded-graphics | 0.8.2 | MIT OR Apache-2.0 |
| embedded-graphics-core | 0.4.1 | MIT OR Apache-2.0 |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| float-cmp | 0.9.0 | MIT |
| image | 0.25.10 | MIT OR Apache-2.0 |
| micromath | 2.1.0 | Apache-2.0 OR MIT |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| png | 0.18.1 | MIT OR Apache-2.0 |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| zlib-rs | 0.6.8 | Zlib |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib |
| arbitrary-int | 2.2.0 | MIT |
| bitbybit | 2.0.1 | MIT |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |
| digest | 0.11.3 | MIT OR Apache-2.0 |
| embedded-hal | 1.0.0 | MIT OR Apache-2.0 |
| embedded-io | 0.7.1 | MIT OR Apache-2.0 |
| embedded-sdmmc | 0.10.0 | MIT OR Apache-2.0 |
| embedded-sdmmc-types | 0.1.0 | MIT OR Apache-2.0 |
| hash32 | 0.3.1 | MIT OR Apache-2.0 |
| heapless | 0.9.3 | MIT OR Apache-2.0 |
| hex-literal | 1.1.0 | MIT OR Apache-2.0 |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| sha2 | 0.11.1 | MIT OR Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| aws-lc-rs | 1.18.2 | ISC AND (Apache-2.0 OR ISC) |
| aws-lc-sys | 0.46.0 | ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0) |
| axum | 0.8.9 | MIT |
| axum-core | 0.5.6 | MIT |
| base16ct | 0.2.0 | Apache-2.0 OR MIT |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| base64ct | 1.8.3 | Apache-2.0 OR MIT |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 |
| bytes | 1.12.1 | MIT |
| cc | 1.9.0 | MIT OR Apache-2.0 |
| cfg_aliases | 0.2.2 | MIT |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |
| cmake | 0.1.58 | MIT OR Apache-2.0 |
| combine | 4.6.9 | MIT |
| const-oid | 0.9.6 | Apache-2.0 OR MIT |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| crypto-bigint | 0.5.5 | Apache-2.0 OR MIT |
| crypto-common | 0.1.6 | MIT OR Apache-2.0 |
| curve25519-dalek | 4.1.3 | BSD-3-Clause |
| curve25519-dalek-derive | 0.1.1 | MIT/Apache-2.0 |
| der | 0.7.10 | Apache-2.0 OR MIT |
| deranged | 0.5.9 | MIT OR Apache-2.0 |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| ecdsa | 0.16.9 | Apache-2.0 OR MIT |
| ed25519 | 2.2.3 | Apache-2.0 OR MIT |
| ed25519-dalek | 2.2.0 | BSD-3-Clause |
| elliptic-curve | 0.13.8 | Apache-2.0 OR MIT |
| errno | 0.3.14 | MIT OR Apache-2.0 |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |
| ff | 0.13.1 | MIT/Apache-2.0 |
| fiat-crypto | 0.2.9 | MIT OR Apache-2.0 OR BSD-1-Clause |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |
| foldhash | 0.2.0 | Zlib |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| fs_extra | 1.3.0 | MIT |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 |
| futures-core | 0.3.34 | MIT OR Apache-2.0 |
| futures-task | 0.3.34 | MIT OR Apache-2.0 |
| futures-util | 0.3.34 | MIT OR Apache-2.0 |
| generic-array | 0.14.9 | MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| group | 0.13.0 | MIT/Apache-2.0 |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| hashlink | 0.12.2 | MIT OR Apache-2.0 |
| hkdf | 0.12.4 | MIT OR Apache-2.0 |
| hmac | 0.12.1 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| http-body | 1.1.0 | MIT |
| http-body-util | 0.1.5 | MIT |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| httpdate | 1.0.3 | MIT OR Apache-2.0 |
| hyper | 1.12.0 | MIT |
| hyper-rustls | 0.27.10 | Apache-2.0 OR ISC OR MIT |
| hyper-util | 0.1.21 | MIT |
| icu_collections | 2.3.0 | Unicode-3.0 |
| icu_locale_core | 2.3.0 | Unicode-3.0 |
| icu_normalizer | 2.3.0 | Unicode-3.0 |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 |
| icu_properties | 2.3.0 | Unicode-3.0 |
| icu_properties_data | 2.3.0 | Unicode-3.0 |
| icu_provider | 2.3.1 | Unicode-3.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |
| ipnet | 2.12.2 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| jni | 0.22.4 | MIT OR Apache-2.0 |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 |
| jobserver | 0.1.35 | MIT OR Apache-2.0 |
| js-sys | 0.3.106 | MIT OR Apache-2.0 |
| jsonwebtoken | 11.1.0 | MIT |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 |
| libm | 0.2.16 | MIT |
| libsqlite3-sys | 0.38.2 | MIT |
| litemap | 0.8.3 | Unicode-3.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| lru-slab | 0.1.3 | MIT OR Apache-2.0 OR Zlib |
| matchit | 0.8.4 | MIT AND BSD-3-Clause |
| memchr | 2.8.3 | Unlicense OR MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| mio | 1.2.4 | MIT |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 |
| num-bigint-dig | 0.8.6 | MIT/Apache-2.0 |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| num-integer | 0.1.47 | MIT OR Apache-2.0 |
| num-iter | 0.1.46 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| openssl-probe | 0.2.1 | MIT OR Apache-2.0 |
| p256 | 0.13.2 | Apache-2.0 OR MIT |
| p384 | 0.13.1 | Apache-2.0 OR MIT |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| pem | 3.0.6 | MIT |
| pem-rfc7468 | 0.7.0 | Apache-2.0 OR MIT |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| pkcs1 | 0.7.5 | Apache-2.0 OR MIT |
| pkcs8 | 0.10.2 | Apache-2.0 OR MIT |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 |
| potential_utf | 0.1.6 | Unicode-3.0 |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| primeorder | 0.13.6 | Apache-2.0 OR MIT |
| quinn | 0.11.12 | MIT OR Apache-2.0 |
| quinn-proto | 0.11.19 | MIT OR Apache-2.0 |
| quinn-udp | 0.5.16 | MIT OR Apache-2.0 |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later |
| rand | 0.8.8 | MIT OR Apache-2.0 |
| rand | 0.10.3 | MIT OR Apache-2.0 |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |
| rand_pcg | 0.10.2 | MIT OR Apache-2.0 |
| redox_syscall | 0.5.18 | MIT |
| reqwest | 0.13.5 | MIT OR Apache-2.0 |
| rfc6979 | 0.4.0 | Apache-2.0 OR MIT |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rsa | 0.9.10 | MIT OR Apache-2.0 |
| rsqlite-vfs | 0.1.1 | MIT |
| rusqlite | 0.40.2 | MIT |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |
| rustls-native-certs | 0.8.5 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 |
| rustls-platform-verifier | 0.7.1 | MIT OR Apache-2.0 |
| rustls-platform-verifier-android | 0.2.0 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.16 | ISC |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| schannel | 0.1.30 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| sec1 | 0.7.3 | Apache-2.0 OR MIT |
| security-framework | 3.7.0 | MIT OR Apache-2.0 |
| security-framework-sys | 2.17.0 | MIT OR Apache-2.0 |
| semver | 1.0.28 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| serde_path_to_error | 0.1.20 | MIT OR Apache-2.0 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| shlex | 2.0.1 | MIT OR Apache-2.0 |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 |
| signature | 2.2.0 | Apache-2.0 OR MIT |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |
| simple_asn1 | 0.6.4 | ISC |
| slab | 0.4.12 | MIT |
| smallvec | 1.16.3 | MIT OR Apache-2.0 |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |
| spin | 0.9.9 | MIT |
| spki | 0.7.3 | Apache-2.0 OR MIT |
| sqlite-wasm-rs | 0.5.5 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| sync_wrapper | 1.0.2 | Apache-2.0 |
| synstructure | 0.14.0 | MIT |
| time | 0.3.55 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.32 | MIT OR Apache-2.0 |
| tinystr | 0.8.4 | Unicode-3.0 |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT |
| tokio | 1.54.1 | MIT |
| tokio-macros | 2.7.2 | MIT |
| tokio-rustls | 0.26.6 | MIT OR Apache-2.0 |
| tower | 0.5.3 | MIT |
| tower-http | 0.6.11 | MIT |
| tower-layer | 0.3.3 | MIT |
| tower-service | 0.3.3 | MIT |
| tracing | 0.1.44 | MIT |
| tracing-core | 0.1.36 | MIT |
| try-lock | 0.2.5 | MIT |
| ulid | 3.0.0 | MIT |
| untrusted | 0.9.0 | ISC |
| url | 2.5.8 | MIT OR Apache-2.0 |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT |
| uuid | 1.28.0 | Apache-2.0 OR MIT |
| vcpkg | 0.2.15 | MIT/Apache-2.0 |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| walkdir | 2.5.0 | Unlicense/MIT |
| want | 0.3.2 | MIT |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 |
| web-sys | 0.3.106 | MIT OR Apache-2.0 |
| web-time | 1.1.0 | MIT OR Apache-2.0 |
| webpki-root-certs | 1.0.10 | CDLA-Permissive-2.0 |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| writeable | 0.6.4 | Unicode-3.0 |
| yoke | 0.8.3 | Unicode-3.0 |
| yoke-derive | 0.8.4 | Unicode-3.0 |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.8 | Unicode-3.0 |
| zeroize | 1.9.1 | Apache-2.0 OR MIT |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT |
| zerotrie | 0.2.5 | Unicode-3.0 |
| zerovec | 0.11.8 | Unicode-3.0 |
| zerovec-derive | 0.11.6 | Unicode-3.0 |
| zmij | 1.0.23 | MIT |

## Firmware link proof dependencies (2026-10-22)

Exact new lockfile packages and declared licences from downloaded crates.io
manifests follow. No upstream firmware application code was copied.
`esp-wifi-sys-esp32c3` ships prebuilt Espressif radio libraries; its manifest
and README declare MIT OR Apache-2.0, but the crate archive does not contain
separate blob licence notices. Confirm the underlying ESP-IDF binary notices
and include required notices before distributing a firmware binary (Phase 8).
This host link proof is not a firmware release.

| Crate | Version | Declared licence |
| --- | --- | --- |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| allocator-api2 | 0.3.1 | MIT OR Apache-2.0 |
| bitfield | 0.19.5 | MIT OR Apache-2.0 |
| bitfield-macros | 0.19.5 | MIT OR Apache-2.0 |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| const-default | 1.0.0 | MIT |
| cordyceps | 0.3.5 | MIT |
| critical-section | 1.2.0 | MIT OR Apache-2.0 |
| darling | 0.20.11 | MIT |
| darling | 0.21.3 | MIT |
| darling | 0.24.1 | MIT |
| darling_core | 0.20.11 | MIT |
| darling_core | 0.21.3 | MIT |
| darling_core | 0.24.1 | MIT |
| darling_macro | 0.20.11 | MIT |
| darling_macro | 0.21.3 | MIT |
| darling_macro | 0.24.1 | MIT |
| defmt | 1.2.0 | MIT OR Apache-2.0 |
| defmt-macros | 1.2.0 | MIT OR Apache-2.0 |
| defmt-parser | 1.1.0 | MIT OR Apache-2.0 |
| delegate | 0.13.5 | MIT OR Apache-2.0 |
| docsplay | 0.1.3 | MIT OR Apache-2.0 |
| docsplay-macros | 0.1.2 | MIT OR Apache-2.0 |
| document-features | 0.2.12 | MIT OR Apache-2.0 |
| embassy-embedded-hal | 0.6.0 | MIT OR Apache-2.0 |
| embassy-executor | 0.10.0 | MIT OR Apache-2.0 |
| embassy-executor-macros | 0.8.0 | MIT OR Apache-2.0 |
| embassy-executor-timer-queue | 0.1.0 | MIT OR Apache-2.0 |
| embassy-futures | 0.1.2 | MIT OR Apache-2.0 |
| embassy-hal-internal | 0.4.0 | MIT OR Apache-2.0 |
| embassy-net | 0.9.1 | MIT OR Apache-2.0 |
| embassy-net-driver | 0.2.0 | MIT OR Apache-2.0 |
| embassy-sync | 0.6.2 | MIT OR Apache-2.0 |
| embassy-sync | 0.7.2 | MIT OR Apache-2.0 |
| embassy-sync | 0.8.0 | MIT OR Apache-2.0 |
| embassy-time | 0.5.1 | MIT OR Apache-2.0 |
| embassy-time-driver | 0.2.2 | MIT OR Apache-2.0 |
| embassy-time-queue-utils | 0.3.2 | MIT OR Apache-2.0 |
| embedded-can | 0.4.1 | MIT OR Apache-2.0 |
| embedded-hal | 0.2.7 | MIT OR Apache-2.0 |
| embedded-hal-async | 1.0.0 | MIT OR Apache-2.0 |
| embedded-io | 0.6.1 | MIT OR Apache-2.0 |
| embedded-io-async | 0.6.1 | MIT OR Apache-2.0 |
| embedded-io-async | 0.7.0 | MIT OR Apache-2.0 |
| embedded-nal | 0.9.0 | MIT OR Apache-2.0 |
| embedded-nal-async | 0.9.0 | MIT OR Apache-2.0 |
| embedded-storage | 0.3.2 | MIT OR Apache-2.0 |
| embedded-storage-async | 0.4.2 | MIT OR Apache-2.0 |
| enumset | 1.1.14 | MIT/Apache-2.0 |
| enumset_derive | 0.15.0 | MIT/Apache-2.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| esp-alloc | 0.10.0 | MIT OR Apache-2.0 |
| esp-bootloader-esp-idf | 0.5.0 | MIT OR Apache-2.0 |
| esp-config | 0.7.0 | MIT OR Apache-2.0 |
| esp-hal | 1.1.2 | MIT OR Apache-2.0 |
| esp-hal-procmacros | 0.22.0 | MIT OR Apache-2.0 |
| esp-metadata-generated | 0.4.0 | MIT OR Apache-2.0 |
| esp-metadata-generated | 0.5.3 | MIT OR Apache-2.0 |
| esp-phy | 0.2.0 | MIT OR Apache-2.0 |
| esp-radio | 0.18.0 | MIT OR Apache-2.0 |
| esp-radio-rtos-driver | 0.3.0 | MIT OR Apache-2.0 |
| esp-riscv-rt | 0.14.0 | MIT OR Apache-2.0 |
| esp-rom-sys | 0.1.5 | MIT OR Apache-2.0 |
| esp-rtos | 0.3.0 | MIT OR Apache-2.0 |
| esp-storage | 0.9.0 | MIT OR Apache-2.0 |
| esp-sync | 0.2.1 | MIT OR Apache-2.0 |
| esp-wifi-sys-esp32c3 | 0.2.0 | MIT OR Apache-2.0 |
| esp32 | 0.40.2 | MIT OR Apache-2.0 |
| esp32c2 | 0.29.2 | MIT OR Apache-2.0 |
| esp32c3 | 0.32.2 | MIT OR Apache-2.0 |
| esp32c3 | 0.33.0 | MIT OR Apache-2.0 |
| esp32c6 | 0.23.2 | MIT OR Apache-2.0 |
| esp32h2 | 0.19.2 | MIT OR Apache-2.0 |
| esp32s2 | 0.31.2 | MIT OR Apache-2.0 |
| esp32s3 | 0.35.2 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| fugit | 0.3.9 | MIT OR Apache-2.0 |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 |
| gcd | 2.3.0 | MIT/Apache-2.0 |
| generator | 0.8.11 | MIT/Apache-2.0 |
| heapless | 0.8.0 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| indoc | 2.0.8 | MIT OR Apache-2.0 |
| instability | 0.3.14 | MIT |
| jiff | 0.2.38 | Unlicense OR MIT |
| jiff-core | 0.1.1 | Unlicense OR MIT |
| jiff-static | 0.2.38 | Unlicense OR MIT |
| linked_list_allocator | 0.10.6 | Apache-2.0/MIT |
| litrs | 1.0.1 | MIT OR Apache-2.0 |
| loom | 0.7.2 | MIT |
| managed | 0.8.0 | 0BSD |
| matchers | 0.2.0 | MIT |
| nb | 0.1.3 | MIT OR Apache-2.0 |
| nb | 1.1.0 | MIT OR Apache-2.0 |
| nu-ansi-term | 0.50.3 | MIT |
| num-derive | 0.4.2 | MIT OR Apache-2.0 |
| paste | 1.0.15 | MIT OR Apache-2.0 |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT |
| portable_atomic_enum | 0.3.1 | MIT |
| portable_atomic_enum_macros | 0.2.1 | MIT |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 |
| rand_core | 0.9.5 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| riscv | 0.15.0 | ISC |
| riscv-macros | 0.3.0 | MIT OR Apache-2.0 |
| riscv-pac | 0.2.0 | ISC |
| riscv-rt | 0.16.0 | ISC |
| riscv-rt-macros | 0.6.1 | MIT OR Apache-2.0 |
| riscv-target-parser | 0.1.3 | ISC |
| rlsf | 0.2.3 | MIT/Apache-2.0 |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 |
| serde_yaml | 0.9.34+deprecated | MIT OR Apache-2.0 |
| sharded-slab | 0.1.7 | MIT |
| smoltcp | 0.13.1 | 0BSD |
| somni-expr | 0.2.0 | MIT OR Apache-2.0 |
| somni-parser | 0.2.2 | MIT OR Apache-2.0 |
| strsim | 0.11.1 | MIT |
| strum | 0.27.2 | MIT |
| strum_macros | 0.27.2 | MIT |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thread_local | 1.1.10 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_edit | 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| tracing-attributes | 0.1.31 | MIT |
| tracing-log | 0.2.0 | MIT |
| tracing-subscriber | 0.3.23 | MIT |
| ufmt-write | 0.1.0 | MIT OR Apache-2.0 |
| unsafe-libyaml | 0.2.11 | MIT |
| valuable | 0.1.1 | MIT |
| vcell | 0.1.3 | MIT OR Apache-2.0 |
| void | 1.0.2 | MIT |
| windows-result | 0.4.1 | MIT OR Apache-2.0 |
| winnow | 1.0.4 | MIT |
| xtensa-lx | 0.13.0 | MIT OR Apache-2.0 |
| xtensa-lx-rt | 0.22.0 | MIT OR Apache-2.0 |
| xtensa-lx-rt-proc-macros | 0.5.0 | MIT OR Apache-2.0 |

## Companion link reading (2026-10-24, ADR 015)

Direct pins: `dom_smoothie` 0.18.2 (MIT, Mozilla Readability port),
`url` 2.5.8 (MIT OR Apache-2.0), `encoding_rs` 0.8.42
((Apache-2.0 OR MIT) AND BSD-3-Clause). New transitive crates and licences,
from `cargo metadata --locked`:

| Licence | Crates |
| --- | --- |
| MIT | derive_more, derive_more-impl, dom_query, gjson, html-escape, new_debug_unreachable, nom, phf, phf_codegen, phf_generator, phf_macros, phf_shared, precomputed-hash |
| MIT OR Apache-2.0 | bit-set, bit-vec, core_detect, dtoa, fastrand, html5ever, markup5ever, multiversion_no_op, servo_arc, siphasher, string_cache, string_cache_codegen, tendril, unicode-segmentation, web_atoms |
| Apache-2.0 | flagset |
| **MPL-2.0** | cssparser 0.37.0, cssparser-macros 0.7.1, dtoa-short 0.3.5, selectors 0.38.0 |

MPL-2.0 is file-level copyleft. These crates are used unmodified as
dependencies, which is compatible with this project's MIT OR Apache-2.0
licence. A binary or container image that includes them must point to their
source (crates.io and the Servo repositories); modified MPL files would have to
stay under MPL. No MPL code is copied into this repository.

## Companion hyphenation (2026-10-27, ADR 020)

Direct pin: `hypher` 0.1.8 (MIT OR Apache-2.0, typst/hypher), features
`english` and `polish` only; no other crates added. It embeds tries built from
two hyph-utf8 pattern files, both checked in the upstream repository's
`patterns/` folder on 2026-10-27:

| Patterns | Copyright | Licence |
| --- | --- | --- |
| `hyph-en-us.tex` (2005-05-30) | © 1990, 2004, 2005 Gerard D.C. Kuiken | "Copying and distribution of this file, with or without modification, are permitted in any medium without royalty provided the copyright notice and this notice are preserved." |
| `hyph-pl.tex` (3.0a, 1995) | © 1987–1995 Hanna Kołodziejska, Bogusław Jackowski, Marek Ryćko | MIT, or public domain (either) |

The patterns run in the companion only; the firmware carries none.

## Wi-Fi setup QR code (2026-11-01, ADR 025)

Direct pin: `qrcodegen-no-heap` 1.8.1 (MIT, © Project Nayuki), in `x3-core`, so in
the firmware. It draws the QR code that joins the setup hotspot. No other crate was
added. `cargo deny check` passes with it.
