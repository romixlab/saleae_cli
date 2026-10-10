# saleae_cli features and roadmap

This file is the single source of truth for what saleae_cli does, what is broken and what is planned.
[CHANGELOG.md](CHANGELOG.md) records what changed and when; this file records the current state.

## How to use this file

- **Status**: ✅ done · 🧪 done, not yet verified on real hardware or real bus traffic · 🚧 partial (the note says
  what is missing) · 🐛 known bug · 📋 planned · 💡 idea · ⛔ blocked (on what) · 🔍 needs a check.
- **IDs** (`CAP-3`) are stable: never renumber or reuse one. New ids come from `tpm work claim AREA`. Use the ID in
  commit messages, CHANGELOG entries and code `TODO`s (`// TODO(CAP-4): ...`); name it with a short slug when
  talking to people (`CAP-4 mso-capture`).
- When finishing work, update the item in the same commit. Obsolete items go to *Dropped and superseded*.

## Library (`LIB`)

- 🧪 **LIB-1 Library-first split**: a Cargo workspace: `crates/saleae_automation` (the library: server
  lifecycle, typed capture/analyzer/export calls and results, summaries; `saleae_automation::Error` via
  thiserror; not `saleae`, that crates.io name is someone else's older, unrelated package), `crates/saleae_cli`
  (the same `saleae` commands, flags, JSON and completions, on the library; CLI-6) and `crates/saleae_py`
  (PY-1).
  Unit tests moved with the code and pass; `crates/saleae_cli/tests/sim.rs` (decode on simulated devices,
  `server stop`) passes against the real headless server.

## Server (`SRV`)

- ✅ **SRV-1 Install**: `saleae server install` downloads Saleae's preview headless server zip for this platform
  (`--build`, `--url`, or `--zip` for a downloaded file) with `curl` and unpacks it into
  `~/.local/share/saleae_cli/server` (`SALEAE_CLI_HOME` overrides the data dir); prints the udev rules hint on
  Linux (`src/server.rs` `install`).
- ✅ **SRV-2 Locate and auto-start**: `--server-bin`/`SALEAE_SERVER_BIN`, then the installed one, then `PATH`. Any
  command that needs the server starts it detached (own process group, log in the data dir) when nothing answers
  on a local `--addr` (default `127.0.0.1:10430`), unless `--no-launch`; `--sim-only` starts it without USB
  scanning (`Conn::session`).
- ✅ **SRV-3 start / stop / status / path**: `server start [--foreground]`, `server stop` (SIGTERM to the
  server's `launch_pid` after checking `/proc/PID/comm`, waits until it is gone), `server status`, `server path`.
- 📋 **SRV-4 Verify downloads**: no checksum or signature check of the downloaded zip yet (Saleae publishes none);
  pin a SHA-256 per known build in the CLI.
- 🔍 **SRV-5 macOS and Windows**: builds and download names are mapped, `server stop` uses `kill`/`taskkill`; not
  run on either platform.

## Capture (`CAP`)

- ✅ **CAP-1 Devices**: `saleae devices [--real]` lists real and simulated devices (F4241 Logic Pro 16, F4244 Logic
  Pro 8, F4243 Logic 8); verified with a real Logic 8 (7 Oct 2026).
- ✅ **CAP-2 Timed capture**: `saleae capture` with `-D`/`-A` channel lists, `-r`/`--analog-rate`, `-V` threshold
  (Pro only; Logic 8 rejects one, so it is left out there), `-t`, `--trim`, `--glitch CH=WIDTH`, `--buffer-mb`;
  the first real device by default, an error listing the simulated ones when none is connected
  (`src/capture.rs`). Verified on simulated devices and a real Logic 8.
- ✅ **CAP-3 Digital trigger**: `--trigger CH[:rising|falling|pulse-high|pulse-low]`, `--after`, `--min-pulse`,
  `--max-pulse`, `--link CH=high|low`, `--timeout` (stops and keeps the capture, reported as `trigger_timeout`).
  Verified on simulated devices.
- 📋 **CAP-4 Logic MSO**: the server supports MSO (scope channels, probe ports, analog triggers, API 1.2); the CLI
  refuses MSO devices for now (device type matched by number, `DEVICE_TYPE_LOGIC_MSO`). Needs
  `MsoDeviceConfiguration`, `MsoChannels` exports and channel-select settings, and the API 1.2 proto, which Saleae
  has not published on GitHub yet: build with `SALEAE_PROTO_DIR` pointing at the server zip's `proto` dir, or
  wait for an upstream release and bump the pin in `build.rs`.
- 💡 **CAP-5 Manual capture**: start now, stop with a command (`ManualCaptureMode` + `StopCapture`), for "capture
  until I say stop" sessions.
- 💡 **CAP-6 Sample-rate help**: pick a valid digital/analog rate pair from the device instead of failing with the
  server's list.

## Analyzers (`ANA`)

- 🧪 **ANA-1 Protocol shorthands**: `spi`, `i2c`, `serial` (`uart`), `can`, `lin`, `onewire` with typed flags,
  turned into Logic 2 setting names and option texts, `--set KEY=VALUE` to override (`src/analyzers.rs`). Setting
  names are accepted by the server for all six; decoded output only seen on simulated noise so far.
- ✅ **ANA-2 Any analyzer**: `analyzer add ... other NAME --set ...` for the other bundled analyzers;
  `analyzer list`, `analyzer remove`. A wrong setting name makes the server list the valid ones.
- 📋 **ANA-3 High level analyzers**: `AddHighLevelAnalyzer` (extension directory, settings) is not wrapped.
- 💡 **ANA-4 More shorthands**: I2S, SWD, Manchester, Modbus.

## Export (`EXP`)

- ✅ **EXP-1 Raw export**: `export raw` (CSV or `--binary`, channel selection, `--downsample`, `--iso`), and
  `capture --export-raw DIR`.
- ✅ **EXP-2 Data table**: `export table` for one or more analyzers, `--radix`, `--column`, `--filter`.
- ✅ **EXP-3 .sal files**: `save`, `capture --save`, `decode --save`, `load`.
- 📋 **EXP-4 Legacy analyzer export**: `LegacyExportAnalyzer` (the analyzer-specific text export) is not wrapped.

## Decode and summaries (`DEC`)

- 🧪 **DEC-1 decode**: capture + analyzer + summary + close in one command, capture options before or after the
  protocol, `--keep`, `--save`, `--csv`, `--limit`, `--json` (`main.rs` `Cmd::Decode`). Works end to end against
  simulated devices and a real Logic 8 with nothing wired; not yet on real bus traffic.
- 🧪 **DEC-2 Summaries**: I2C transactions with repeated starts, NAKs and an address census; SPI per chip-select
  window; serial as text lines or hex rows with error counts; CAN frames; any other analyzer as rows
  (`src/summary.rs`). Frame types and columns follow Logic 2's analyzer frames; I2C, serial and CAN are tested on
  hand-written tables only (the simulator's random edges produce no complete frames), SPI `result` columns are
  confirmed by the server.
- ✅ **DEC-3 summarize**: the same summary for an analyzer already added to a capture.

## CLI (`CLI`)

- ✅ **CLI-1 Version**: `--version` prints version, git SHA (`-dirty`) and build time (`build.rs`).
- ✅ **CLI-2 Shell completion**: dynamic, `source <(COMPLETE=bash saleae)` (zsh, fish, elvish, powershell);
  devices from the running server (simulated ones as fallback), capture and analyzer ids from the state file,
  analyzer names; the Bash adapter from wire_weaver_cli quotes values with spaces (`src/complete.rs`).
- ✅ **CLI-3 JSON output**: `--json` on every command, errors as `{"error": ...}` with exit code 1.
- ✅ **CLI-4 State**: captures and analyzers the CLI opened are remembered per server process (`state.json`),
  shown by `saleae status`, used by exports and completion.
- ✅ **CLI-5 Published crate**: `saleae_cli` on crates.io and github.com/romixlab/saleae_cli (MIT or Apache-2.0).
  The proto is not vendored: `build.rs` downloads Saleae's Apache-2.0 `saleae.proto` from the logic2-automation
  repo at a pinned commit (v1.0.11) with a pinned SHA-256 (system `curl`); `SALEAE_PROTO_DIR` for offline builds
  or a newer proto; `DOCS_RS` builds a stub (no network on docs.rs). `cargo package` verified.
- 🧪 **CLI-6 Thin CLI over the library**: `crates/saleae_cli` keeps the same commands, flags, JSON output and
  completions, now as a thin layer (clap parsing, text/JSON formatting) over `crates/saleae_automation`
  (LIB-1); `anyhow` only here, the library's `saleae_automation::Error` everywhere else.

## Python (`PY`)

- 🧪 **PY-1 Python module `saleae_automation`**: `crates/saleae_py` (PyPI name `saleae-automation`; not
  `saleae`, same name clash as LIB-1), PyO3 (abi3, Python >= 3.9) built with maturin:
  `Session` (devices, capture, add_analyzer/remove_analyzer via a settings dict, summarize, save/load/close),
  module functions `install()`/`stop()`, exceptions under `SaleaeError` mapped from `saleae_automation::Error`.
  The typed
  SPI/I2C/... shorthands the CLI has are not wrapped yet; analyzers go through the generic settings-dict path
  (`OtherOptions`). Type stubs (`saleae.pyi`) ship in the wheel. `just test-py` builds it in a venv under /tmp
  and runs the pytest smoke test against the simulated devices (skips with a reason when no server is
  installed); verified passing against a real `saleae server install`.

## Agent skill (`SKILL`)

- 🧪 **SKILL-1 skills/saleae/SKILL.md**: what to ask the user (channels, ground, voltage, speed, timing), decode
  recipes, reading the summaries, reporting. Not yet used on a real debugging session.
- 📋 **SKILL-2 Skill tree**: usage signals and skilltree entries for the `saleae` commands (P2618 task).

## Dropped and superseded

(none yet)
