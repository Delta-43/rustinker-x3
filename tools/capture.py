#!/usr/bin/env python3
"""Save the X3's screens as PNGs while you use it.

Usage: capture.py OUTDIR [SECONDS] [PORT]

Sends `capture on`, then records every frame the firmware draws (see
crates/fw/src/capture.rs) until SECONDS pass (default 300) or Ctrl+C, and
sends `capture off`. Black-and-white frames become one PNG each; a grey
picture (base, low and high planes) becomes one PNG in four greys.
Files are numbered in the order they appeared: 001.png, 002.png, ...
Needs pyserial and Pillow.
"""
import pathlib
import sys
import time

import serial
from PIL import Image

ROWS, ROW_BYTES = 528, 99  # raw panel rows; each is one portrait column
W, H = 528, 792  # portrait image
LEVELS = {0: 0, 1: 85, 2: 170, 3: 255}


def bits(lines):
    """Raw rows (hex) -> set test: ink[x][y] where the raw bit is 1."""
    grid = []
    for line in lines:
        b = bytes.fromhex(line)
        if len(b) != ROW_BYTES:
            raise ValueError(f"row of {len(b)} bytes")
        grid.append(b)
    if len(grid) != ROWS:
        raise ValueError(f"{len(grid)} rows")
    return lambda x, y: (grid[x][y >> 3] >> (7 - (y & 7))) & 1


def save_bw(path, one):
    im = Image.new("L", (W, H), 255)
    px = im.load()
    for x in range(W):
        for y in range(H):
            if not one(x, y):  # 0 bits are ink
                px[x, y] = 0
    im.save(path, optimize=True)


def save_grey(path, base, low, high):
    im = Image.new("L", (W, H), 255)
    px = im.load()
    for x in range(W):
        for y in range(H):
            if low(x, y):
                level = 1
            elif high(x, y):
                level = 2
            elif not base(x, y):
                level = 0
            else:
                level = 3
            px[x, y] = LEVELS[level]
    im.save(path, optimize=True)


def main() -> int:
    if not 2 <= len(sys.argv) <= 4:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 300.0
    port = sys.argv[3] if len(sys.argv) > 3 else "/dev/ttyACM0"
    n = 0
    pending = {}
    with serial.Serial(port, timeout=0.2) as s:
        time.sleep(0.5)
        s.reset_input_buffer()
        s.write(b"capture on\n")
        s.flush()
        end = time.time() + seconds
        tag, rows, buf = None, [], b""
        try:
            while time.time() < end:
                buf += s.read(4096)
                while b"\n" in buf:
                    raw, buf = buf.split(b"\n", 1)
                    line = raw.decode(errors="replace").strip()
                    if line.startswith("CAP "):
                        word = line[4:]
                        if word == "end" and tag:
                            try:
                                grid = bits(rows)
                            except ValueError as e:
                                print(f"skipped a {tag} frame: {e}", file=sys.stderr)
                                tag, rows = None, []
                                continue
                            if tag == "bw":
                                n += 1
                                save_bw(out / f"{n:03}.png", grid)
                                print(f"{n:03}.png", flush=True)
                            else:
                                pending[tag] = grid
                                if tag == "high" and {"base", "low"} <= pending.keys():
                                    n += 1
                                    save_grey(out / f"{n:03}.png", pending["base"], pending["low"], grid)
                                    print(f"{n:03}.png (greys)", flush=True)
                                    pending.clear()
                            tag, rows = None, []
                        else:
                            tag, rows = word, []
                    elif tag:
                        rows.append(line)
        except KeyboardInterrupt:
            pass
        s.write(b"capture off\n")
        s.flush()
    print(f"saved {n} screens to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
