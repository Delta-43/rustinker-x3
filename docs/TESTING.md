# Test and acceptance guide

PLAN.md defines phase completion. A local pass is not hosted CI evidence.
Hardware tests need owner output; none have passed in this session.

## Host commands (Linux or Windows with Rust installed)

```text
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --exclude fw --all-features
cargo check --locked -p fw --target riscv32imc-unknown-none-elf
cargo build --locked -p fw --release --target riscv32imc-unknown-none-elf
cargo run --locked -p sim -- samples
cargo run --locked -p x3-tools -- token samples/token-1.png target/token.tk1 contain landscape
```

Core and proto are no_std. The firmware target is a release link proof, not
an installable device image. Host tools and PNG decoding use std and allocations.
CI runs the first five checks. Protocol and storage coverage exceeds the 80%
line target. ADR 012 HTTP 204 handling and companion interoperability now pass
local tests, including real TCP and a FAT-backed simulated Receive.

## Phase 0 — research and safety

Agent: inspect pinned sources, root and nested licences; record pin/bus/I2C/button
and display variants with confidence; record candidate partitions and a paper
RAM budget; prepare backup instructions. Missing sources remain unverified.
Owner: charger and custom-firmware acceptance confirmed 2026-10-22 (runs
CrossPoint); stack approved (ADR 009). Still needed: chip/security read, two
16 MiB backups with matching SHA-256 (BACKUP.md) and the panel controller.
No flashing before these pass.

## Phase 1 — independent logic and protocol

| Area | Automated host checks | Remaining acceptance |
| --- | --- | --- |
| Bitmap | MSB pack/unpack, bad bit depth, sample range, lengths, padding, Bayer endpoints, PNG goldens at 1/2 bpp | Owner may review sample quality |
| Text | Polish, long words, soft hyphens, blank paragraphs, headings, empty input, UTF-8 positions, controls and caps | Full pages rendered to PNG |
| Cache | add/replace/evict, oldest-read priority, reload, CRC/truncation, invalid paths, interrupted write/flush/rename in fake storage, 500 items | Fake guarantees are not FAT power-loss guarantees |
| Settings | bounds and defaults | Hardware policies deferred |
| Protocol v2 | **To implement** (PROTOCOL.md approved) | round-trip; malformed/truncated/oversized lines and payloads; wrong magic/version/CRC/SHA-256; field counts; ID traversal; UTF-8 and length caps; HTTP head limits, chunked, 3xx |
| Tools | Raw image conversion only | `tk1` (contain/cover, orientation) and `ar1` encoders to implement |

Goldens are committed expected pixels, decoded before comparison. Ordinary tests
never regenerate them. To propose changed goldens, run `cargo run -p sim --
target/proposed-goldens`, review the images and copy them deliberately into
`crates/sim/tests/golden/`. Current samples use original geometric art and text.
A simulator pass proves pixels and wrapping, not refresh timing or physical grey.
Measure production code with `cargo llvm-cov -p x3-core --summary-only
--ignore-filename-regex '/tests/'` (install cargo-llvm-cov and llvm-tools-preview).
Coverage must come from a run, not test count. Corrupt index recovery opens an
empty usable library and retains orphan payloads for later validation.

## Phase 1B — companion and simulated Receive (planned)

All tests below are new requirements, not existing results. `sim receive` plays
the X3: it pulls from a running companion and writes to a disk image.

| Area | Required evidence |
| --- | --- |
| Durable acceptance | Reader offline: content and job persist before an accepted response; phone can disconnect |
| Restart | Accepted and in-progress jobs recover after process termination; no loss or duplicates |
| Idempotency | Repeated `submission_id` and a lost device ack produce one logical committed item |
| Bounded inputs | Bad auth, malformed metadata, traversal, oversized images/text and full server disk fail safely |
| Delivery policy | Leases, `delivery_started` cancel races (ADR 012), separate delivered/displayed outcomes |
| HTTP interop | Device client parses real axum responses over TCP, including bodyless 204 |
| Replacement | Old committed content retained on failure; changed article text has an explicit position policy |
| Conversion | Aspect ratio/orientation; non-queueing token preview with hash check; Polish text; approved formats |
| Restore | `VACUUM INTO` backup restored to a scratch instance; same jobs and payload hashes |
| Device persistence | Streaming adapter plus disk-image torn-write/rebuild tests; no FAT safety claim from fake rename |

## Phase 1C — deployment on home-server (planned)

Use INFRASTRUCTURE.md section 8. The owner reviews the concrete diff before
anything is applied. Required checks: Access JWT required on every public
request (including via Caddy's LAN port 80), `/device/*` not reachable through
the public hostname, Shortcut and browser get JSON (not a login page), `sim
receive` from another LAN machine pulls from the deployed device listener,
backup/restore and rollback keep pending content. Do not mark this stage
passed from local Rust tests or documentation review.

## Phase 2 — peripherals (owner only, out of this session's scope)

In order, after approved backup and restore plan: serial hello/chip information;
full white/black/checkerboard then partial display with logged times; every button
once per press; SD mount/list/100 MB random write/read with checksum plus removal
and reinsertion; DS3231 read/set/power-cycle; gauge voltage/SOC/charging; optional
IMU orientation; deep sleep/wake and measured sleep current if a meter exists.
Each row passes from pasted output or an explicit owner waiver. A failed flash
stops work; record the exact error and do not retry blindly.

## Phase 3 — local reader

Agent: stable pagination goldens across chosen fonts. Owner: page-turn latency,
50-turn ghosting check, Polish glyphs, reading position across power cycles.
The earlier MarigoldOS 424 ms report is a comparison only, not this project's result.

## Phase 4 — Wi-Fi and Receive client

Agent: the Receive state machine against a local companion: wrong token (401
stops the session), oversize (rejected ack), truncated download (no committed
item), lost ack (replay accepted), session timeout. Owner: ten Receive sessions
from the deployed companion, Wi-Fi off afterwards, battery drop per session.
Measure live RAM: >=40 KiB headroom; otherwise an ADR and owner decision are needed.

## Phase 5 — tokens

Agent: protocol round-trip and non-MTG dithering goldens. Owner: send five tokens,
power-cycle, last image persists, recall older cached token offline, legibility
at table distance. Physical 4-grey refresh remains a separate test.

## Phase 6 — offline articles

Agent: cache property tests, 500-item listing timing, Polish/emoji robustness.
Owner: five articles plus two summaries, Wi-Fi-off reading; five deliberate power
cuts during upload; reopen library with no corrupt visible entry; cap/eviction.
Obtain safe power-cut instructions for the actual device before those tests.

**Interrupted-Receive drill (Phase 6, run 2026-11-01).** Host: `cargo test
--release -p x3-core --all-features --test receive -- --ignored` cuts a whole
Receive (replace, new article, delete, new token, eviction, GC) at every
write with three tear sizes (3,162 cuts); a sampled version runs by default.
Device: `tools/drill-companion.sh up` starts a throwaway companion (its own
container, volume and tokens; the real one is never touched), `queue 20` adds
large articles, the serial command `drill TOKEN` points the device at it
(the real `/X3/SERVER.CFG` is kept in `SERVER.BAK`), `tools/cutdrill.py 5`
resets the chip 4–14 s into each Receive, and `check` verifies the card after
every cut; `drill off` and `drill-companion.sh down` undo it. This is a reset
with the SD card still powered, **not** a power cut.


## Phase 7 — iPhone flow

Agent: browser automation against simulator and current Scryfall client rules.
Owner: ten clean runs each of token search/send/view, Safari article share/read
offline and email summary. Record failures; document web UI and Shortcut setup.

## Phase 8 — release

Agent: cargo audit, cargo deny, dependency/font licence review, full CI, clean
clone on another OS if available, auth/limits/path review. Owner: seven-day fixed
battery routine, 24-hour idle soak, restore-to-stock drill and release sign-off.
No release or hardware phase is complete from a host build alone.

**Short soak (run 2026-11-01, `tools/soak.py 40 --interval 60`):** 40 Receives in
45 minutes against the real companion with nothing queued (each is Wi-Fi join,
hello, empty job list, GC, Wi-Fi off). Result: 40 ok, 0 failed; duration min 3.5 s,
median 3.8 s, max 8.5 s (the slower ones were a slower DHCP answer from the router);
four `check` runs, each 30 items verified, 0 damaged; free stack 96,776 bytes at the
start, one 80-byte step at cycle 11, then constant at 96,696 for the other 29 cycles.
This shows repeated Wi-Fi bring-up and tear-down does not fail or leak stack in 45
minutes. It is **not** the 24 h soak: it moved no data, so it does not exercise
downloads or storage writes, and it ran on USB power, so it says nothing about sleep
or battery.

**Supply chain (2026-11-01):** `cargo deny check` and `cargo audit` pass, with two
companion-only advisories accepted with written reasons (`deny.toml`,
`.cargo/audit.toml`, [SECURITY.md](SECURITY.md)). The public export built and passed
all 146 host tests on its own (`tools/export-public.sh`).


### Wi-Fi setup and saved networks (2026-11-01, ADR 025)

Host: `cargo test -p x3-proto` (config list, save, replace, fifth drops oldest, bad
files), `cargo test -p x3-core --test setup` (request parser: truncated, oversized,
smuggled, chunked, duplicate length; form decoder; page escaping; password alphabet;
QR text; join order), `--test dhcp` (offer, ack, junk, mutated packets). The QR in
`samples/gui/gui-wifi-setup.png` decodes to the join string.

Device (owner's unit): hotspot, QR join on an iPhone, page, Save, second network,
result screen. Join order followed the signal with the second network on and off.
**Unverified:** hidden saved network, wrong password on the page, power cut during the
save, other units.

## Protocol and storage implementation evidence

Host checks now include `--all-features` so the optional FAT adapter is tested:

```text
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --exclude fw --all-features
cargo check --locked -p fw --target riscv32imc-unknown-none-elf
```

40 tests pass locally. Protocol line coverage: 99.41%; storage 94.67%; FAT adapter
95.83% (test sources excluded). Storage faults: 228 replacement cuts, 30 position
cuts and 30 deletion cuts. At each block write, fail before writing or after
1, 127, 255, 511 or 512 bytes, then discard the manager and reopen the image.
This models torn writes to the addressed block, not arbitrary corruption of
unrelated blocks or a real card controller. Hardware guarantees remain unverified.
The fixture is an original synthetic FAT16 image; FAT32-specific and physical SD
behaviour still require further validation. Garbage collection is not implemented.

The HTTP codec currently implements A.1 literally, including mandatory
Content-Length on 204; this is incompatible with standard HTTP. ADR 012 proposes
the correction and requires a real HTTP response regression test before 1B.
No companion, deployment or hardware acceptance result is implied by host tests.

## Phase 1B local acceptance evidence (2026-10-22)

Run `cargo test -p x3-companion --test acceptance`. TCP tests bind loopback;
restricted sandboxes may require permission for local sockets. No live service
credentials, Scryfall art or reader are involved.

| Infrastructure section 8 row | Passing test |
| --- | --- |
| Submit offline; durable acceptance | durable_accept_repeat_conflict_restart_backup |
| Kill after 202, restart | process_killed_after_accept_keeps_job (real child process) |
| Same submission; changed body | durable_accept_repeat_conflict_restart_backup |
| Simulated pull and lost ack | simulated_receive_lost_ack_then_replay_and_token_display |
| Cancel wins | cancellation_three_races_and_lost_ack |
| Delivery wins, including expired lease | cancellation_three_races_and_lost_ack |
| Cancelled skip-path ack | cancellation_three_races_and_lost_ack |
| Real axum 204 over TCP | real_axum_ack_over_tcp |
| Preview PNG/hash, no job, mismatch | preview_hash_and_concurrency |
| Malformed/oversized/auth/disk full | input_auth_and_full_storage (SQLite page exhaustion) |
| Backup/restore and payload hashes | durable_accept_repeat_conflict_restart_backup |

Additional tests validate signed RS256 tokens, wrong issuer/audience/expiry/nbf,
algorithm confusion, spoofed identity headers, URL allowlists, conversion limits
and the interrupted-session database recovery fix. Scryfall tests use generated
grey pixels; live API/image policy verification remains Phase 7. Browser/iPhone,
production Access, Caddy, Docker and physical SD results remain unverified.

The Receive session itself is `x3_core::receive` (ADR 021), shared by the
simulator and the firmware. `crates/core/tests/receive.rs` runs it against a
scripted fake companion: new, replaced, held and deleted items; `show` and the
owed `displayed` ack; unsupported, cancelled and bad-hash jobs; 3xx, chunked,
missing length, truncated, overrun, oversized and bad-line responses; 401, 500,
connect and timeout errors; a full index and token eviction. Responses arrive
in 7- and 97-byte pieces to test TCP fragment boundaries.

The simulator creates an original FAT16 image only if the supplied image file
is absent. It rejects non-regular files. Run with a private companion config:

```text
cargo run -p sim -- receive path/to/server.cfg target/sim-sd.img target/receive
```

`screen.json` and `display.png` model the host display, outside the SD image.
The last committed show token is rendered even when a later session operation
fails. A subsequent session repeats committed before displayed, so a lost commit
ack cannot make the displayed ack fail. Articles only produce preview files.
