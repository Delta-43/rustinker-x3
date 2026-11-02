#!/usr/bin/env python3
"""Repeated-Receive soak (Phase 8): many Receives in a row, watching for failures,
memory loss and card damage.

Usage: soak.py CYCLES [--interval S] [--port P] [--check-every N]

Each cycle sends `receive`, waits for it to end, and records the outcome, how long
it took and the stack left free. Every N cycles it runs `check` (every live item
verified). Run it with the esptool environment's Python (it has pyserial) and
nothing else holding the port. Writes nothing to the flash. It is a short stand-in
for the plan's 24 h soak, not the same thing.
"""
import argparse
import re
import sys
import time

import serial

DONE = re.compile(r"receive: done in (\d+) ms")
FAIL = re.compile(r"receive: (.+) after (\d+) ms")
STACK = re.compile(r"stack (\d+) untouched of (\d+)")
CHECK = re.compile(r"check: (\d+) items, (\d+) verified, (\d+) damaged")


def run(port: str, line: str, done, timeout: float) -> str:
    with serial.Serial(port, timeout=0.1) as s:
        time.sleep(2.0)
        s.reset_input_buffer()
        s.write(line.encode() + b"\n")
        s.flush()
        text, end = "", time.time() + timeout
        while time.time() < end:
            data = s.read(512)
            if data:
                text += data.decode(errors="replace")
                if done(text):
                    break
        return text


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("cycles", type=int)
    ap.add_argument("--interval", type=float, default=60.0)
    ap.add_argument("--port", default="/dev/ttyACM0")
    ap.add_argument("--check-every", type=int, default=10)
    a = ap.parse_args()
    ok = failed = damaged = 0
    stacks, times, reasons = [], [], {}
    started = time.time()
    for n in range(1, a.cycles + 1):
        out = run(a.port, "receive", lambda t: DONE.search(t) or FAIL.search(t), 90)
        m, f, st = DONE.search(out), FAIL.search(out), STACK.search(out)
        if m:
            ok += 1
            times.append(int(m.group(1)))
            verdict = f"ok {m.group(1)} ms"
        else:
            failed += 1
            why = f.group(1)[:60] if f else "no reply"
            reasons[why] = reasons.get(why, 0) + 1
            verdict = f"FAILED: {why}"
        if st:
            stacks.append(int(st.group(1)))
        print(f"[{(time.time() - started) / 60:5.1f} min] cycle {n}/{a.cycles}: {verdict}; "
              f"stack free {st.group(1) if st else '?'}", flush=True)
        if n % a.check_every == 0 or n == a.cycles:
            c = CHECK.search(run(a.port, "check", lambda t: CHECK.search(t), 90))
            if c:
                damaged += int(c.group(3))
                print(f"           check: {c.group(1)} items, {c.group(2)} verified, {c.group(3)} damaged", flush=True)
            else:
                print("           check: NO REPLY", flush=True)
                damaged += 1
        if n < a.cycles:
            time.sleep(a.interval)
    print(f"\n{a.cycles} Receives in {(time.time() - started) / 60:.0f} min: {ok} ok, {failed} failed, "
          f"{damaged} damaged items found")
    if times:
        print(f"duration ms: min {min(times)}, median {sorted(times)[len(times) // 2]}, max {max(times)}")
    if stacks:
        print(f"stack free: first {stacks[0]}, last {stacks[-1]}, lowest {min(stacks)} (growing loss would show here)")
    for why, count in reasons.items():
        print(f"failure x{count}: {why}")
    return 1 if failed or damaged else 0


if __name__ == "__main__":
    sys.exit(main())
