# Security

RustInker-X3 is a small hobby project. This page says what it protects against, how, and
what it does not. It is a self-review, **not an independent audit**.

## Threat model

- **In scope:** another device on your Wi-Fi; someone on the internet trying to reach your
  companion; a hostile or broken web page, image or Scryfall response; a hostile companion
  reply; a power cut at the wrong moment.
- **Out of scope:** someone holding your X3. They can read the SD card (Wi-Fi passwords,
  device token) and use the USB serial console. Keep nothing on the card you would not
  give to a finder.

## How it is protected

**On the device**

- Every network input is bounded: status line and headers 1,024 bytes, text lines 192
  bytes, job lists 4,096 bytes, payloads to their format's size. Anything over a cap ends
  the session.
- Payloads are checked (length, SHA-256, magic, CRC, UTF-8) before anything becomes
  visible. A failed check is rejected and its partial file removed at the next Receive.
- File names on the card come only from generation numbers, never from titles, URLs or item
  IDs, so path traversal cannot happen. Item IDs are limited to `[A-Za-z0-9_-]`, 48 bytes.
- Config files are capped at 512 bytes, and a parse error never prints the file.
- Storage survives power loss: a host test cut a whole Receive at every write (3,162 cuts)
  and the device was reset in the middle of downloads; every case recovered with nothing
  damaged and nothing left over.

**Companion**

- The web page and API need a valid Cloudflare Access token: RS256, key ID, issuer,
  audience, expiry and not-before are checked; keys are fetched over HTTPS without
  redirects, with a size cap and a refresh limit.
- The device API needs a per-device token, stored only as a SHA-256 hash.
- Article links: private, loopback, link-local, metadata and similar addresses are refused
  when the name is resolved, so redirects and DNS tricks cannot reach them.
- Uploaded images are decoded with limits (8 MiB file, 4,096 px a side, 64 MiB decoded).
- The test mode without Access refuses to start unless every listener is on loopback.
- The container runs read-only, unprivileged, with no capabilities.

**Receive away from home.** The X3 does not check the TLS certificate. Instead every request
and reply is signed with a key derived from the device token, which is never sent, and
nonces are not accepted twice. Someone on the same Wi-Fi can read what is fetched, but
cannot forge a job, change a payload or learn the token. An Access service token keeps
scanners away from the companion.

**Wi-Fi setup hotspot.** WPA2 with a random 10-character password shown only on the
screen, one client at a time, off after 5 minutes or on Back. Requests are capped (1 KiB
head, 256 bytes body) and strictly parsed; the page escapes every network name. Passwords
typed on the page are not logged.

**Supply chain.** `cargo deny check` enforces advisories, licences, bans and sources
([`deny.toml`](../deny.toml)); only crates.io, no git dependencies. Every crate in the
firmware is permissively licensed.

## Known limits

| Limit | Why it is accepted |
| --- | --- |
| On the LAN the device token travels in plain HTTP | keep the device listener on a trusted home network; never expose it |
| Wi-Fi passwords, device token and service token sit unencrypted on the card | the ESP32-C3 has no safe store without flash encryption, which would break switching back to the previous firmware |
| The USB serial console needs no login (it can switch the boot slot) | physical access only |
| No confidentiality against an active attacker on the away-from-home route | the signatures still stop forged or changed content |
| A stolen companion database could forge device requests | it holds the token hash, which is the signing key; protect the data directory |
| Nonces are forgotten when the companion restarts | a replay would need a captured request and a restart within its window |
| Two advisories reach the companion through `jsonwebtoken` (RSA and big-integer timing) | it only verifies with public keys, holds no private key and pins RS256 |
| Espressif's prebuilt Wi-Fi libraries are not reviewed | they are the vendor's radio stack, used as shipped |

## Not done

- No fuzzing; the parsers have hostile-input tests, not a fuzzer.
- No penetration test of a deployed Cloudflare and proxy setup.
- No test of real SD cards losing power in the middle of a write.

## Reporting a problem

Open an issue on the project page and say what you saw. For anything sensitive, do not
paste `LOG.TXT`, `WIFI.CFG`, `SERVER.CFG`, a flash backup or a companion database.
