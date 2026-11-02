# Release checklist for v0.1.0

Status on 2026-11-02. A tag and a public push are the owner's call.
`done` means checked with evidence (see [TESTING.md](TESTING.md)). `owner` needs you. `open` is not done.

## Code and supply chain

| Item | State |
| --- | --- |
| `cargo fmt`, `clippy -D warnings`, tests (146 on the host), `cargo check -p fw` | done |
| `cargo deny check` (advisories, bans, licences, sources) | done, `deny.toml` |
| `cargo audit` | done, `.cargo/audit.toml`; 2 companion-only advisories accepted with reasons |
| Licence audit and `THIRD_PARTY.md` | done, 2026-11-01 |
| Security review | done as a self-review, [SECURITY.md](SECURITY.md); not an independent audit |
| Partition layout compatible with CrossPoint | done: only `app1` and `otadata` are written; ADR 017 |

## Device, verified on one unit (owner's, 2026-10-24 to 2026-11-01)

Home, library with read state, reader, tokens in four greys, Receive, Device page,
button bar and key functions, garbage collection, article and token eviction,
summaries, interrupted Receive (5 resets mid-download), idle power, and the clock.

| Item | State |
| --- | --- |
| Short soak: 40 Receives in 45 min (`tools/soak.py`) | done: 40 ok, 0 damaged, stack steady; moved no data, USB powered |
| 24 h soak | **open**, see below |
| 7-day battery run | **owner**, see below |
| Restore drill | **owner**, see below |
| On-device "Back to CrossPoint" dialog | done: owner walkthrough of every screen, 2026-11-02 (dialog cancelled). The switch itself uses the serial-tested path. |
| A second X3 unit | **open**: nothing is known about other units |
| Wi-Fi setup from the phone, two saved networks (ADR 025) | done on one unit; hidden network and wrong password not tried |
| Physical power pull | waived by the owner (2026-11-01) |

### 7-day battery run (owner)

1. Charge to 100 %. Unplug.
2. Leave the device on Home. It deep-sleeps by itself after 10 minutes.
3. Once a day, wake it, choose Receive, read for a few minutes.
4. After seven days, open `X3/LOG.TXT` on a computer. Every Receive logged a line
   with `soc=` (charge before and after) and `mv=`. Paste the lines here. Expect the
   charge to fall slowly; note the day it reaches 20 %.

### Restore drill (owner)

Do this once, with the backup from [BACKUP.md](BACKUP.md) safe on the computer.

1. From the device: Home, Device, Confirm, Confirm. CrossPoint should start.
2. Check CrossPoint opens a book. This proves the way back works.
3. Write `otadata-app1.bin` (INSTALL.md step 6) to return to this firmware.
4. Optional and risky: restore the **full** backup (BACKUP.md). Only do this if you are
   willing to lose anything written since the backup.

### 24 h soak (open)

Leave a device on USB, then run `tools/soak.py 1000 --interval 85` (about 24 hours of
Receives) and keep the output. Pass: no failed Receive other than a known Wi-Fi refusal,
no damaged item, no steady fall in free stack. The short run in TESTING.md is not a
substitute.

## Packaging

| Item | State |
| --- | --- |
| README, install guide, user guide | done |
| Companion as an optional Docker image | done: `companion/Dockerfile`, `infra/` |
| Prebuilt firmware image | done for v0.1.0: `rustinker-x3-v0.1.0-app1.bin` with `NOTICES.txt` (`tools/notices.py`) and SHA-256 sums on the release. No web flasher. |
| Phone Shortcuts (Share Sheet) | dropped for v0.1.0 (owner, 2026-11-02): the web page is enough. Guide and tested request bodies stay in [SHORTCUTS.md](SHORTCUTS.md) |
| Scryfall rules | done: four greys accepted as a personal display (owner, 2026-11-02, ADR 030) |
| CI (`.github/workflows/ci.yml`: fmt, clippy, tests, firmware build, `cargo deny`) | the commands were run locally and pass; the workflow itself has **not been run** on a hosted runner |
| Publish as a **new** repository from `tools/export-public.sh` | **owner**: this repository's history and hand-off log hold private server and network details, so do not make it public. The script builds a clean tree (no history), fails on personal text and broken links, and its output was built and tested standalone (146 tests). |
| Tag `v0.1.0` | **owner**: `git tag -a v0.1.0 -m "..."`, then push. Not done. |

## Known limits to state in the release notes

UC8253 panels only. One unit tested. 7-day battery run and 24 h soak still to come.
Local time is fixed to Central Europe. No physical power-pull test.
