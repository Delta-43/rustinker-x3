# Security review

Reviewed 2026-11-01 by the project's coding agent (Claude Code), with the owner's
tests. This is a self-review of a small hobby project, **not an independent audit**.
It lists what was looked at, how, and what is left. Hardware claims carry their
evidence in [TESTING.md](TESTING.md).

## Threat model

- **In scope:** another device on your home Wi-Fi; someone on the internet trying to
  reach your companion; a hostile or broken web page or Scryfall response; a broken
  or hostile companion reply; a power cut at the wrong moment.
- **Out of scope:** someone holding your device. With the device in hand they can read
  the SD card (Wi-Fi password, device token) and use the USB console. That is
  inherent. Do not store anything on the card you would not give to a finder.

## Findings

| # | Area | Finding | State |
| --- | --- | --- | --- |
| 1 | Device log | A failed Wi-Fi join wrote the network name and the router's hardware address to `/X3/LOG.TXT`. People paste that file into bug reports. | **Fixed:** only the reason code and signal strength are kept. Built and checked; not yet seen on the device. |
| 2 | Device to companion | Plain HTTP on the home LAN. The device token crosses the LAN in clear. | **Accepted** (design, ADR 008). Keep the device listener on a trusted network. Never expose it to the internet. |
| 3 | SD card | Wi-Fi password and device token are stored unencrypted in `X3/WIFI.CFG` and `X3/SERVER.CFG`. | **Accepted.** The ESP32-C3 has no safe place for them without flash encryption, which would break the restore path. |
| 4 | USB console | Debug commands need no login, including `crosspoint` (switches the boot slot), `sleep` and `drill` (rewrites `SERVER.CFG`). | **Accepted:** physical access only. Out of scope above. |
| 5 | Companion dependencies | Two advisories reach the companion through `jsonwebtoken` (timing issues in RSA and big-integer code). | **Accepted with reasons** in `deny.toml`: the companion only verifies tokens with public keys, holds no private key and pins RS256. Re-check on each dependency update. |
| 6 | Unmaintained crates | `paste`, `unsafe-libyaml`, `ttf-parser`. | **Accepted:** build-time or host-tool only, no vulnerability. |
| 7 | Prebuilt radio libraries | The Wi-Fi stack ships Espressif's prebuilt libraries (`libnet80211`, `libphy`, `libpp` and others). | **Noted.** This repository distributes source only. See [THIRD_PARTY.md](../THIRD_PARTY.md) before ever distributing a built image. |

## What was checked

**Device, input handling** (all bounded; tests include malformed, truncated,
oversized and hostile input):

- HTTP responses: header, line and body sizes are capped and checked against
  `Content-Length`; unexpected statuses and framing errors end the session.
  Tests: `crates/core/tests/receive.rs` (`hostile_responses`, `statuses_and_network_errors`).
- Payloads: magic, lengths, UTF-8, CRC and SHA-256 are checked before anything becomes
  visible. A mismatch is rejected, and the partial file is cleaned up by the next
  Receive.
- File names come only from numeric generation numbers (eight hex digits), never from
  titles, URLs or item IDs, so path traversal is not possible. Item IDs are limited
  to `[A-Za-z0-9_-]`, 48 bytes.
- Config files are size-capped (512 bytes); a parse error never prints the file.
- Storage survives power loss: a host sweep cut a whole Receive at every write
  (3,162 cuts) and the device was reset mid-download five times. All recovered with
  nothing damaged and nothing left over.

**Companion:**

- Public API: every request needs a valid Cloudflare Access token. The token must be
  RS256, with a key ID, the right issuer and audience, expiry and not-before; duplicate
  headers are refused; keys are fetched over HTTPS with no redirects, a size cap and
  a refresh limit. (`companion/src/auth.rs`.)
- Device API: a per-device bearer token, stored only as a hash.
- Fetching a web page for an article: private, loopback, link-local and metadata
  addresses are refused when the name is resolved, so redirects and DNS tricks cannot
  reach them. Tested, including `127.0.0.1` and `169.254.169.254`
  (`companion/src/link.rs`).
- The insecure test mode needs loopback listeners and refuses to start otherwise.

**Supply chain:** `cargo deny check` passes (advisories, licences, bans, sources;
`deny.toml`). Only crates.io is allowed. No git dependencies. The firmware's 148
dependencies are all permissive-licensed.

## Internet route (ADR 026)

Away from home the X3 talks TLS to a public hostname behind Cloudflare Access. It does not
check the certificate, so the connection alone proves nothing. Requests and replies are
signed with a key derived from the device token, which is never sent. A person on the same
Wi-Fi can read what is fetched, but cannot forge a job list, change a payload, or learn the
token. Cloudflare Access (a service token, no bypass) keeps scanners away from the companion.
Limits: no confidentiality against an active attacker; a stolen companion database could
forge requests; nonces are forgotten at restart; the service token and device token sit on
the SD card unencrypted, like the Wi-Fi password.

## Wi-Fi setup hotspot (ADR 025)

While you run Wi-Fi setup, the X3 is an access point and serves one page on
192.168.4.1. It is WPA2 with a random 10-character password shown only on the screen,
one client at a time, off after 5 minutes or on Back. Requests are capped (1 KiB head,
256 B body), one per connection, and strictly parsed; the page escapes every name.
Someone who can read the screen can join it. Passwords typed on the page are not
printed to the log. They are stored unencrypted on the card, as before.

## Not covered

- No fuzzing run. Parsers have hostile-input tests, not a fuzzer.
- No penetration test of the deployed Cloudflare and Caddy setup.
- No review of the ESP32 Wi-Fi blobs themselves.
- Real SD-card behaviour when power is cut mid-write is untested.

## Reporting a problem

Open an issue on the project page and say what you saw. For something sensitive, do
not paste your `LOG.TXT`, `WIFI.CFG`, `SERVER.CFG` or a flash dump.
