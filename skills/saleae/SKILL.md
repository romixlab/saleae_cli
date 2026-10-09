---
name: saleae
description: Debug digital buses (I2C, SPI, UART, CAN, LIN, 1-Wire, ...) with a Saleae logic analyzer through the `saleae` CLI and the headless Logic 2 automation server: ask for wiring, capture, decode, summarize. Use when the user wants to look at, sniff, decode or check a bus or signal with a Saleae / logic analyzer, or debug why a device doesn't answer on I2C/SPI/UART/CAN.
---

# Saleae logic analyzer debugging

The `saleae` CLI (`cargo install saleae_cli`; source: https://github.com/romixlab/saleae_cli) drives Saleae
Logic 8 / Pro 8 / Pro 16 through Saleae's headless automation server. No GUI is needed. The server keeps captures in memory and starts in
the background on the first command; `saleae server stop` ends it when you are done.

Add `--json` to any command for machine-readable output (errors become `{"error": "..."}` with exit code 1).

## 0. Setup check (once per machine)

```sh
saleae server path      # fails → saleae server install   (downloads Saleae's preview server zip)
saleae devices          # real devices first, then simulated F4241 (Pro 16), F4244 (Pro 8), F4243 (Logic 8)
```

If only simulated devices are listed while one is plugged in: on Linux the udev rules are missing. Tell the user
the one-time command `saleae server install` prints (`sudo cp .../99-SaleaeLogic.rules /etc/udev/rules.d/ ...`),
then replug the device, `saleae server stop`, and list the devices again. Don't run sudo yourself.

## 1. Ask the user before capturing

You cannot see the bench. Ask, in one message, only what you don't already know:

- **Which bus, and which analyzer channel goes to which signal** (e.g. "ch0 = SDA, ch1 = SCL"; SPI: CLK, MOSI,
  MISO, CS; UART: which side's TX is on which channel). Channel numbers are printed on the probe harness, 0-based.
- **Ground**: the analyzer's GND must be connected to the target's ground. Most "garbage" decodes are a missing
  ground. **Wire every channel as a twisted pair with its own ground** (the channel's signal lead twisted with
  its grey ground lead, or a black ground wire, grounded at both ends; photo:
  [assets/twisted-pair-leads.jpg](assets/twisted-pair-leads.jpg)). One shared ground for several fast lines gives
  crosstalk and ringing: on 9 Oct 2026 four loose lines with one GND made 132 false edges per run on a quiet line
  and -0.7 V / +4.1 V ringing at 10 ns edges; twisted pairs took the Saleae to 0. Remind the user of this when
  they describe loose wires.
- **Logic voltage** (1.2 / 1.8 / 3.3 V, or 5 V): Logic Pro 8/16 take `-V 1.2|1.8|3.3` (use 3.3 for 5 V logic);
  Logic 8 has a fixed threshold, don't pass `-V`.
- **Expected speed** (I2C 100/400 kHz, SPI clock, UART baud, CAN bit rate) and SPI mode if known.
- **When the traffic happens**: continuously, at boot (user must reset the target during the capture), or on an
  action (button, command). This decides between a timed capture and a trigger.

Sample rate: at least 4x, better 10x the fastest bit/clock rate (`-r 10M` default; `-r 50M` for SPI above
~2 MHz). Fewer channels allow higher rates; the server error lists allowed rates when one doesn't fit.

## 2. Decode in one step

`saleae decode` captures, adds one analyzer, prints a compact summary and closes the capture. Options may follow
the protocol.

```sh
saleae decode i2c --sda 0 --scl 1 -t 500ms
saleae decode spi --clk 2 --mosi 0 --miso 1 --cs 3 --mode 0 -r 50M -t 200ms
saleae decode uart --rx 0 --baud 115200 -t 2s            # also: --parity even --stop 2 --inverted --bits 9
saleae decode can --rx 0 --bitrate 500k -t 1s             # --inverted when the probe is on CAN H
saleae decode lin --channel 0 --bitrate 19200
saleae decode onewire --channel 0
saleae decode other "Manchester" --channels 0 --set Manchester=0 --set "Bit Rate (Bits/s)=10000"
```

Useful options: `-d DEVICE` (default: first real device), `-t` duration, `--limit N` lines (default 40),
`--csv FILE` full table, `--save FILE.sal` (the user can open it in Logic 2), `--keep` (leave the capture open for
more analyzers or exports).

Trigger instead of a fixed time, for traffic at an unknown moment:

```sh
saleae decode i2c --sda 0 --scl 1 --trigger 1:falling --after 50ms --timeout 30s
# tell the user: "capturing, now reset the board / press the button"
```

`--trigger CH[:rising|falling|pulse-high|pulse-low]`, `--after` keeps recording after the trigger,
`--min-pulse/--max-pulse` bound pulse triggers, `--link 2=high` requires another channel's state. When the
trigger isn't seen within `--timeout` the header says `trigger NOT seen`. Tell the user to start the decode
first and then do the action; with a long timeout run the command in the background.

## 3. Read the summary

```
I2C on capture 3 (C5FA4D78CE1B1642, D0-1 @ 10 MS/s, 500 ms)
12 frames (address 2, data 6, start 2, stop 2), 12.3 ms .. 13.1 ms
addresses: 0x3C (0 ACK, 1 NAK), 0x50 (1)
   12.3 ms  W 0x50: 00 10 | R 0x50: AB CD NAK
   15.0 ms  W 0x3C NAK
```

- I2C: one line per START..STOP; `|` is a repeated start; `NAK` after an address means nobody answered at that
  address (wrong address, device unpowered or in reset, missing pull-ups); `NAK` after the last read byte is
  normal (the master ends the read).
- SPI: one line per chip-select window, `MOSI: ...  MISO: ...` in hex. All `FF` or all `00` on MISO: the device is
  not driving (CS, power, mode, wrong MISO channel).
- UART: text lines when the bytes are printable, otherwise hex rows. Many `framing` errors: wrong baud rate,
  inverted signal, or the wrong channel. 
- CAN: `0x123 [2] 01 02 ACK`; `NO-ACK` means no other node acknowledged (single node on the bus, wrong bit rate,
  missing termination). Only `ERROR` frames: wrong bit rate or probing CAN H without `--inverted`.
- `0 frames ... nothing decoded`: no activity or wrong channels. Re-check wiring and ground, then capture raw
  edges (`saleae capture ... --export-raw DIR`) to see whether the lines toggle at all.

`--json` gives the same as structured data: `frames`, `counts`, `errors`, `items` (per transaction), `truncated`.

## 4. Report to the user

Summarize in plain words: what was on the bus (addresses, commands, text), what went wrong (NAKs, errors, missing
responses, timing), the likely cause and the next check. Quote a few key lines from the summary, not the whole
dump. Offer the `.sal` file (`--save`) when the user wants to look at it in Logic 2.

## Step by step (several analyzers, exports)

```sh
saleae capture -D 0-3 -r 25M -t 1s               # prints "capture 4 ..."; stays open in the server
saleae analyzer add -c 4 i2c --sda 0 --scl 1      # prints "analyzer 1 ..."
saleae analyzer add -c 4 --label debug uart --rx 2 --baud 115200
saleae summarize -c 4 -a 2                        # summary of one analyzer
saleae export table -c 4 -o /tmp/table.csv        # all analyzers of the capture, one CSV
saleae export raw -c 4 --dir /tmp/raw             # digital.csv / analog.csv
saleae save -c 4 /tmp/cap.sal
saleae close 4
saleae status                                     # server, open captures and analyzers
saleae load file.sal                              # analyze an existing capture file
```

Analog (Logic 8 / Pro): `-A 0 --analog-rate 1.25M`, digital and analog sample rates must be a pair the device
supports (the error lists them). `saleae analyzer list` lists every bundled analyzer.

## Limits

- Simulated devices (F4241, F4244, F4243) produce random edges, not protocol traffic: use them only to check that
  the tools work, never to draw conclusions about a bus.
- Logic MSO is not supported yet by the CLI (the server supports it).
- Saleae's server is a preview build (2.4.45-insider); if it misbehaves: `saleae server stop`, then retry; its log
  is `~/.local/share/saleae_cli/server.log`.
