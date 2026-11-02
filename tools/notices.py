#!/usr/bin/env python3
"""Write the licence notices that must travel with a built RustInker-X3 firmware file.

Usage: tools/notices.py OUTFILE

Reads `cargo metadata` and `cargo tree` for the fw crate on the device target, so the
list matches the lock file. For every crate linked into the firmware it copies the
licence, copyright and notice files the crate ships. It then adds the code and font
incorporated into this repository and Espressif's prebuilt radio libraries.
Fails if a crate ships no licence file and has no known fallback.
"""
import json
import pathlib
import subprocess
import sys

TARGET = "riscv32imc-unknown-none-elf"
ROOT = pathlib.Path(__file__).resolve().parent.parent
PREFIXES = ("license", "licence", "copying", "copyright", "notice", "unlicense")


def run(*args: str) -> str:
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def linked_packages() -> set[tuple[str, str]]:
    out = run("cargo", "tree", "-p", "fw", "--target", TARGET, "-e", "normal",
              "--prefix", "none", "-f", "{p}")
    pkgs = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            pkgs.add((parts[0], parts[1][1:]))
    return pkgs


def licence_files(directory: pathlib.Path) -> list[pathlib.Path]:
    found = []
    for p in sorted(directory.iterdir()):
        if p.is_file() and p.name.lower().startswith(PREFIXES):
            found.append(p)
    # Workspace crates (esp-hal and friends) keep the texts one level up.
    if not found and (directory.parent / "LICENSE-MIT").is_file():
        found = [directory.parent / "LICENSE-MIT", directory.parent / "LICENSE-APACHE"]
        found = [p for p in found if p.is_file()]
    return found


def section(title: str) -> str:
    return f"\n{'=' * 78}\n{title}\n{'=' * 78}\n"


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    meta = json.loads(run("cargo", "metadata", "--format-version", "1",
                          "--filter-platform", TARGET))
    wanted = linked_packages()
    ours = {"fw", "x3-core", "x3-proto"}
    out = []
    out.append("RustInker-X3 firmware: third-party notices\n")
    out.append("This file lists the third-party software built into the firmware image,\n"
               "with the licence and copyright texts each one requires. RustInker-X3's own\n"
               "code is MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE in the source).\n")

    out.append(section("Espressif radio libraries (prebuilt, in object form)"))
    out.append(
        "libnet80211.a, libpp.a, libphy.a, libcoexist.a and the other prebuilt Wi-Fi,\n"
        "PHY and coexistence libraries, from espressif/esp32-wifi-lib,\n"
        "espressif/esp-phy-lib and espressif/esp-coex-lib, packaged by\n"
        "esp-rs/esp-wireless-drivers-3rdparty and the esp-wifi-sys-esp32c3 crate.\n\n"
        "    Copyright (C) 2015-2016 Espressif Systems\n\n"
        "These binary libraries are provided under the same licence as ESP-IDF: the\n"
        "Apache License 2.0, reproduced below. (The licence calls this \"Object\" form.)\n"
        "No NOTICE file is published with them. Source of the libraries' packaging:\n"
        "https://github.com/espressif/esp32-wifi-lib\n"
        "https://github.com/espressif/esp-phy-lib\n"
        "https://github.com/espressif/esp-coex-lib\n\n")
    out.append((ROOT / "LICENSE-APACHE").read_text())

    out.append(section("Display driver constants and grey method (MIT)"))
    out.append(
        "Panel init bytes, waveform tables and refresh sequences in crates/fw/src/epd.rs\n"
        "come from MarigoldOS, CrossPoint Reader and CrossPlant (freeink-sdk); see\n"
        "THIRD_PARTY.md in the source.\n\n"
        "    Copyright (c) 2026 Jon-Vii\n"
        "    Copyright (c) 2025 CrossPoint Reader organization\n"
        "    Copyright (c) 2025 Dave Allie\n\n")
    out.append(MIT_BODY)

    out.append(section("Literata font (SIL Open Font License 1.1)"))
    out.append((ROOT / "assets/fonts/Literata-OFL.txt").read_text())

    missing = []
    seen_text: dict[str, str] = {}
    for pkg in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        key = (pkg["name"], pkg["version"])
        if key not in wanted or pkg["name"] in ours:
            continue
        directory = pathlib.Path(pkg["manifest_path"]).parent
        files = licence_files(directory)
        out.append(section(f"{pkg['name']} {pkg['version']}"))
        out.append(f"Licence: {pkg.get('license') or 'not declared'}\n")
        if pkg.get("repository"):
            out.append(f"Source: {pkg['repository']}\n")
        if not files:
            name, lic = pkg["name"], (pkg.get("license") or "")
            if name in SOLE:
                holder, kind = SOLE[name]
                out.append(f"\n    {holder}\n\n{MIT_BODY if kind == 'MIT' else ISC_BODY}")
            elif "Apache-2.0" in lic:
                out.append("\nNo licence file in the published crate. Used under Apache-2.0;\n"
                           "the full text is in the Espressif section above.\n")
            else:
                missing.append(f"{name} {pkg['version']} ({lic})")
            continue
        for f in files:
            text = f.read_text(errors="replace").strip()
            first = seen_text.get(text)
            if first:
                out.append(f"\n-- {f.name}: same text as {first}\n")
            else:
                seen_text[text] = f"{pkg['name']} {pkg['version']} {f.name}"
                out.append(f"\n-- {f.name}\n\n{text}\n")

    if missing:
        print("crates without a licence file (add them by hand or fix):", file=sys.stderr)
        for m in missing:
            print("  " + m, file=sys.stderr)
        return 1
    pathlib.Path(sys.argv[1]).write_text("".join(out))
    print(f"wrote {sys.argv[1]}: {sum(1 for n, _ in wanted if n not in ours)} crates")
    return 0


# Crates under a single licence that publish no licence file: copyright lines taken
# from the upstream repository (checked 2026-11-02).
SOLE = {
    "portable_atomic_enum": ("Copyright (c) Thomas Bächler, Dániel Buga (Cargo authors; "
                             "upstream publishes no copyright line)", "MIT"),
    "portable_atomic_enum_macros": ("Copyright (c) Thomas Bächler, Dániel Buga (Cargo "
                                    "authors; upstream publishes no copyright line)", "MIT"),
    "qrcodegen-no-heap": ("Copyright (c) Project Nayuki", "MIT"),
    "void": ("Copyright (c) 2015 The rust-void Developers", "MIT"),
    "riscv": ("Copyright 2019-2025 RISC-V team (rust-embedded)", "ISC"),
    "riscv-pac": ("Copyright 2019-2025 RISC-V team (rust-embedded)", "ISC"),
    "riscv-rt": ("Copyright 2019-2025 RISC-V team (rust-embedded)", "ISC"),
}

ISC_BODY = """Permission to use, copy, modify, and/or distribute this software for any purpose
with or without fee is hereby granted, provided that the above copyright notice
and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.
"""

MIT_BODY = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

if __name__ == "__main__":
    sys.exit(main())
