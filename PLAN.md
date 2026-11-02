# PLAN.md

Main goal: custom Rust firmware for the Xteink X3 that works as (1) a Magic:
The Gathering token display and (2) an offline article reader. The owner submits
content from an iPhone or browser to a self-hosted companion; when Receive is
pressed, the X3 pulls it over home Wi-Fi. The project should be publishable.

**Current action (2026-10-27): Phases 0, 2 and 4 done; Phase 3 done except the owner's 50-turn ghosting check. Phase 4B done. Next: Phase 5 tokens, then Phase 6B device GUI (MEMORY.md Next steps).** Phases 1, 1B and
1C (agent part) are done; the companion runs on home-server. Stock flash
backup and slot layout are verified (ADR 017 amended: our firmware lives in
app1, CrossPoint in app0).

Sources of truth: docs/PROTOCOL.md (contracts), docs/INFRASTRUCTURE.md
(deployment), docs/DECISIONS.md (decisions), docs/TESTING.md (tests).

Status key: `[ ]` not started, `[~]` in progress, `[x]` done with evidence in `MEMORY.md`.
**Who**: `Agent` runs it on the host; `Owner` runs it on the device and pastes the output.

## Product definition

**Boot.** Home screen with two entries: Tokens and Library, plus Receive. Deep
sleep when idle; wake on a button.

**Token mode.** The owner searches Scryfall in the companion UI, previews the
exact device pixels and queues the token, optionally marked "show". The X3 shows
the last "show" token after Receive. The panel keeps the image while asleep.
The last 20 tokens are cached on SD and can be recalled with buttons.

**Reader mode.** The owner shares an article (or pastes an existing email
summary) from the iPhone. The companion stores it durably even while the X3
sleeps. Receive copies it to SD. Receiving never interrupts the current page.
Library, offline reading, remembered position. Polish text renders correctly
(ą ć ę ł ń ó ś ź ż).

**Transport.** Wi-Fi only during a Receive session (max 180 s by default). The
X3 is an HTTP client that pulls from the companion's LAN listener with a device
token. Phone and browser reach the companion through Cloudflare Tunnel, Access
and Caddy on home-server. Details: PROTOCOL.md, INFRASTRUCTURE.md.

**Non-goals (v0.1).** NFC drivers, Bluetooth, device HTTP server, VPN on the MCU,
Tailscale, n8n in the submission path, mDNS, TLS on the device, EPUB, custom
image upload, native iOS app, mailbox access,
network wake from deep sleep.

## Phone and desktop workflow

| Job | v1 path |
| --- | --- |
| Send a token | Companion web UI: search, exact preview, queue (show/store) |
| Send a Safari article | Share Sheet Shortcut → `POST /api/v1/articles` with Access service token |
| Send an existing summary | Same Shortcut or a second one with kind `summary` |
| See status | Companion UI: accepted, preparing, ready, sending, delivered, displayed |
| Receive on the X3 | Press Receive; X3 pulls, commits, acks, Wi-Fi off |
| Fast launch | Optional NFC sticker triggers a Shortcut (phone only) |

## Review gate (done)

- [x] Document the companion direction and infrastructure.
- [x] Owner and Claude review (ADR 008).
- [x] Host chosen: home-server; LAN address reserved at 192.168.1.20 (owner, 2026-10-22).
- [x] Firmware base: independent (ADR 009). Stack ADR 003 approved (ADR 009).
- [x] Protocol v2 approved (PROTOCOL.md, ADR 009).

## Phase 0. Recon and safety (no device writes)

Tasks
- [x] Prior art, pins, licences and paper RAM budget in docs/HARDWARE.md.
- [x] Fork-or-fresh decision (ADR 001, ADR 009), licence (ADR 002), stack (ADR 003, ADR 009).
- [x] docs/TESTING.md and docs/BACKUP.md.

Owner steps (Agent prints exact commands; see docs/BACKUP.md)
- [x] Charger available; unit works (owner, 2026-10-22).
- [x] Unit accepts custom firmware: it already runs CrossPoint (owner, 2026-10-22).
- [x] Two full 16 MB flash reads of the current CrossPoint image, matching SHA-256
  (2026-10-26, MEMORY.md Evidence log).
- [x] Panel controller identity: **UC8253** (CrossPoint 1.6.5 About screen,
  owner photo, 2026-10-24).
- [x] Data-capable cable for the X3: 4-pin cable works (`/dev/ttyACM0`, 2026-10-26).
- [x] Agent: read-only dump inspector `x3-tools flash-info` (2026-10-24).
- [x] Partition table, slot contents and active slot from the dump (HARDWARE.md,
  2026-10-26). Does not match the source candidate; ADR 017 amended and accepted.
- [x] Restore path decided: ADR 017 accepted (owner, 2026-10-24): CrossPoint
  stays in its slot, ours goes in the other, "Back to CrossPoint" action, USB
  restore as fallback. The dump must confirm the slot layout.

Exit criteria: two hashes match and are in MEMORY.md; controller identified;
partition table read from the dump and consistent with ADR 017.

## Phase 1. Host-first core and contracts (no hardware)

Tasks
- [x] Cargo workspace; `fw` stub compiles for `riscv32imc-unknown-none-elf`.
- [x] `core`: bitmap pack/unpack, dithering, pagination, cache index, settings.
- [x] `sim`: PNG rendering with golden tests. CI config.
- [x] `proto`: protocol v2 — hello/jobs/ack line parsers and writers, `tk1` and
  `ar1` header encode/decode with CRC, bounded HTTP/1.1 response-head parser and
  request writer for the device client, `.cfg` file parser.
- [x] `core`: streaming staging/commit storage (incremental SHA-256 and CRC),
  metadata generations, index rebuild from validated payloads, item → file
  mapping, replacement resets position.
- [x] Disk-image tests: run the storage layer on embedded-sdmmc over an in-memory
  block device with fault injection (torn and failed writes at every step).
- [x] `tools`: aspect-preserving token encoder (`contain`/`cover`, orientation)
  replacing `resize_exact`; article encoder from a text file.

Tests (**Agent**)

| Area | Test |
| --- | --- |
| Protocol | Round-trip; truncated, oversized, overlong line, bad magic/version/CRC/hash, hostile bytes, wrong field counts all rejected |
| HTTP client | Status/header limits, missing Content-Length, chunked, 3xx, body overrun all rejected |
| Bitmap / text | Existing goldens; portrait token; Polish; long words; empty input |
| Storage | Power cut at each step leaves old or new state, never a valid-looking partial item; rebuild works |
| Paths | Item ID validation; traversal rejected |

Exit: CI green; ≥80 % line coverage on `proto` and storage code (measured);
sample token and article render to PNG.

Evidence so far (2026-10-21): 20 host tests pass; core coverage 98.12 %, cache
98.55 % (old interface). Proto empty. Re-checked 2026-10-22: still green.

## Phase 1B. Companion and simulated delivery (host only)

Tasks
- [x] `companion/` crate, binary `x3-companion` (ADR 009 stack, pinned).
- [x] SQLite schema: devices, submissions, jobs, payload blobs, state history.
- [x] Public API per PROTOCOL.md B, with Access JWT verification and a loopback-only dev mode.
- [x] Device API per PROTOCOL.md A, with leases and idempotent acks.
- [x] Preparation worker: articles → `ar1`; Scryfall → `tk1` via x3-core; previews.
- [x] Scryfall client: allowlisted hosts, required headers, cache, throttle.
- [x] Minimal web UI (target < 100 KB): token search/preview/queue, article
  paste, job list, cancel. Same-origin API.
- [x] Article from a link (ADR 015) and page rebuilt in Field Journal (2026-10-24).
- [x] CLI: `serve`, `device add`, `backup`, `restore-check`.
- [x] `sim receive`: plays the X3 — pulls from a running companion using `proto`
  and the core storage on a disk image, then renders PNGs.
- [x] Dockerfile.

Exit: INFRASTRUCTURE.md section 8 "1B" tests pass; a queued article and token
render after simulated Receive; evidence in MEMORY.md.

## Phase 1C. Deployment on home-server (owner reviews before applying)

Tasks
- [x] Agent prepares `infra/` examples (compose, Caddy block, README steps).
- [x] Owner: Tunnel hostname reader.example.org → 192.168.1.20:80 (2026-10-22).
- [x] Owner: Access application, Allow policy, team domain and AUD supplied (2026-10-22).
- [x] Owner: Shortcut service token: not needed (Shortcuts dropped, 2026-11-02).
- [x] Owner approved; agent deployed 2026-10-24 (container, data dir, Caddy block).
- [x] Access denial (incl. LAN port 80, forged headers), `/device/*` blocked
  publicly, device token scoped, `sim receive` against the live listener (empty).
- [ ] Owner: browser login and a real article/token, then `sim receive` renders it;
  Shortcut over HTTPS; restore drill; off-host backup of `backups/`.
- [x] iPhone Shortcuts: dropped by the owner (2026-11-02); the web page is enough. Guide stays.

Exit: owner-observed HTTPS submissions land in the deployed companion; a LAN
`sim receive` pulls them; rollback tested. No real X3 transfer claimed.

## Phase 2. Hardware bring-up (human-in-the-loop)

Gate: Phase 0 exit (backups + controller). Each step is flashed by the owner. A
`hwtest` firmware prints a table to serial; the owner pastes it back. Restore
path: the owner's backup, or reinstall CrossPoint with its web flasher.

| Step | Test | Who | Pass |
| --- | --- | --- | --- |
| 2.0 | Agent: ADR 003 crates build and link together (host) | Agent | **Passed 2026-10-22:** release ELF links; sizes in MEMORY.md |
| 2.1 | Boot, print "hello" and chip info on serial (flashed to the free OTA slot only, ADR 017) | Owner | **Passed 2026-10-26:** hello lines on serial; otadata restore boots CrossPoint intact |
| 2.1b | "Back to CrossPoint": switch otadata to CrossPoint's slot and restart | Owner | **Passed 2026-10-26:** `crosspoint` serial command; CrossPoint booted (log on serial) |
| 2.2 | Display: full white, black, checkerboard, then partial refresh | Owner | **Passed 2026-10-27:** clean full frames (1.7–1.9 s), fast 457 ms/step, slight ghost cleared by full refresh; orientation in HARDWARE.md |
| 2.3 | Buttons: every button reports once per press | Owner | **Passed 2026-10-27:** 7 inputs mapped (HARDWARE.md); `keys`: 21 presses, one per press, long hold = 1, double-tap ~240 ms = 2 |
| 2.4 | SD: mount, list, 100 MB write/read with checksum, remove and reinsert | Owner | **Passed 2026-10-27:** 100 MiB written and read back, 0 bytes differ (181/495 KiB/s); removal gives a clean error (slow: ~70 s, fix retries); reinsert lists again |
| 2.5 | RTC DS3231: read, set, survive power cycle | Owner | **Passed 2026-10-27:** I²C finds 0x55/0x68/0x6B; RTC is UTC; set and read back; after shutdown within ~1 s, OSF clear |
| 2.6 | Battery gauge and charge state | Owner | **Passed 2026-10-27:** 4.348 V, 97% (620/642 mAh), +160/+165 mA charging, 29.6 °C, health 100% |
| 2.7 | Motion sensor (optional) | Owner | **Passed 2026-10-27:** live after soft reset; flat −Z, portrait −X, landscape (buttons right) −Y |
| 2.8 | Deep sleep and button wake; current if a meter exists | Owner | **Passed 2026-10-27:** wake by power button (`CoreDeepSleep/Gpio`) and by 120 s timer (`CoreDeepSleep/Timer`); boots back into app1; no current meter |

## Phase 3. Local reader MVP

Render `ar1` articles from SD in **portrait (528×792) as default**, the way the
owner reads in CrossPoint (landscape optional), with a proportional font,
justified text and hyphenation; Polish font that also
covers typographic punctuation (’ ‘ “ ” „ – — … and no-break spaces: web
articles use them everywhere; the Latin-2 simulator font maps them); page-turn
buttons; remembered position; partial refresh with a full refresh every N pages.
Agent: golden frames. Owner: page-turn latency (yardstick ~424 ms), ghosting
after 50 turns, Polish sample, position survives power cycle.

Status 2026-10-27: golden frame **done** (`sim` `reader_golden`); latency
**passed** (~411–426 ms); Polish sample **passed** (photos); position across
a power cycle **passed** (4949 → 4949). Ghosting after 50 turns: open.
Fonts: Literata 24/28 px (owner). Library: `/X3/DATA`, newest first.

## Phase 4. Wi-Fi and Receive client

Join Wi-Fi from `/x3/wifi.cfg`; Receive session per PROTOCOL.md A against
`/x3/server.cfg`; timeout; RAM high-water logging.
Agent [x]: firmware client logic against the companion on localhost (host build of
the session state machine). Owner [x]: Receive from the deployed companion; 10
sessions; Wi-Fi off afterwards; battery drop per session.
Exit: all pass; RAM headroom with Wi-Fi up ≥ 40 KB (else ADR).
**[x] Done 2026-10-27** (evidence in MEMORY.md): 23+ device sessions, no
errors; 4 articles and a token downloaded; stack ≥ 99 KB untouched; Wi-Fi
off after each; ~40 µAh per empty Receive, 96 µAh with 4 articles. Also: RTC
set from `hello`, Confirm starts a Receive, `/X3/LOG.TXT`. Found: the idle
loop draws ~30–60 mA (next work item).

## Phase 4B. Idle power

Light sleep between button polls (timer wake ~50 ms, ADC ladders read on each
wake; Back/Confirm/Left never pull a GPIO low, so level wake cannot cover
them), only while no USB host is connected (USB SOF counter), so serial
bring-up is unchanged. Deep sleep after a long idle, power-button wake.
Owner: unplugged idle current from the gauge (logged to `/X3/LOG.TXT`), every
button still wakes and responds, deep sleep and wake. Exit: idle draw at
least 10× below the 30–60 mA measured in Phase 4; no missed presses.
**[x] Done 2026-10-27** (MEMORY.md): idle ~3–4 mA on battery (from 30–60),
deep sleep after 10 min with power-button wake, USB back after replug,
image stays crisp. Panel controller deep sleep while running turned the
image grey and was removed.

## Phase 5. Token mode

Display `tk1` (landscape and portrait) in four greys (ADR 018: grey waveform,
full refresh, two 1-bit planes from SD; 1-bit fallback), show-after-Receive, `displayed` ack,
last-20 cache, button recall. Owner: queue 5 tokens, Receive, power cycle, recall
offline, legible at table distance; grey refresh time and ghosting measured.

**Status 2026-10-27:** four-grey display, show-after-Receive, `displayed`
ack (persisted in `/X3/SHOWN.CFG`), last-20 cache and recall (`tokens`
command) verified on the device. Button recall without serial moves to the
device GUI (6B). Open: legibility at table distance (owner), grey draw time
(~6.7 s; could be faster).

## Phase 6. Articles and offline cache

**Status 2026-10-31:** garbage collection (device-verified), article eviction
(ADR 023, **device-verified** with a temporary cap of 3, all three rules),
read state in the library (device-verified) and an exhaustive host power-cut
sweep (3,162 cuts) are done, and the device drill (5 cuts mid-download,
reset with the card powered) passed. **Phase 6 is complete (2026-11-01):** two summaries were received,
verified and listed on the device; the owner waived a physical power pull (the
reset drill kept the card powered, so real torn-sector behaviour is accepted as
unverified).

**Earlier status 2026-10-30:** garbage collection of superseded and abandoned
generations built and host-tested; device check pending. Article eviction,
article eviction built (ADR 023, host-tested, flashed). Read state and the
power-cut drill are open.

Library, read state, position, delete jobs, replacement, eviction. Owner: 5
articles + 2 summaries, read with Wi-Fi off; cut power mid-Receive 5 times — no
corrupt entry, library opens; fill past the cap and confirm eviction.

## Phase 6B. Device GUI

**Status 2026-11-02 (end):** pictures (ADR 028) beside tokens, verified; faster
screen changes; Storage action row; last Receive on the Home tile; sleep picture
chosen in Pictures. F1 done (2026-11-02, later): screens in `x3_core::screens`,
shared by firmware and simulator; F3 one sha2 (−16.9 KB); neither flashed yet. F2 (serial `library` panic) open.

**Status 2026-11-02:** presses kept during refreshes; clean second pass for
menu moves; Clean screen; sleep screen with standby on USB and deep sleep on
battery (all verified by the owner). Audit done (docs/AUDIT-2026-11.md); its
follow-ups are open.

**Status 2026-11-01:** Device is a menu (Wi-Fi choice with a join check,
Storage with delete-everything, About, Back to CrossPoint); Home notices the
card coming and going. Verified on the device by the owner, except the
delete-everything run. The internet route (ADR 026) is verified on a phone
hotspot.

**Status 2026-10-31:** Home, Library, Device and the button bar are on the
device and verified by the owner (Home loop in `fw/src/home.rs`). Open: a
Receive progress screen, the Tokens list screen (built 2026-11-01, names from the item ID;
device check open),
and the owner decisions on an inbox listing and on-device token queueing.

An on-device interface in the spirit of the companion web portal, in black,
white and greys, driven by the 7 buttons (Up/Down move, Confirm opens, Back
returns, Left/Right page). Screens: Home (Library, Tokens, Receive, Device);
Library (articles, read state, progress, delete); Tokens (last 20, show one in
four greys); Receive (item-by-item progress); Device (Wi-Fi, battery, storage,
last Receive, firmware, log). Layout from the portal: headings, cards, a
highlighted row for the selection, partial refresh to move it, full refresh
between screens. Visual language: the owner's **Field Journal** design system
(design library), which the web portal uses: ivory paper, Literata serif,
moss-ink accent. On the panel: paper = white, ink = black, the moss accent
becomes black or dark grey; follow its type scale, spacing and components.
Literata only (General Sans may not be redistributed).
Screens live in `x3-core`, host-tested and rendered by the simulator to PNG for
owner review before any flash. Typing is out of scope: links and pasted text
stay on the phone and portal.
Open (owner decision, protocol change, ADR): (a) an inbox listing of what is
waiting on the companion before Receive; (b) queueing a token from a
companion-supplied list of favourites or recent tokens.
Owner: review sim PNGs, then use every screen on the device.

## Phase 7. Phone side polish

**Status 2026-11-02:** portal reworked (ADR 029): inbox of 10 with Show older, saved tokens
and pictures with Send again, tokens-only ranked search with both faces, UI review fixes.
Deployed 2026-11-02 (image 8b828a0c43b6, schema v5); owner verified on laptop and phone.

Finish UI and Shortcuts; verify Scryfall rules and image terms; screenshots and
user docs. Owner: ten clean runs of each flow including save-while-offline then
Receive. Agent: browser test of the UI against `sim receive`.

## Phase 8. Hardening and release

**Status 2026-11-02 (evening):** audit F1/F2/F3 closed on the device; every screen walked
(owner); ADR 030 companion deployed; Scryfall greys accepted as a personal display.

**Status 2026-11-01:** long articles open (ADR 027, host-tested; device check open). Audits (deny, audit, licences), security self-review, install
and user guides, README, CI supply-chain job, public-export script and a 45 min soak
are done. Open: 7-day battery run, restore drill, 24 h soak (all owner/time), a
second unit, tag `v0.1.0` and publication (owner). See docs/RELEASE_CHECKLIST.md.

Partition layout compatible with CrossPoint and stock; web-flasher or manual
install path (OTA only if brick-safe); security review; `cargo audit`, `cargo
deny`; licence audit; 7-day battery and 24 h soak; restore drill; README and
guides; companion as optional Docker image; tag `v0.1.0`.

**Added 2026-11-01 (ADR 025):** Wi-Fi setup from the phone (hotspot, QR, page) and up
to four saved networks; verified on the owner's unit except the cases listed in MEMORY.md.

## Later ideas (not in v0.1)

Device web server for owners without a companion; n8n digests and
notifications calling the companion API; life/counter overlays; tilt to rotate;
BLE page turner; timed sync; delivery away from home.
