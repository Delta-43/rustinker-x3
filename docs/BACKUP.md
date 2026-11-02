# Owner-run flash backup (Windows PowerShell)

**Prepared only. No device command has been run by the agent.** Update
2026-10-22: the unit already runs CrossPoint, so these reads capture the
CrossPoint image, not the original stock firmware. That is still the restore
point before our first flash. Charge the device first. Close serial monitors. These
steps read flash and security information; they do not erase, flash or unlock.
esptool may reset the device and load a temporary helper into RAM.

Keep the device awake and on a CrossPoint screen while connecting: in deep
sleep its USB port disappears. If no new port shows up, wake it and plug it in
again.

Use Windows PowerShell in `C:\src\xteink-x3-esp32-os`, Python 3.10 or newer.
The commands use esptool v5's hyphenated names, checked against
[Espressif installation](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/installation.html)
and [command reference](https://docs.espressif.com/projects/esptool/en/latest/esp32c3/esptool/basic-commands.html).
These commands have not been tested on Windows here.

## Cable day at a glance (4-pin cable, due 2026-10-26)

| Step | Owner does | Pass | Stop if |
| --- | --- | --- | --- |
| 1 | Install esptool on the PC | `esptool version` prints 5.x | install fails |
| 2 | Cable check: port list before and after plugging in | a new COM port appears | no new port after the checks in step 2 |
| 3 | `flash-id` and `get-security-info` | 16 MB flash, no secure boot or encryption | any error, lock, or anything in the table in step 3 |
| 4 | Two full reads, compare SHA-256 | both hashes match | a read fails or the hashes differ |
| 5 | Paste the output; agent inspects the dump | partition table and active slot recorded | the agent finds a problem |

Paste the output after steps 2, 3 and 4. Each step only reads. Nothing here
erases, flashes or unlocks.

## 1. Install host tools

This installs tools on the PC only. The virtual environment stays inside the
ignored backups directory, alongside dumps. Do not publish that directory.

```powershell
Set-Location 'C:\src\xteink-x3-esp32-os'
New-Item -ItemType Directory -Force backups | Out-Null
py -3 --version
py -3 -m venv backups\esptool-env
& .\backups\esptool-env\Scripts\python.exe -m pip install 'esptool>=5,<6'
if ($LASTEXITCODE -ne 0) { throw 'Tool installation failed. Stop.' }
& .\backups\esptool-env\Scripts\python.exe -m esptool version
Get-CimInstance Win32_SerialPort | Select-Object DeviceID, Name, PNPDeviceID
& .\backups\esptool-env\Scripts\python.exe -m serial.tools.list_ports -v
```

## 2. Cable check: does a port appear?

The ESP32-C3 has a built-in USB serial port, so no driver is needed on
Windows 10 or 11. Run the two list commands from step 1 with the X3 unplugged,
then wake the X3 (press a button, stay on a CrossPoint screen), attach the
cable and run them again.

- **Pass:** a new port, usually named "USB Serial Device (COMn)" or "USB
  JTAG/serial debug unit", with `VID_303A&PID_1001` in its `PNPDeviceID`
  (303A is Espressif). Note the COM number.
- **No new port:** check the X3 is awake, the cable sits flat on all four
  contacts, and try another USB port on the PC (not a hub). Windows also plays
  the connect sound when a device appears. If still nothing, the cable is
  probably charge-only too. Stop and report; do not try other tools.
- **A port with a different VID:** paste the line and stop.

## 3. Identify the chip and read the security state

Set `$X3Port` to the port you found in step 2. The line below prompts for it.

```powershell
$X3Port = Read-Host 'Enter the confirmed device port, for example COM7'
if ($X3Port -notmatch '^COM[0-9]+$') { throw 'Invalid COM port. Stop.' }
& .\backups\esptool-env\Scripts\python.exe -m esptool --chip esp32c3 --port $X3Port flash-id
if ($LASTEXITCODE -ne 0) { throw 'Flash identification failed. Stop and paste the exact error.' }
& .\backups\esptool-env\Scripts\python.exe -m esptool --chip esp32c3 --port $X3Port get-security-info
if ($LASTEXITCODE -ne 0) { throw 'Security read failed. Stop and paste the exact error.' }
```

Paste the identification and security output before going further if anything
mentions secure download, encryption, secure boot, a lock, or a size other than
16 MB. How to read it:

| Output | Expected (unlocked unit) | If not |
| --- | --- | --- |
| `Detected flash size` | `16MB` | stop |
| `Chip is ESP32-C3` / chip revision | ESP32-C3, v0.4 (About screen said rev 4) | stop |
| `Secure Boot` | `Disabled` | stop: flashing our firmware would fail |
| `Flash Encryption` | `Disabled` | stop: the dump would be ciphertext |
| `SPI Boot Crypt Count` | `0x0` | stop |
| esptool cannot connect at all, or says download mode is disabled | (does not happen) | stop: this is the eFuse lock; recovery needs an external programmer, a separate decision |

Write the flash JEDEC ID down too (one known unit reads `85 20 18`, Puya). Read success alone does not prove custom firmware is permitted. Do not
run unlock tools, eFuse commands, force flags or erase commands. Stop on any
connection/security error; record it and ask for review rather than retrying.

## 4. Two complete reads, with the firmware held in download mode

The `--after no-reset` option prevents booting the installed firmware between
reads, so it cannot write to flash and make the dumps differ. Keep power connected. Each
read starts at zero and covers exactly 0x1000000 bytes (16 MiB).
Names are unique per session and the commands refuse to overwrite existing dumps.

```powershell
$X3Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$X3DumpA = "backups\crosspoint-$X3Stamp-a.bin"
$X3DumpB = "backups\crosspoint-$X3Stamp-b.bin"
if ((Test-Path $X3DumpA) -or (Test-Path $X3DumpB)) { throw 'Backup names already exist. Stop.' }
& .\backups\esptool-env\Scripts\python.exe -m esptool --chip esp32c3 --port $X3Port --baud 115200 --after no-reset read-flash 0x0 0x1000000 $X3DumpA
if ($LASTEXITCODE -ne 0) { throw 'First flash read failed. Stop and paste the exact error.' }
& .\backups\esptool-env\Scripts\python.exe -m esptool --chip esp32c3 --port $X3Port --baud 115200 --after no-reset read-flash 0x0 0x1000000 $X3DumpB
if ($LASTEXITCODE -ne 0) { throw 'Second flash read failed. Stop and paste the exact error.' }
if ((Get-Item $X3DumpA).Length -ne 16777216) { throw 'First dump has the wrong size. Stop.' }
if ((Get-Item $X3DumpB).Length -ne 16777216) { throw 'Second dump has the wrong size. Stop.' }
$X3HashA = (Get-FileHash -Algorithm SHA256 $X3DumpA).Hash
$X3HashB = (Get-FileHash -Algorithm SHA256 $X3DumpB).Hash
Get-FileHash -Algorithm SHA256 $X3DumpA, $X3DumpB | Format-Table -AutoSize
if ($X3HashA -ne $X3HashB) { throw 'Hashes differ. Keep both dumps and stop for review.' }
"Verified matching SHA-256: $X3HashA"
```

The built-in USB port ignores the baud rate, so each read takes a few minutes
rather than the half hour a serial adapter would need. Do not disconnect during a read.
After both succeed, unplug/reconnect normally to return to CrossPoint.
Keep both files and an additional private copy on another disk. Dumps may contain
Wi-Fi credentials and device identifiers. Paste only the command output, sizes
and hashes; do not upload the dumps or commit them.

## 5. Report and stop

Paste both matching SHA-256 values, esptool version, flash/security output and
the CrossPoint version shown on the device. The next agent records evidence in
MEMORY.md and examines the partition table locally with your permission.

`T:` maps the `data` share on home-server, so the dumps also sit in
`/data/projects/xteink-x3-esp32-os/backups/` there. With your permission the
agent runs one read-only command on the first dump:

```
cargo run -q -p x3-tools -- flash-info backups/crosspoint-<stamp>-a.bin
```

It prints the bootloader header, the partition table, the app name and
version in each slot, both otadata entries and which slot boots. That answers
the restore-path question (ADR needed before Phase 2): is the second OTA slot
empty, or does it hold another CrossPoint build? The agent pastes nothing from
the dump except that report; NVS (which holds Wi-Fi credentials) is never
printed. No restore or custom flash
is part of this session. If USB download is locked, report it; an external
programmer is a separate owner-approved recovery task, not an automatic fallback.
