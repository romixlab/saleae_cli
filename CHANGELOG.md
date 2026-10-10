# Changelog

All notable changes to saleae_cli are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). [FEATURES.md](FEATURES.md) holds the current status of
every feature; IDs in parentheses refer to it.

## [Unreleased]

### Changed

- The library crate is `saleae_rs` (was `saleae_automation`, easy to mix up with Saleae's official Python
  `saleae.automation` from logic2-automation); the Python package is `saleae-rs` (`import saleae_rs`); the repo is
  github.com/romixlab/saleae_rs (was saleae_cli). The `saleae` binary and its crate `saleae_cli` keep their names.

## [0.4.0] - 2026-10-10

### Added

- `saleae_automation` library crate (not `saleae`: that crates.io name is someone else's older, unrelated
  package): server lifecycle, typed capture/analyzer/export calls and results, summaries, errors as an enum
  (LIB-1).
- `saleae_automation` Python module (`crates/saleae_py`, PyPI name `saleae-automation`,
  `pip install ./crates/saleae_py`): `Session` (devices, capture, add_analyzer/remove_analyzer, summarize,
  save/load/close), `install()`/`stop()`, type stubs (PY-1).

### Changed

- The repo is a Cargo workspace: `crates/saleae_automation` (library), `crates/saleae_cli` (the `saleae`
  binary, same commands and output), `crates/saleae_py`; install with `cargo install --path crates/saleae_cli`
  or `just install` (LIB-1, CLI-6).

### Docs

- Skill: wire every channel as a twisted pair with its own ground, with a photo of such leads and the numbers
  from the 9 Oct 2026 cross-check bench.

## [0.3.0] - 2026-10-07

### Added

- Crate metadata for crates.io (`saleae_cli`, repository, keywords, categories), `LICENSE-MIT` and
  `LICENSE-APACHE`, a README section on the API license and related crates (CLI-5).
- `SALEAE_PROTO_DIR=DIR` env var: build from `DIR/saleae/grpc/saleae.proto` (offline, or the API 1.2 proto from
  the server zip); `DOCS_RS` builds a stub binary without the proto (CLI-5).

### Changed

- The gRPC API is no longer vendored: `build.rs` downloads Saleae's Apache-2.0 `saleae.proto` from the
  `saleae/logic2-automation` GitHub repo at a pinned commit (tag v1.0.11, API 1.0.0) and checks a pinned SHA-256
  with the system `curl`. The published proto is API 1.0, so the Logic MSO device type is now recognised by its
  number only; MSO devices are still refused (CLI-5, CAP-4).
- The command tree moved from `src/main.rs` to `src/cli.rs`; `main.rs` only wires the modules.
- The agent skill tells how to install from crates.io instead of from a local checkout (SKILL-1).

### Removed

- `proto/` (the vendored API 1.2 proto and its README) (CLI-5).

## [0.2.0] - 2026-10-07

### Added

- `saleae` CLI for Saleae Logic 8 / Pro 8 / Pro 16 over the headless Logic 2 automation server (gRPC, vendored
  `saleae.proto` API 1.2.0), pure Rust (tonic, prost, protox).
- `saleae server install|start|stop|status|path`: downloads Saleae's preview server (build `407561e0`, server
  2.4.45-insider.1) into `~/.local/share/saleae_cli`; any command starts it in the background when none is
  running (SRV-1, SRV-2, SRV-3).
- `saleae devices`, real and simulated (CAP-1); `saleae capture` with timed or digital trigger mode, digital and
  analog channels, threshold, glitch filter (CAP-2, CAP-3).
- `saleae analyzer add|remove|list` with typed shorthands for SPI, I2C, Async Serial, CAN, LIN, 1-Wire and any
  other bundled analyzer via `--set` (ANA-1, ANA-2).
- `saleae export raw|table`, `save`, `load`, `close`, `status` (EXP-1, EXP-2, EXP-3, CLI-4).
- `saleae decode`: capture, analyze and print a compact summary (I2C transactions, SPI per chip select, serial
  text, CAN frames) in one command; `saleae summarize` for an existing analyzer (DEC-1, DEC-2, DEC-3).
- `--json` on every command, `--version` with git SHA and build time, dynamic shell completion
  (`source <(COMPLETE=bash saleae)`) (CLI-1, CLI-2, CLI-3).
- Agent skill `skills/saleae/SKILL.md` (SKILL-1).
