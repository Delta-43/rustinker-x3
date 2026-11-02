#!/usr/bin/env python3
"""Reset-in-the-middle-of-Receive drill.

Usage: cutdrill.py CYCLES [--port P] [--wait S] [--min S] [--max S] [--seed N]

Each cycle: `check`, start `receive`, hard-reset the chip after a random delay
(a read-only `esptool chip-id`, which resets it mid-session), wait for the
reboot, `check`. The jobs a cut leaves unacknowledged come back after the
companion's 300 s lease, so the next cycle cuts into them again: one queue of
tokens and articles feeds many cuts. After the last cycle a full recovery
`receive` runs (retrying a join the router refuses after a reset) and a final
`check` must show a clean card: every item verified, and exactly one metadata
and one payload file per item.

The chip joins Wi-Fi in about 3 s and then downloads (about 5 s per token), so
the default cut window is 4-14 s. Queue 20 or more items.

This kills the firmware mid-write with the SD card still powered. It is not
a power cut: the card's own power is never removed. Run it with the esptool
environment's Python (it has pyserial), and nothing else holding the port.
It writes nothing to the flash.
"""
import argparse
import random
import re
import subprocess
import sys
import time

import serial

SUMMARY = re.compile(
    r"check: (\d+) items, (\d+) verified, (\d+) damaged; "
    r"(\d+) metadata and (\d+) payload files"
)


def read_until(s: serial.Serial, done, timeout: float) -> str:
    """Reads lines until `done(text)` is true or `timeout` passes."""
    text = ""
    end = time.time() + timeout
    while time.time() < end:
        data = s.read(512)
        if data:
            text += data.decode(errors="replace")
            if done(text):
                break
    return text


def session_over(text: str) -> bool:
    """True once a Receive has ended, successfully or with an error."""
    for line in text.splitlines():
        if line.startswith("receive: done in"):
            return True
        if line.startswith("receive: ") and " after " in line and "log " not in line:
            return True
    return False


def command(port: str, line: str, done, timeout: float) -> str:
    with serial.Serial(port, timeout=0.1) as s:
        time.sleep(2.0)
        s.reset_input_buffer()
        s.write(line.encode() + b"\n")
        s.flush()
        return read_until(s, done, timeout)


def check(port: str) -> tuple[str, tuple[int, ...] | None]:
    # Opening the port can nudge the chip; the first try may arrive too early.
    for _ in range(3):
        out = command(port, "check", lambda t: SUMMARY.search(t) is not None or "check: cannot" in t, 60)
        lines = [l for l in out.splitlines() if l.startswith("check:")]
        m = SUMMARY.search(out)
        if lines:
            return "\n".join(lines), tuple(int(x) for x in m.groups()) if m else None
        time.sleep(3)
    return "", None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("cycles", type=int)
    ap.add_argument("--port", default="/dev/ttyACM0")
    ap.add_argument("--wait", type=float, default=310.0)
    ap.add_argument("--min", type=float, default=4.0)
    ap.add_argument("--max", type=float, default=14.0)
    ap.add_argument("--seed", type=int, default=None)
    a = ap.parse_args()
    rng = random.Random(a.seed)
    bad = 0
    for n in range(1, a.cycles + 1):
        print(f"--- cycle {n}/{a.cycles}", flush=True)
        text, base = check(a.port)
        print(f"before:   {text or 'NO CHECK REPLY'}", flush=True)
        delay = rng.uniform(a.min, a.max)
        with serial.Serial(a.port, timeout=0.1) as s:
            time.sleep(2.0)
            s.reset_input_buffer()
            s.write(b"receive\n")
            s.flush()
            seen = read_until(s, session_over, delay)
        # The reset: esptool's connect sequence resets the chip at once.
        subprocess.run(
            [sys.executable, "-m", "esptool", "--port", a.port, "--after", "hard-reset", "chip-id"],
            check=False,
            capture_output=True,
        )
        finished = session_over(seen)
        last = [l for l in seen.splitlines() if l.startswith(("receive:", "wifi:", "token:"))][-1:]
        label = "Receive had ALREADY FINISHED, not a real cut" if finished else "mid-session"
        print(f"cut at {delay:.1f} s ({label}); last line: {last[0][:100] if last else '-'}", flush=True)
        time.sleep(7)
        text, after = check(a.port)
        print(f"after:    {text or 'NO CHECK REPLY'}", flush=True)
        ok = after is not None and after[2] == 0 and base is not None
        verdict = "PASS" if ok else "FAIL"
        bad += verdict == "FAIL"
        print(f"cycle {n}: {verdict} (nothing damaged after the cut: {ok})", flush=True)
        if n < a.cycles:
            print(f"waiting {a.wait:.0f} s for the companion's lease to expire", flush=True)
            time.sleep(a.wait)
    # One full recovery at the end. The router may refuse the first join after
    # a reset (AssociationComebackTimeTooLong): retry.
    print("--- recovery", flush=True)
    recovered = False
    for attempt in range(1, 5):
        out = command(a.port, "receive", session_over, 180)
        lines = [l for l in out.splitlines() if l.startswith("receive: ") and "log " not in l and "panel" not in l and "items on" not in l]
        print(f"recovery {attempt}: " + " | ".join(l[:110] for l in lines[-3:]), flush=True)
        if "receive: done in" in out:
            recovered = True
            break
        time.sleep(30)
    text, final = check(a.port)
    print(f"final:    {text or 'NO CHECK REPLY'}", flush=True)
    clean = recovered and final is not None and final[2] == 0 and final[3] == final[0] and final[4] == final[0]
    print(f"final card: {'CLEAN' if clean else 'NOT CLEAN'} (recovered: {recovered})", flush=True)
    bad += not clean
    print(f"{a.cycles} cuts, {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
