# Changelog

All notable changes to saleae_cli are recorded here, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). [FEATURES.md](FEATURES.md) holds the current status of
every feature; IDs in parentheses refer to it.

## [Unreleased]

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
