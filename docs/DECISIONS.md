# Decisions

## ADR 001 — Fresh Rust implementation (accepted for host work, 2026-10-21)

Context: X3 support and restore safety matter more than sharing another reader's UI.
Options: fork MarigoldOS; port CrossPoint; start fresh with hardware references.
Choice: start fresh. The host code is original. No upstream firmware code or LUT
bytes have been copied. Research findings are facts and source links.
Consequences: more bring-up work; no inherited flash scripts or partition changes.
MarigoldOS, CrossPoint, CrossPlant and CrossInk have MIT root licences at the
revisions in HARDWARE.md. Preserve notices for any later copied code. Submodules,
fonts and vendored code may have separate licences. Rustmix has no root licence
in the inspected tree; TrustyReader returned 403. Neither may be copied.

## ADR 002 — Project licence (accepted, 2026-10-21)

Context: publish original firmware and allow reuse.
Options: MIT; Apache-2.0; dual MIT OR Apache-2.0.
Choice: dual MIT OR Apache-2.0 for original contributions. Licence texts at root.
Consequences: users can choose either. Dependencies retain their own licences;
THIRD_PARTY.md records them. No card art or data dumps are distributed. This is
an unofficial fan project with no association with Wizards of the Coast.

## ADR 003 — Device stack (approved 2026-10-22, amended by ADR 009)

Context: ESP32-C3 has no PSRAM; networking and display share limited RAM.
Options: esp-idf/std; blocking esp-hal; esp-hal with Embassy.
Choice proposed: esp-hal 1.2.2 (esp32c3), Embassy executor 0.10.0, esp-radio
0.18.0 (Wi-Fi), embassy-net 0.9.1 (IPv4/DHCP/TCP), picoserve 0.20.1 for HTTP (dropped by ADR 009),
embedded-sdmmc 0.10.0 for FAT. Serialise SD/display on shared SPI. Bounded
buffers, one active upload, no TLS on device. Use core traits for all hardware.
The executor yields during long display waits. Do not enable BLE.

These are current stable versions returned by the crates.io API on 2026-10-21:
[esp-hal](https://crates.io/api/v1/crates/esp-hal),
[esp-radio](https://crates.io/api/v1/crates/esp-radio),
[embassy-executor](https://crates.io/api/v1/crates/embassy-executor),
[embassy-net](https://crates.io/api/v1/crates/embassy-net),
[picoserve](https://crates.io/api/v1/crates/picoserve),
[embedded-sdmmc](https://crates.io/api/v1/crates/embedded-sdmmc).
ESP32-C3 support is documented by [esp-hal](https://docs.rs/esp-hal/1.2.2/esp_hal/)
and [esp-radio](https://docs.rs/esp-radio/0.18.0/esp_radio/).
These independently current releases are **not a verified compatible bundle**.
Check their HAL, scheduler, time-driver and executor requirements together before
adding device dependencies. No HAL or network crate is in the stub today.

Host choice implemented: Rust 1.99.0 (already available on this Linux host,
then explicitly installed and pinned), image 0.25.10 (PNG/JPEG only) and
embedded-graphics 0.8.2. Both crate versions checked with crates.io; exact pins
and Cargo.lock are committed. Core/proto have no dependencies or allocation.
Use the public-domain X.Org fixed 10×20 font, Latin-2 subset supplied by
embedded-graphics, for Polish text previews. The layout uses Unicode scalar
columns with UTF-8 byte reading positions. Emoji falls back to the font's missing
glyph. Proportional shaping and combining-mark normalisation are outside v1.

Consequences: image decoding and full-size greyscale buffers are host-only.
Device fonts/renderer can reuse embedded-graphics, but no hardware claim follows
from PNG output. The target check validates the stub only. New firmware stack
versions and static memory sizes need a real link and runtime measurements.

## ADR 004 — Partition safety and display uncertainty (accepted constraint)

Context: current upstream X3 drivers conflict with the initial SSD1677 assumption;
stock compatibility depends on the owner's actual flash.
Choice: no partition table, bootloader, linker script, LUT or driver is added.
Record candidate tables and controller variants in HARDWARE.md. Compare the
owner's backup and obtain approval before proposing any device writes.
Consequences: `fw` is compile-only and deliberately has no entry point or driver.
Phase 2 remains out of scope. No restore command is supplied before image and
layout validation. Backup reads are human-run only.

## ADR 005 — Reuse Caddy and Cloudflare for optional companion access (amended by ADR 008)

Date: 2026-10-22. Status: accepted, amended by ADR 008 (n8n leaves the v1
submission path). This does not approve ADR 003 or wire bytes.

Context: the owner has an always-on home-LAN machine, self-hosted n8n, Caddy and
Cloudflare Tunnels. Content should be accepted while the X3 sleeps.
Options: direct device push only; Tailscale Serve plus a companion; reuse the
existing remote ingress and automation with a small companion.
Choice for the revised plan: the third option. n8n receives authenticated
submissions and handles optional automations. Caddy routes the intended surfaces.
Cloudflare Tunnel carries remote ingress with explicit user/machine auth. The
companion owns persistent content, conversion and delivery. Tailscale remains an
optional private access/admin path. The X3 receives over LAN only during Receive.
Consequences: most server infrastructure is reused; existing services remain
independent. Remote submission can outlive the phone session and device sleep.
The companion is optional for published firmware; direct LAN operation remains.
Host placement, origin routes, auth policy and versions must be inventoried.
Setup, primary sources and acceptance tests: INFRASTRUCTURE.md.

## ADR 006 — Durable delivery and streaming contracts (accepted; see ADR 008–009)

Date: 2026-10-22. Status: accepted in principle, amended by ADR 008 (pull
delivery, SQLite blobs); not wire-protocol approval.

Context: sleeping readers, workflow retries and lost upload acknowledgements must
not lose or duplicate accepted content. Whole JSON/payload copies strain RAM.
Options: retain n8n execution data and retry raw pushes; build an independent
persistent inbox and use idempotent streamed device commits.
Proposed choice: independent companion database and files; stable submission and
content identities; per-device serial delivery with bounded retry and restart
recovery; explicit accepted, delivered and displayed meanings. n8n returns accepted
only after companion persistence, not merely after starting a workflow. Proposed
single-instance storage: SQLite on local persistent disk plus content files;
existing PostgreSQL is an alternative to resolve at review.
Consequences: n8n history/pruning is no longer a data-retention mechanism. The
companion needs its own quota, backup and restore policy. Device storage needs a
reviewed streaming commit/recovery model rather than assuming FAT atomic rename.
PROTOCOL.md remains a historical first draft pending changes. Agree companion
submission and device contracts separately before implementing either.

## ADR 007 — Firmware foundation comparison (closed by ADR 009)

Date: 2026-10-22. Status: closed by ADR 009 — keep the independent firmware.

Context: chongfun/MarigoldOS now presents itself as CalendulaOS and documents
existing Rust X3 reader, Wi-Fi and recovery features. The initial independent
host core remains useful, but duplicating all peripheral/recovery work may not
serve the product goal. Newer-panel compatibility is not assumed.
Options: continue ADR 001's independent firmware; extend a pinned compatible
upstream; reuse selected licensed components.
Proposed next step: compare scope, exact panel support, tested memory, SD recovery,
API extension points, licences and ongoing maintenance before selecting a base.
Consequences: ADR 001 still governs current original host code. No upstream code,
fonts, LUTs or flashing method are imported by this proposal. A chosen base needs
recorded notices, an ADR update and the same backup/partition/hardware gates.

## ADR 008 — Review outcome: pull delivery, Rust companion, home-server (accepted)

Date: 2026-10-22. Status: accepted by the owner in review with Claude Code.
Amends ADR 005 and ADR 006. Does not approve wire bytes, ADR 003 or ADR 007.

Context: the drafted design had the companion push to a device HTTP server,
an n8n intake hop, SQLite plus separate content files, and an unchosen runtime.
The companion cannot know when a deep-sleeping X3 is awake.
Choices:
- **Delivery: the X3 pulls.** Pressing Receive joins Wi-Fi, calls the companion's
  LAN listener, lists pending jobs, streams each one to SD, commits, acks, and
  turns Wi-Fi off when the queue is empty. A lost ack replays the same job ID;
  the device recognises committed content and acks again. The device HTTP
  server for direct-LAN use becomes a later fallback, not v1.
- **n8n: later only.** Shortcut and browser call the companion API through
  Cloudflare Access (service token for machines, login for browsers). n8n may
  later call the same API for digests or notifications.
- **Companion: Rust (axum + rusqlite), reusing x3-core** for dithering, layout and
  previews. Content and metadata in one SQLite database; payloads are blobs.
  Backup with `VACUUM INTO` or the online backup API. Do not share n8n's database.
- **Host: home-server**, which is on the X3's home LAN and already runs n8n,
  Caddy and cloudflared. public-vps is a public VPS with no LAN path to the X3.
- Tailscale is not needed in v1.
Consequences: the device needs an HTTP client, a stored companion address and a
device credential instead of a v1 server. Shorter radio time. No file/database
reconciliation. Two companion listeners: public (via Caddy/Tunnel/Access) and
LAN-only device API. The origin is LAN-reachable, so the companion verifies the
Access JWT or uses its own auth; it never trusts identity headers alone.
home-server needs a DHCP reservation. Exact routes, versions and pins: Phase 1C.

## ADR 009 — Owner confirms all open decisions; implementation starts

Date: 2026-10-22. Status: accepted. The owner asked Claude Code to confirm every
open decision so Codex can start programming. Supersedes conflicting text in
ADRs 003, 005, 006 and 007 and in older docs.

Hardware context (owner report, 2026-10-22): charger present; the unit works,
runs CrossPoint and is on the home Wi-Fi. It therefore accepts custom firmware.
The stock image is no longer on the device.

Decisions:
1. **Protocol v2 approved** as written in PROTOCOL.md: pull-based device API with
   line-based text bodies, `tk1` and `ar1` payload formats, ULID jobs, logical
   item IDs, SHA-256 identity and idempotent acks; companion JSON API behind
   Cloudflare Access with client `submission_id` idempotency.
2. **Firmware base: independent Rust firmware (ADR 001 stands).** The pull design
   differs from upstream push servers, and the host core already exists.
   CrossPoint, CrossPlant and CalendulaOS (all MIT) stay references; copying a
   specific component (for example a panel init sequence) is allowed only with
   its notice recorded in THIRD_PARTY.md first.
3. **Device stack approved:** esp-hal 1.2.2, esp-radio 0.18.0, embassy-executor
   0.10.0, embassy-net 0.9.1, embedded-sdmmc 0.10.0. **picoserve is dropped**:
   v1 has no device HTTP server. The HTTP/1.1 client and line parsers are our
   own bounded code in `proto`, host-tested. First firmware task: prove these
   crates build and link together for `riscv32imc-unknown-none-elf`. If they do
   not, the agent may move to the nearest compatible releases of the same crates
   without new approval, and records the set here. Changing crate families needs
   the owner.
4. **Companion:** Rust workspace member `companion/` (binary `x3-companion`):
   tokio, axum, rusqlite (bundled SQLite), image, x3-core and x3-proto. Payloads
   are SQLite blobs. JWT verification for Access with a maintained crate. Pin
   exact versions after checking crates.io and record them in THIRD_PARTY.md.
   Packaged as one Docker image; deployed on home-server next to existing
   Caddy and cloudflared. Public listener on the Docker network only; device
   listener on the server's reserved LAN address.
5. **Defaults** (configurable): companion storage quota 512 MiB; delivered items
   kept 30 days, pending never pruned; nightly `VACUUM INTO` backup, 7 kept, in a
   host directory the owner's existing backup collects; Receive session 180 s;
   token cache 20; article text cap 256 KiB.
6. **v1 non-goals confirmed:** n8n in the submission path, Tailscale, device HTTP
   server, custom image upload, mDNS, TLS on the device, 2-bit tokens on the
   panel, EPUB. Each may return later via an ADR.
7. **Still gated on the owner:** two matching flash backups of the current
   (CrossPoint) image and the panel controller identity before any flashing or
   driver work (Phase 2). Deployment changes on home-server need the owner to
   review the concrete diff (Phase 1C).

Consequences: Codex may implement Phases 1, 1B and prepare 1C now, following
CODEX_PROMPT.md. Hardware rules in AGENTS.md are unchanged.

### Link-proven device set (2026-10-22)

The crates.io manifests for esp-radio 0.18.0 and esp-rtos 0.3.0 require
`esp-hal ~1.1.0-rc.0`, which excludes the proposed 1.2.2. Under the permission
above, pin **esp-hal 1.1.2**, **esp-radio 0.18.0**, **esp-rtos 0.3.0**,
**esp-alloc 0.10.0**, **embassy-executor 0.10.0**, **embassy-net 0.9.1** and
**embedded-sdmmc 0.10.0**. All use ESP32-C3 features where applicable.
RTOS and allocator supply the radio scheduler, Embassy time driver and heap.
Official release manifests: [HAL](https://crates.io/crates/esp-hal/1.1.2),
[radio](https://crates.io/crates/esp-radio/0.18.0),
[RTOS](https://crates.io/crates/esp-rtos/0.3.0).

`fw` uses the HAL-provided `linkall.x`; no partition or bootloader file changes.
The default `link-proof` feature retains a never-called integration function:
radio creation, DHCP stack creation, FAT volume parsing over synthetic blocks,
and the RTOS-backed Embassy executor. The entry point only retains its address
and spins. Heap use is behind that feature; its 100 KiB reservation is a build
fixture, not a measured budget or a final firmware allocation decision.

Phase 2.1 (2026-10-26) adds **esp-bootloader-esp-idf 0.5.0**, the newest
release that accepts `esp-hal ~1.1` (0.6.0 needs 1.2). It supplies the app
descriptor that CrossPoint's ESP-IDF bootloader reads. The app image is made
with `esptool elf2image` (esptool 5.6.0); no espflash is needed.
Phase 2.1b adds **esp-storage 0.9.0** (the last release for `esp-hal ~1.1`)
and **embedded-storage 0.3.2** for flash access. otadata logic lives in
`x3-core::otadata`, host-tested and shared with `x3-tools`; the bootloader
crate only reads the partition table (with its MD5 check).
Release linking succeeds on Rust 1.99.0. Symbol inspection confirms radio init,
FAT volume opening and Embassy executor polling remain in the ELF. This proves
link compatibility, not Wi-Fi operation, SD access, runtime RAM or bootability
with the owner's installed bootloader. Hardware gates remain in force.

## ADR 010 — Device text line cap raised to 192 bytes

Date: 2026-10-22. Status: accepted by the owner. Amends PROTOCOL.md A.1.

Context: Codex found that valid job lines exceed the 160-byte cap. A token job
with a 48-byte item ID, length 52288, a 64-digit hash and `show` is 161 bytes
without the LF; the largest article job with `-` is also 161.
Options: raise the line cap; shorten the item ID cap.
Choice: line cap 192 bytes excluding the LF. The `hello` request body keeps its
160-byte cap (longest valid line 84 bytes). The jobs body cap stays 4,096 bytes
(16 lines of 192 plus header and `end` fit).
Consequences: device line buffer of 193 bytes. Tests must cover a 192-byte line
(accepted) and a 193-byte line (rejected), plus the 161-byte worst-case job lines.

## ADR 011 — Immutable storage generations (implementation choice)

Status: implemented on the host under the approved C.3 storage contract.
Each new payload gets an exclusive eight-hex-digit `.DAT` filename. A separate
112-byte checksummed `.MET` record maps the full logical item ID, payload SHA-256,
length and reading position to a payload generation. Commit persists and rereads
both before success. Replacement starts at position zero. Position updates and
deletes append metadata generations; tombstones prevent old-item resurrection.
The current index can be reconstructed by scanning metadata and validating its
referenced payloads. Uncommitted payloads are ignored. Never reuse a generation,
including one left by a failed write. No atomic FAT rename is assumed.

Dependency versions checked using crates.io API: sha2 **0.11.1** (no default
features; **0.10.9** in x3-core and x3-proto since 2026-11-02, audit F3, to share
embedded-tls's copy on the device; companion and tools stay on 0.11.1) and embedded-sdmmc **0.10.0** (optional `fat` adapter feature). No
upstream code or disk image copied. Tests construct an original FAT16 image.
Bounded rereads validate CRC, SHA-256, UTF-8 and text controls without a whole
article buffer. Physical card synchronisation and controller behaviour remain
unverified. History and abandoned generations are retained; later garbage
collection must preserve the latest committed state and tombstones.

## ADR 012 — Companion contract corrections (accepted with amendments)

Status: **approved by the owner on 2026-10-22 with the amendments at the end of
this ADR**. PROTOCOL.md, INFRASTRUCTURE.md and TESTING.md are updated. Required before the companion can meet
all existing acceptance criteria. ADR 010's line cap is unaffected.

### Empty acknowledgements

Problem: PROTOCOL.md A.1 requires Content-Length on every response, but A.5
requires 204. HTTP forbids Content-Length on a 204 response, even when zero
([RFC 9110 section 8.6](https://httpwg.org/specs/rfc9110.html#field.content-length)).
204 has no body and ends at the end of the headers
([RFC 9112 section 6.3](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3)).
The current proto parser follows the approved document literally, so it will
reject a standards-compliant 204 without Content-Length. Its tests are not
interoperability evidence.

Proposed change: keep ack status 204; make it the explicit exception to A.1.
For 204 require no Content-Length, no Transfer-Encoding and no body bytes.
For other accepted response statuses retain the mandatory bounded Content-Length.
Add a regression test using an actual axum TCP response, not only constructed
header strings. Alternative: change successful acks to 200 with an empty body
and Content-Length: 0.

### Cancellation after delivery starts

Problem: INFRASTRUCTURE.md section 8 requires cancellation during `sending` to
return payload 404 and prevent any device commit. Counterexample: the companion
sends the last payload byte; the owner cancels; the device, which already has
all bytes, verifies and commits before its ack reaches the server. No existing
message can revoke those bytes or tell the device about the cancellation.
Even an expired lease does not establish that the device failed to commit.

Proposed change: cancellation succeeds only before delivery can begin. Persist
a `delivery_started` flag in the same transaction that first authorises a
payload response. Once set, cancellation returns 409, including after lease
expiry. For delete jobs, set it when the command is included in a jobs response,
as these jobs have no payload request. Before that point cancellation is
idempotent, removes the job from listings, and payload requests return 404.
A later removal uses the existing queued DELETE item operation. Replace the
impossible acceptance row with two race tests: cancellation wins before delivery
(no payload served), or delivery wins (cancel returns 409; ack remains valid).
This keeps the pull protocol and honest delivery status without a new commit
handshake.

### Preview before queueing

Problem: the approved UI flow previews exact pixels before queueing, but B.2 only
exposes `/api/v1/jobs/{job}/preview.png`. Creating that job already submits the
token, and a concurrently receiving device could download it before approval.

Proposed addition: `POST /api/v1/tokens/preview`, authenticated like the other
public routes. Body: `scryfall_id`, `face`, `fit`, `orientation`, with the same
validation and limits as token submission; unknown/duplicate fields rejected.
Response: 200 image/png, exact converted pixels. It creates no job or submission;
Scryfall fetch/cache/throttle and conversion are shared with token submission.
The UI enables queueing only after this preview succeeds. The existing job
preview route remains for already-submitted work. Alternative: introduce a
separate draft/confirm lifecycle, which would require more state and routes.

### Owner decision and amendments (2026-10-22, reviewed with Claude Code)

Approved, with these amendments:
1. 204: the device also accepts `Content-Length: 0` on a 204; a non-zero value,
   `Transfer-Encoding` or body bytes are errors. Keep the real-axum TCP test.
2. Cancellation: as proposed. Add a third race test: the device skips a download
   because it already holds the hash and acks a job cancelled meanwhile; the
   server returns 404 and the device treats it as done. The UI answers a 409
   with "Already sent. Use Remove from device instead".
3. Preview: use `GET /api/v1/tokens/preview.png` with query parameters (safe,
   no side effects, works as an `<img src>`). Return the payload SHA-256 in
   `X3-Payload-Sha256`; token submission accepts optional `expected_sha256` and
   returns 409 on mismatch. At most 2 concurrent conversions shared by preview
   and submission; overflow returns 503 `busy`.

## ADR 013 — Companion host implementation and pins

Date: 2026-10-22. Implements ADRs 009 and 012 without changing the wire format.
Versions checked on crates.io: axum 0.8.9, tokio 1.54.1, rusqlite 0.40.2
(bundled SQLite), jsonwebtoken 11.1.0 (RustCrypto), reqwest 0.13.5 (rustls),
serde 1.0.229, serde_json 1.0.154, uuid 1.28.0, ulid 3.0.0, getrandom 0.4.3,
base64 0.23.1. Test helpers: tower 0.5.3, http-body-util 0.1.5, rsa 0.9.10.
The existing image, core, proto, tools and simulator code supplies conversion.
Exact pins and transitive checksums are in Cargo.lock; licences in THIRD_PARTY.md.

One process owns one SQLite database and one registered reader (the public
contract has no device selector). WAL with synchronous FULL; acceptance, history
and canonical submission identity commit together. Blobs and original requests
share the jobs table. A mutex serialises database transactions; network requests
and conversions happen outside it. Pending payload space is reserved against the
quota; SQLite max_page_count also caps database pages. WAL, temporary conversions
and backups need extra disk space beyond that quota. Run one serve process only.

A worker retries interrupted preparations on server startup. Opening the database
from an admin CLI does not reset live work. Expected-hash token submissions
prepare before acceptance, then persist those exact bytes; other jobs prepare
in the background. Preview/submission conversion uses two shared permits with
a 250 ms wait. Scryfall fetches have bounded bodies, HTTPS host allowlists, no
redirects, a 150 ms request interval, bounded Retry-After and a 64 MiB/64-entry,
one-hour in-memory cache. Live image/API terms still need Phase 7 verification;
no external art or upstream fixture was copied into the repository.

Access verification requires RS256, a trusted JWKS key, issuer, audience and
expiry; nbf is checked when present. Unknown identity headers confer no access.
The developer bearer requires loopback listeners. Separate device tokens are
random 32-byte values stored only as SHA-256 hashes. Browser mutation requests
with an Origin from a different host are rejected. No content or credentials
are included in application error logs.

Daily backup uses VACUUM INTO, verifies the result, keeps seven and prunes
finished jobs older than 30 days (superseded by ADR 014). Backup failures are reported
without content and retried. Restore-check opens the backup read-only and checks
SQLite integrity, payload hashes and format validation. Production Access,
Caddy, Docker deployment and real hardware remain owner-reviewed stages.

## ADR 014 — Companion retention, retries and schema version 2

Date: 2026-10-23. Claude Code review of the Codex companion before deployment.

Context. Three defects found in review. (1) Retention deleted every delivered
job after 30 days. The item ID lived only in those rows, so "Remove from device"
and article replacement returned 404 for items still on the SD card. (2) Any
preparation error failed the job for good, including a Scryfall outage (502) or
the 250 ms conversion wait (503). (3) Failed and cancelled jobs reserved their
maximum payload against the quota forever and were never pruned.

Choice.
- Retention removes old failed, cancelled, delete and superseded delivered
  jobs. The newest delivered content job of each item stays as the item record,
  with its payload and request text cleared. Pending jobs are never pruned.
  A pruned payload returns 404, not 503.
- Only `accepted` and `preparing` jobs reserve payload space.
- 502 and 503 during preparation return the job to `accepted` with a retry time
  (30, 60, 120, 240 s); the fifth failure fails it. Other errors fail at once.
  A waiting retry does not block later jobs.
- Schema `user_version` 2 adds `attempts` and `retry_at`. Version 0 and 1
  databases migrate in one transaction on open.

Consequences. A pre-ADR-014 binary refuses a version 2 database
(`unsupported_database`). Nothing is deployed yet, so this costs nothing now;
after deployment, roll back by restoring a pre-upgrade backup. A replay of a
submission ID older than 30 days whose request text was cleared returns 409
instead of 200. Wire formats are unchanged. Tests: four new acceptance tests
(retry/backoff/limit, retention, quota release, migration and refusal).

## ADR 015 — Articles from a link; companion page in Field Journal

Date: 2026-10-24. Owner request: create an article from a link, and restyle
the companion page with the Field Journal system from the owner's design library.

Context. The companion shares `proxy-net` with n8n, Gitea, MinIO and Home
Assistant, and Home Assistant listens on the LAN. Fetching a user-supplied URL
from there is a server-side request forgery risk.

Choice.
- New `POST /api/v1/articles/link`. Accept at once (202); the worker fetches
  with a dedicated client: no proxies, connect 10 s, total 25 s, at most five
  redirects, body cap 5 MiB, HTML or plain text only. A custom DNS resolver
  drops every non-public address (private, loopback, link-local, CGNAT and
  Tailscale, unique-local, documentation, NAT64, 6to4, Teredo, multicast), so
  rebinding and redirects cannot reach the LAN. URL rules reject IP literals
  in those ranges, explicit ports, credentials and single-label or `.local`,
  `.lan`, `.internal`, `.home.arpa`, `localhost` names.
- Extraction with `dom_smoothie` (Readability port) in Markdown mode, converted
  to the reader format: `# ` headings, paragraphs, `• ` items, table rows as
  `a · b`; images, link targets, emphasis marks and citation markers removed.
  Text over 250 KiB is cut at a paragraph with a note.
- Schema `user_version` 3 adds `jobs.title` for the inbox.
- Page: Field Journal tokens copied into `companion/web/tokens.css`; Literata
  (OFL) embedded; General Sans only from `X3_FONT_DIR` (licence forbids
  redistribution). CSP now `style-src 'self'`, `font-src 'self'`, and
  `img-src https://cards.scryfall.io` for search thumbnails loaded by the
  browser. Fonts cached `private, immutable`; everything else stays `no-store`.

Consequences. Card thumbnails are fetched by the owner's browser directly
from Scryfall; this joins the open Phase 7 check of Scryfall's image terms.
Four MPL-2.0 crates arrive with the HTML parser (THIRD_PARTY.md). Some sites
block automated readers or need a login; those jobs fail with a hint to paste
the text. Wire formats to the X3 are unchanged. Tests: link rules and address
ranges, loopback refusal, Markdown conversion, extraction, charsets, link
submission/retry/failure, titles, fonts and headers.

## ADR 016 — Token image quality

Date: 2026-10-24. Owner asked for better token images.

Context. Tokens were resized bilinearly and dithered with a 4×4 Bayer
pattern: visible crosshatch, banding, grey speckle in white text boxes, and
dark art went muddy. Conversion is host-side only; the X3 shows finished
bits, so this needs no firmware or wire change.

Options compared side by side at 1:1 on three Scryfall tokens (`cargo run -p
x3-tools --example dither_compare`): original; Lanczos3 + Floyd–Steinberg;
Lanczos3 + Atkinson; plus levels and sharpening; plus a 0.8 tone curve.

Choice. Lanczos3 resampling, a 0.5 % levels stretch (skipped on near-flat
images), unsharp mask radius 0.8, grey raised to the power 0.8, then serpentine
Atkinson error diffusion. Atkinson keeps white areas clean and type crisp;
the tone curve opens the shadows it would otherwise clip. `Quality::ORIGINAL`
stays for comparison.

Consequences. The same card now encodes to different bytes than before, so
earlier preview hashes no longer match (a stale preview gets 409 and the page
asks for a fresh preview). Tokens already queued keep their old bytes. The
companion page previews now scale smoothly, because nearest-neighbour shrinking
of a dither shows moiré that the panel never shows. Real panel appearance,
ghosting and any 4-grey mode stay Phase 5 hardware questions.

## ADR 017 — Restore path before Phase 2 (accepted, 2026-10-24)

Date: 2026-10-24. Status: **accepted** by the owner on 2026-10-24 (option B,
with A as fallback). The dump must still confirm the slot layout; if it does
not match the candidate table, reopen this ADR before any flash.

**Reopened and amended 2026-10-26; amendment accepted by the owner the same
day.** The dump does not match the candidate table (HARDWARE.md). Real layout:
CrossPoint 1.6.5 boots from app0 (0x10000); app1 (0x780000, 0x770000 bytes)
holds an older Arduino-based image. Option B stands with more room (7.44 MiB
per slot). Our image goes into app1 at 0x780000 and must be at most 0x770000
bytes. This overwrites the older image in app1; it stays in the full backup.

Context. CrossPoint's SD-card update is part of the CrossPoint app, not the
bootloader, so it cannot rescue a unit that runs our firmware. AGENTS.md rule 3
requires a way back to CrossPoint or stock at every step.

Options.
- A. USB only. Restore means writing the full 16 MB backup back with esptool
  (or using CrossPoint's web flasher). Simple, and needs no firmware code. Every
  restore needs a working data cable and a PC.
- B. Keep CrossPoint in the other OTA slot. Our image goes only into one app
  slot; bootloader, partition table and the CrossPoint slot stay untouched. Our
  firmware gets a "Back to CrossPoint" action that marks the other slot in
  otadata and restarts. No cable needed to get back. Our image must fit one
  slot (7.44 MiB on the owner unit; our firmware is far smaller).
  CrossPoint's own updates write to its inactive slot, which would then be ours,
  so the owner must not update CrossPoint while our firmware is installed.
- C. Our own SD-card update path. Most code and most risk; later, if ever.

Choice. B, with A as the fallback that is always available. Before the first
flash, `x3-tools flash-info` on the backup confirms which slot CrossPoint boots
from; our image goes into the other one. The owner does not update CrossPoint
while our firmware is installed.

Consequences. Phase 2.1 flashes one app slot only, plus otadata to select it;
no bootloader or partition table write. The "Back to CrossPoint" action is a
Phase 2 requirement, tested on the device before any later phase. A USB
restore of the full backup stays the recovery for anything else.

## ADR 018 — Four-grey tokens (accepted, 2026-10-24)

Status: **accepted** by the owner on 2026-10-24. Changes the wire format
(PROTOCOL.md C.1 amended).

Context. Panel test on the owner's X3 (UC8253) through CrossPoint's sleep
screen, 2-bit BMPs that CrossPoint maps without dithering: 1-bit Atkinson
(ADR 016) against four greys with even tone steps and against four greys
tuned to CrossPoint's panel model (15/35/90/210). Owner verdict from photos:
four greys clearly beat 1-bit; even steps beat panel-tuned and match or beat
CrossPoint's own rendering of the original grey art.

Choice. Tokens use `tk1` with bits per pixel `2` (already reserved
in PROTOCOL.md C.1): levels 0 black, 1 dark grey, 2 light grey, 3 white,
four pixels per byte, MSB first. Encoding: ADR 016 preparation, then
Atkinson diffusion to even tones (0, 1/3, 2/3, 1) via `x3_tools::grey4`.
The companion only produces 2-bit tokens; devices accept 1 and 2 bpp, so
tokens queued earlier still deliver. The 1-bit encoder stays for tests.

Consequences.
- Token cap 52,288 → 104,560 bytes (header + 104,544). Twice the SD space per
  cached token (20 tokens ≈ 2 MB) and twice the Receive time per token.
- Companion: payload hashes change again (stale previews get 409); previews
  show four levels; jobs listing cap check uses the new cap.
- Firmware (Phase 5): the panel's grey mode needs its grey waveform and a full
  refresh, which is slower and flashes more than a 1-bit update. CrossPoint
  (MIT) has X3 grey LUTs; study and record before reuse. Measure refresh time
  and ghosting on the device. 1-bit stays the fallback if grey mode fails.
- RAM: draw from SD in two 1-bit planes; never hold a 2-bit frame (104 KB).
- PROTOCOL.md C.1, TESTING.md and the device `hello` (`formats=tk1`) are
  updated with the code.

## ADR 019 — Port the X3 UC8253 constants from MarigoldOS (accepted, 2026-10-26)

Context. Phase 2.2 needs the UC8253 init bytes and waveform tables. Wrong
values can stress the panel, and the owner's controller is UC8253 (About
screen and CrossPoint's boot probe, 2026-10-26).

Options. A: derive values from the UC8253 datasheet (not in hand). B: port
MarigoldOS 4778b2e, Rust, X3 as reference board, values stated as
hardware-verified on the X3, itself ported from CrossPoint. Both MIT.

Choice. B, black and white path only (HALF, FAST, FULL banks; no grey). The
tables are checked byte for byte against the source. Licence texts and both
copyright lines are in THIRD_PARTY.md; `epd.rs` names the source. SPI starts
at 16 MHz, not the reference's 20, for bring-up.

Consequences. MIT notices travel with the firmware. Grey (ADR 018) needs a
separate review of CrossPoint's grey banks before use.

## ADR 020 — Reader text: Literata bitmap fonts, companion hyphenation (accepted, 2026-10-27)

Context. Phase 3 needs a proportional Polish font, justified text and
hyphenation on a 400 KB-RAM device. Owner asked to proceed with Phase 3.

Options. A: parse TTF on the device (rasteriser in firmware, slow, large).
B: pre-rasterised bitmap fonts made on the host. Hyphenation: on the device
(pattern dictionaries per language in flash) or on the companion (soft
hyphens U+00AD inserted before sending).

Choice. B, `x3f` format (`x3_core::font`), made by `x3-tools font` with
fontdue 0.9.4 from Literata 3.103 (OFL 1.1, no Reserved Font Name; licence
and source hashes in `assets/fonts/`). 1-bit, threshold 110. Body Regular
24 px, headings Bold 28 px, side margins 20 px: the owner found the first
guess (30/36) too large on the panel; 24 px gives about 23 lines a page.
Hyphenation on the companion; the device breaks only at spaces and soft
hyphens. U+00A0 never breaks or stretches. Layout and rendering in
`x3_core::reader`, portrait 528×792, transposed to panel rows per HARDWARE.md.

Consequences. Fonts cost ~59 KB of flash for two sizes. Articles without soft
hyphens still render, with more ragged spacing. Grey antialiasing (ADR 018
style) is possible later from the same tool. The companion gains a
hyphenation step (Phase 4/6 task).

## ADR 021 — One Receive session in core; per-session item index (accepted, 2026-10-27)

Context. The simulator had its own Receive client (std, allocation), and it
ran `storage::recover` for every job, which re-hashes every stored payload:
fine on the host, minutes per Receive on an SPI SD card. The device needs
the same protocol logic without std. The SD config name `companion.cfg`
is not 8.3, and embedded-sdmmc opens short names only.

Options. A: separate device client, sim client kept. B: one no_std session
in `x3-core` behind a transport trait, used by both. For the item lookup:
per-job `recover` scans, or one index built at session start.

Choice. B. `x3_core::receive::run` takes an async `Net` (open, send, recv,
close; the transport enforces the 5 s / 10 s timeouts and the 180 s session
deadline) and a `Catalog` (a `Store` that can list generations). The host
drives it with blocking sockets and one poll; the device with embassy-net.
`Index::load` reads every metadata record once, oldest generation first, so
a tombstone just removes its item; at most 64 live items (20 tokens, 32
articles, slack). Payloads are not re-hashed at load; a held payload is
verified only before a download is skipped because of it. A new item when
the index is full is acked `rejected storage`. SD write and commit I/O
failures are acked `rejected storage`, hash/CRC failures `rejected checksum`.
The device config is `/x3/server.cfg` (PROTOCOL.md A.6).

Consequences. One implementation to test (fake-companion tests in
`crates/core/tests/receive.rs`, plus the companion acceptance test through
the simulator). The index does not catch payload damage after commit;
the reader's full verify on open still does. Garbage collection of old
generations remains open.

## ADR 022 — Four-grey tokens with CrossPoint's X3 grey method (accepted, 2026-10-27)

Context. ADR 018 chose 2 bpp tokens and required a review of CrossPoint's X3
grey waveform before reuse. The UC8253 has no built-in grey mode.

Options. A: design our own grey waveform (slow trial and error on the
owner's panel). B: reuse CrossPoint's method and two banks, MIT.

Choice. B. Sequence: full refresh of a base frame (every non-white pixel
black), settle bank `aa_pre_bw_mid` with both RAM planes holding it (CDI
0xA9, 0x07), low plane (dark grey) to DTM1 and high plane (dark and light
grey) to DTM2 as 1 bits, grey nudge bank `gc` (CDI 0x29, 0x07). Before the
next black-and-white update both planes go white and the half bank (ours
is byte-identical) scrubs. Three passes over the token on the card into one
52 KB frame (`x3_core::token` level masks); no 104 KB grey frame in RAM.
Licence notices in THIRD_PARTY.md.

Consequences. CrossPoint's CDI values are used for these steps only; our
black-and-white path keeps MarigoldOS's. One of the banks is described
upstream as extracted from Xteink's OEM firmware; the MIT grant comes from
the projects that publish it. Tone and ghosting need the owner's eyes.

## ADR 023 — Article cache cap and eviction (accepted, 2026-10-31)

Context. The card keeps at most 32 articles (the shelf cap) and 64 live items in
RAM. A 33rd article was rejected as `storage`, so a full device stopped
receiving until the owner deleted something in the portal.

Options. (1) Evict finished articles first, then the oldest received.
(2) Evict the oldest received whatever its read state. (3) Never evict.

Choice. Option 1. A finished article has its saved position at the end of its
text. Eviction writes a tombstone (like a delete job) before the new download
starts; garbage collection (Phase 6) then frees the files. Replacing an
existing item never evicts. Tokens keep their own rule (last 20; a full index
still rejects a new token). No wire change: the companion sees a normal commit.

Consequences. An unread article can be evicted only when all 32 are unread,
and then the oldest goes. The owner was asked and told to proceed on the
default (2026-10-31). Cap and policy are constants in `x3_core::receive`.

## ADR 024 — Bottom button functions: Left, Right, Back, Confirm (accepted, 2026-10-31)

Context. CrossPoint binds the four bottom ladder levels (GPIO1: 3518, 2685,
1486, 4) to Back, Confirm, Left, Right from the left of the device. The owner
wants the page keys on the left and Back and Confirm on the right, like the
button bar drawn on every screen.

Options. (1) Keep CrossPoint's functions and draw the bar to match: Back and
Confirm on the left pair. (2) Remap in the decoder.

Choice. Option 2. `x3_core::buttons::bottom` returns Left, Right, Back,
Confirm for the four levels, left to right. Nothing else knows the ladder, so
every screen follows. The side keys stay Up (left edge) and Down (right
edge), drawn `<` and `>`.

Consequences. Muscle memory from CrossPoint is inverted: the pair that was
Back and Confirm now pages. Serial diagnostics (`buttons`, `keys`) print our
names. Reversible in one function.

## ADR 025 — Wi-Fi setup from the phone, and several saved networks (accepted, 2026-11-01)

Context. The X3 has no keyboard. A new network meant editing `X3/WIFI.CFG` on a
computer, and the file held one network. ADR 008 says the X3 runs no server in
v1; this adds one narrow exception.

Options. (1) A temporary setup hotspot with a page on the phone. (2) A
four-button on-screen keyboard. (3) Bluetooth provisioning (needs an app; the
iPhone Shortcuts app cannot do it). (4) Only the card file.

Choice. Option 1, plus up to four saved networks.

- **Where.** Device screen, Left key. Never starts by itself. It also stops
  after 5 minutes or on Back. The radio is off the rest of the time.
- **The hotspot.** WPA2, name `X3-setup-XXXX`, with a fresh 10-character
  password per session from the hardware RNG. The screen shows both and a QR
  code in the standard `WIFI:` form, so the iPhone camera joins in one tap. An
  open hotspot would let a neighbour read the home password as it was typed.
- **The page.** Plain HTTP on `192.168.4.1:80`, one client at a time, over the
  WPA2 link. It lists the networks the X3 saw, takes a name and password, and
  saves them. A tiny DHCP server hands the phone `192.168.4.2`. No DNS, no
  captive-portal redirect: the screen shows the address.
- **Limits on input.** Request head 1 KiB, body 256 bytes, one request per
  connection, 10 s per read. Name 1–32 bytes, password 8–63 bytes. Nothing from
  the page is printed to the serial log or `LOG.TXT`.
- **Saved networks.** `WIFI.CFG` keeps repeated `ssid=`/`psk=` pairs. The old
  one-network file is the same format with one pair, so it still works. Four
  at most. Saving a name that exists replaces its password; a fifth drops the
  oldest. The new file is written whole to `WIFI.NEW`, then to
  `WIFI.CFG`, then `WIFI.NEW` is deleted. The card library has no rename. If a
  cut leaves `WIFI.CFG` unreadable, the firmware reads `WIFI.NEW`, so the owner
  is never left with no networks.
- **Choosing.** Receive scans, then joins the saved network with the strongest
  signal. If the scan sees none (hidden network) it tries them in file order.
- **After saving.** The X3 closes the hotspot and tries the new network at once.
  The result is on the e-ink screen. The phone has already lost the link, so the
  page says "look at the X3".

Result (2026-11-01, owner's unit). Hotspot, QR join, page and save worked; the
two-network choice followed the signal in both directions (MiFi on: its address
and -51 dBm; MiFi off: the home router). Not tried on the device: a hidden
saved network, a wrong password on the page, a second unit.

Consequences. The image grew by about 77 KB. The `qrcodegen-no-heap` crate (MIT, record in THIRD_PARTY.md). The page cannot confirm the password
worked; the screen does. Captive-portal and WPA-Enterprise networks still do
not work. This does not make the companion reachable from other networks: that
is a separate decision (TLS on the C3, a device route on the public side).

## ADR 026 — Reaching the companion from any network (accepted, option A, 2026-11-01)

Context. The X3 pulls from the companion's LAN listener over plain HTTP, so it
works only on the home network. The owner needs Receive to work on any Wi-Fi
(a phone hotspot, a café). The device cannot join a tailnet, and an iPhone
hotspot does not route its clients through the phone's VPN. The remaining route
is the public hostname through Cloudflare, over TLS.

Spike result (owner's unit, `tls HOST` serial command, `fw/src/tlsprobe.rs`, both removed
2026-11-02 once the route shipped; certificates
NOT verified; measurement only). `embedded-tls` 0.19.0 (Apache-2.0), TLS 1.3,
AES-128-GCM, against the Cloudflare edge for the owner's hostname:

| Measure | Result |
| --- | --- |
| DNS, TCP connect | 10–41 ms, 20 ms |
| TLS 1.3 handshake | 876–889 ms |
| Whole request (join excluded) | about 2.0 s, reply `302` from the Access gate |
| Image size | +195 KB (1.15 MB of a 7.4 MB slot) |
| Radio heap with a 16.6 KB record buffer | 70.4 KB of 73.7 KB used; 3.3 KB free |
| Stack during the handshake | no measurable change (54 KB untouched) |

The heap is the limit, and it is taken from the stack's share (54 KB spare), so it
can grow by about 16 KB. The server key was ECDSA P-256. The chain sent was
leaf (903 B, `*.` the zone, 90-day life), Google Trust Services WE1 (675 B), and
GTS Root R4 cross-signed (894 B). The crate can only anchor a chain at one
configured CA and the third entry is a cross-signature by an RSA root, so a plain
"pin one root" check does not fit Cloudflare's chain without custom code.

Options for knowing the server is the owner's.
A. **Sign at the application layer.** Every device request carries a fresh nonce
   and a MAC (HMAC-SHA-256 keyed by the device token, which is never sent). Every
   reply that lists or confirms things carries a MAC over the nonce, path, status
   and body. Payload bytes are already checked against the hash in the signed job
   list. TLS is used for confidentiality and is not certificate-checked. An active
   man-in-the-middle in a café can read what is fetched but cannot forge a job
   list, learn the token, or alter an article or token. No certificate store, no
   clock dependence, no breakage when Cloudflare changes its CA.
B. **Verify the chain on the device.** Fix the Universal SSL CA to Google Trust
   Services, embed GTS Root R4, and write a verifier for leaf ← WE1 ← R4 (P-256 and
   P-384, dates from the RTC, hostname). Confidentiality holds against an active
   attacker too. More crypto glue on the device, and it breaks if the CA or chain
   shape changes.
C. Both (A now, B later).

Recommendation: A, then B only if confidentiality against an active attacker
matters. Both need owner decisions on the server side:
1. A device hostname (or path) that skips the Access gate: the device cannot do a
   browser login. Either a Bypass policy for `/device/v1/*`, or an Access service
   token sent in two headers. The per-device token (or its MAC) remains the real
   authentication either way.
2. Caddy routes that hostname to the device listener only.

Decision (owner, 2026-11-01: "We'll enable TLS today"; option A taken as recommended).
Signed requests and replies over TLS that is not certificate-checked. Details in
PROTOCOL.md A.8. Choices made while building it:

- The Access gate is passed with a **service token** (two headers), not a bypass, which
  keeps the owner's rule "no Access bypass". The token is stored on the SD card with the
  others.
- A third, **signed-only listener** (`X3_SIGNED_BIND`, port 8789) serves the proxy. The LAN
  listener stays Bearer-only, so a bearer token cannot be used on the internet route.
- `SERVER.CFG` gains `https://host[:port]`, optional `lan=`, `access_id=`, `access_secret=`.
  The device tries `lan=` first and uses the remote name for the rest of the session if it
  fails. A device that sees only the remote address never sends its token.
- The signing key is the stored token hash, so a stolen database can forge requests (it
  also holds every article, so this adds little).
- Nonces are remembered in memory (1,024). A replay after a restart is possible; every route
  is idempotent.
- The device keeps one TLS connection for the session. Record buffers (16.6 KB + 2 KB) come
  from the heap only for the remote form.
- `fw/src/tls.rs` wraps the radio's random generator as a cryptographic source. This is
  sound only while Wi-Fi is on, which is the only time TLS runs.

Revisit (option B) if confidentiality against an active attacker matters.

## ADR 027 — Long articles: read the text a window at a time (accepted, 2026-11-01)

Context: the library read an article's whole text into an 80 KiB stack buffer and refused
longer ones. The companion accepts up to 250 KiB; `MAX_TEXT` is 256 KiB. The X3 has no
PSRAM, and 256 KiB does not fit beside the frame, list and radio heap.

Options: (A) a bigger buffer — does not fit; (B) page index built once per article and
stored on the card — exact, but new on-card files and a slow first open; (C) a text window
over the card file, pages laid out inside it.

Decision: C. `x3_core::window::Window` holds 48 KiB of text (`fw` `WINDOW`). A window starts
at the first paragraph start in the half window before the place it is loaded for, so
headings and justification match the whole-text layout. It moves when a page would run past
its end, and when paging back gets within 2 KiB of its start. Positions stay absolute byte
offsets, so saved places keep working. The whole payload is still verified (streamed) before
opening.

Consequences: any article up to 256 KiB opens, and the reader needs 32 KiB less stack than
before. Page starts can differ slightly after a window move from what one long layout would
give; paging back then forward may show a page that overlaps the one before by a few lines.
Each window move reads up to 48 KiB from the card (timing unverified on the device). Host
tests: `crates/core/tests/window.rs` (identical pages with a large window; 250 KiB article;
paging back with no gaps; multi-byte cuts; hostile input).

## ADR 028 — Pictures beyond tokens: an `image` job kind (accepted, 2026-11-02)

Context: the owner wants the firmware usable by X3 owners who do not play MTG. A token is
already just a 4-grey 528×792 picture on the device; only the companion's source (Scryfall)
is MTG-specific.

Options: (A) send uploaded pictures as `token` jobs with `i-` item IDs — no protocol change;
(B) a new `image` job kind and `im1` format name — clearer for other projects, a protocol
change. Device side: (1) one "Pictures" list; (2) separate Tokens and Images tiles.

Decision (owner): B and 1. `image` jobs carry `im1`, laid out exactly as `tk1`, so the
decoder, checks and storage are shared (`receive::picture`). The device advertises `im1` in
its hello; the companion offers `image` jobs only to devices that do. Item IDs `i-<slug>-<12
hex>`. The Tokens tile and screen become Pictures (tokens and images together, one 20-item
shelf). Upload: `POST /api/v1/images` with the image as the body and picture content types
only; converted at upload with the existing decoder limits; the web page shrinks photos to
2048 px first.

Consequences: no device format work; old firmware is never offered images. Tokens keep
`tk1` and Scryfall. Tests: proto (formats list, `image` jobs, caps, `show`), core (image jobs
stored and shown as pictures, bad payload refused, names), companion (upload, preview, repeat
and conflict, `im1` gating, wrong types, empty, garbage, truncated, too many pixels, over the
cap, bad settings).

## ADR 029 — Saved tokens and pictures; a shorter inbox; tokens-only search (accepted, 2026-11-02)

Context: the owner found the portal inbox grows without end, wanted tokens and pictures kept
on the server for quick re-sending (articles need no server copy), and found the Scryfall
search returned real cards, in no useful order, with no way to see a token's back face.

Decision (owner): the inbox shows the newest 10 with "Show older" (`GET /api/v1/jobs` takes
`before=<job ID>` and returns `more`); the 30-day prune is unchanged. A saved shelf keeps the
finished X3 image of each token or picture once the device confirms it (`delivered` or
`displayed`), one entry per payload SHA-256, newest 50 by last use (about 5 MB). Schema
version 4 adds the `saved` table. Version 5 fills it once with the tokens and pictures already
delivered (newest 50), so the shelf does not start empty after the upgrade. `GET /api/v1/saved`, `GET /api/v1/saved/{sha}/preview.png`,
`POST /api/v1/saved/{sha}/send` (`submission_id`, `show`; idempotent like other submissions)
and `DELETE /api/v1/saved/{sha}`. Sending again creates a new job and a new item ID with the
kept payload: no Scryfall lookup and no conversion. The device protocol does not change.
Search always adds `t:token (…)`; the companion returns a trimmed list (name, set, year, and
per face: type, rules text, power/toughness, colours, Scryfall image links only), exact names
first, then names that start with the words, then whole-word matches, at most 30.

Consequences: a re-sent token that is still on the X3 appears twice there until the 20-item
shelf moves past it. Saved images count against the companion's disk, not its job quota.
Forgetting one on the page does not touch the device. Tests: saved shelf (articles never kept,
kept only on delivery, one entry per image, preview, send again, repeat and conflict, hostile
IDs, forget, the 50 limit), inbox paging (order, `more`, bad cursors), search (query sent,
bad input, ranking, both faces, image host filter), migration to version 4.

## ADR 030 — Following Scryfall's API and image rules (accepted, 2026-11-02)

Context: Phase 7 asked to check Scryfall's current rules before sign-off. Read on 2026-11-02
from scryfall.com/docs/api, /docs/api/rate-limits and /docs/api/images.

Findings and decision: User-Agent and Accept headers were already sent. Rate limits are
per endpoint (`/cards/search` 2 per second, other calls 10 per second); the companion now
spaces searches 500 ms apart and other calls 100 ms, waits at least 30 s after a 429, and
keeps downloads for 24 h (was 1 h). Images must not be cropped so the artist or copyright
line is lost: tokens are always the whole card (`fit=contain`; `cover` refused for tokens,
kept for the owner's own pictures). The page credits Scryfall and Wizards of the Coast and
says neither made or endorses it. Search results are trimmed for the page, not offered as a
public proxy; the page is behind Cloudflare Access.

Open (owner): the rules also say not to desaturate or colour-shift card images. The X3 can
show only four greys, so every token is converted. This is a personal, non-commercial
display of the whole card, unaltered except for what the panel can show. Before publishing,
decide whether to accept that, or to ask Scryfall.

**Owner decision, 2026-11-02:** the X3 is a personal display; showing card images in
four greys on it is accepted. Each owner who builds this uses it the same way.
