#!/usr/bin/env python3
"""Send one line command to the X3 bring-up firmware and print the replies.

Usage: x3cmd.py COMMAND [SECONDS] [PORT]
Needs pyserial (esptool's environment has it). Reads for SECONDS (default 5)
after sending, on one connection, so the reply is not lost. SECONDS 0 reads
until Ctrl+C, then sends "stop" (ends a running `buttons` watch).
"""
import sys
import time

import serial


def main() -> int:
    if not 2 <= len(sys.argv) <= 4:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    command = sys.argv[1]
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 5.0
    port = sys.argv[3] if len(sys.argv) > 3 else "/dev/ttyACM0"
    if len(command) >= 32 or not command.isprintable():
        print("command must be printable and under 32 characters", file=sys.stderr)
        return 2
    with serial.Serial(port, timeout=0.1) as s:
        time.sleep(0.5)
        s.reset_input_buffer()
        s.write(command.encode() + b"\n")
        s.flush()
        end = time.time() + seconds
        try:
            while seconds == 0 or time.time() < end:
                data = s.read(256)
                if data:
                    print(data.decode(errors="replace"), end="", flush=True)
        except KeyboardInterrupt:
            s.write(b"stop\n")
            s.flush()
            time.sleep(0.3)
            print(s.read(512).decode(errors="replace"), end="", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
