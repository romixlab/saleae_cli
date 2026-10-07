# Working on saleae_cli

Guidance for AI agents and contributors. Read this before changing code.

saleae_cli is the `saleae` command line tool and agent skill for Saleae logic analyzers (Logic 8, Logic Pro 8/16;
Logic MSO planned). It talks gRPC to Saleae's headless automation server (`logic_automation_server`, a preview
Saleae ships since Aug 2026 with the same API as the Logic 2 GUI's automation, no GUI needed):
https://discuss.saleae.com/t/headless-logic2-automation-support/3793 and, for MSO,
https://discuss.saleae.com/t/headless-logic2-automartion-support-for-logic-mso/3798. docs.saleae.com still says
there is no headless mode; the forum posts are right. Python API reference (same concepts):
https://saleae.github.io/logic2-automation/.

## FEATURES.md is the source of truth

[FEATURES.md](FEATURES.md) lists every feature with its status, every known bug, and what is planned, with stable
IDs per area (`SRV`, `CAP`, `ANA`, `EXP`, `DEC`, `CLI`, `SKILL`).

- **Read the relevant area before starting.** A "new" bug or idea is often already recorded.
- **Name IDs with a short slug when talking to the user**: `CAP-4 mso-capture`, never a bare `CAP-4`. Commit
  messages, CHANGELOG and code `TODO`s keep the bare ID.
- **Update it in the same commit** as the code. New ids: `tpm work claim AREA` prints the next free one. Never
  renumber or reuse IDs.
- Don't track status anywhere else. Code `TODO`s that matter reference an ID: `// TODO(CAP-4): ...`.

## CHANGELOG.md records every change

[CHANGELOG.md](CHANGELOG.md) is the history, FEATURES.md the current state; keep both.

- Every change a user would notice gets an entry under `## [Unreleased]` in the same commit: `### Added`,
  `### Changed`, `### Fixed`, `### Removed`. Short and user-facing, with the feature ID in parentheses.
- Also record a new default server build (`DEFAULT_BUILD`), a new proto pin in `build.rs`, and new env vars.
- Pure refactors and typo fixes don't need an entry.

## Layout

Single crate, edition 2024, binary `saleae`.

- `build.rs` — downloads Saleae's `saleae.proto` (Apache-2.0, from the `saleae/logic2-automation` GitHub repo at
  a pinned commit, checked against a pinned SHA-256, system `curl`) into `OUT_DIR` and compiles it with `protox`
  (pure Rust, no `protoc`) into a tonic client (`crate::pb`). `SALEAE_PROTO_DIR=DIR` uses
  `DIR/saleae/grpc/saleae.proto` instead (offline; or the API 1.2 proto from the server zip for MSO work);
  `DOCS_RS` builds a stub binary (`cfg(saleae_stub_proto)`) since docs.rs has no network. Nothing of Saleae's is
  committed; the published proto is API 1.0.0, so API 1.2 things (Logic MSO, `SetReporting`) are matched by number
  or left out, and the code must build against both protos (wildcard match arms).
- `src/main.rs` — wires the modules (all behind `cfg(not(saleae_stub_proto))`); `src/cli.rs` — the command tree
  (clap derive) and the small command handlers; `--json` output goes through `Out::print`.
- `src/server.rs` — install, locate, start/stop the server, connect (`Conn::session`), and the state file that
  remembers captures/analyzers per server pid.
- `src/capture.rs` — capture options (`CaptureArgs`) to `StartCaptureRequest`, device selection, waiting.
- `src/analyzers.rs` — protocol shorthands (`Protocol`) to analyzer names and settings. Setting names and option
  texts must match the analyzer plugins exactly; the server's error lists valid setting names, and
  `strings Analyzers/lib<name>_analyzer.so` shows the option texts.
- `src/summary.rs` — compact summaries of the analyzer data tables (what `decode` prints).
- `src/complete.rs` — dynamic shell completion; its Bash adapter is copied from `wire_weaver_cli` (keep in sync).
- `skills/saleae/SKILL.md` — the agent skill. Update it when commands or their output change.
- `tests/sim.rs` — end to end against the server's simulated devices.

## Commands

```sh
cargo build
cargo run -- devices                    # starts the server in the background on first use
cargo run -- decode i2c --sda 0 --scl 1 -d F4241 -t 200ms
cargo run -- server stop
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo test                              # tests/sim.rs needs `saleae server install` (else it skips)
cargo install --path .                  # puts `saleae` in ~/.cargo/bin
```

Before declaring a change done: build, clippy without warnings, fmt, tests. Run new or changed commands against
the simulated devices (F4241 Logic Pro 16, F4244 Logic Pro 8, F4243 Logic 8); they produce random edges, not
protocol traffic, so decoded content can only be checked with a real device on a real bus. Say so when a change
couldn't be checked that way.

## Hardware and server safety

- Captures only read inputs, so running one on a connected real device is harmless; never change udev rules,
  drivers or anything needing sudo without asking: print the command for the user.
- Use a separate `--addr` port (and `SALEAE_CLI_HOME` for a separate state dir) when testing, so you don't stop
  or confuse a server the user is using. `saleae server stop` only kills a process named like the server.
- The server is Saleae's closed-source preview build under their license (`License.txt` in the zip: no
  redistribution of the software, no reverse engineering). Don't commit the server, its analyzers, its proto or
  `.sal` internals, and don't decode Saleae's internal formats; the published Apache-2.0 proto is the API.

## Dependencies

Pure Rust only (tpm "Code repos" rule): tonic without TLS (the server is local, plain HTTP/2), prost, protox
instead of `protoc`, zip with the zlib-rs backend only, sha2 for the proto checksum. Downloads (the server zip at
run time, the proto at build time) use the system `curl` rather than a TLS stack in the binary or build script
(rustls needs ring or aws-lc, both with C/assembly). Check new dependencies with `cargo tree` for `-sys` crates
and `cc`/`bindgen` build deps (today: only `dirs-sys` and `linux-raw-sys`, both pure Rust).

## Publishing

The crate is `saleae_cli` on crates.io (binary `saleae`), repo `github.com/romixlab/saleae_cli`. `cargo package`
must pass (it builds the packaged crate, so the proto download must work); `tpm land` bumps the version, the user
runs `cargo publish`. Public repo: nothing private in commits, docs or the skill (no internal hosts, paths, ids).

## Code conventions

- Errors: `anyhow` with `.context(...)`; gRPC errors through `rpc_error` so the server's message (which usually
  says exactly what is wrong) reaches the user. No `unwrap`/`expect` on data from the server or the user.
- Every command supports `--json`; keep text output short and line-oriented (agents read it).
- Paths sent to the server must be absolute (`abs()`): the server runs in its own working directory.

## Tests

- Pure logic (parsing, analyzer settings, summaries): unit tests next to the code. A summary change gets a test
  with a data table in Logic 2's CSV format.
- Server behaviour: `tests/sim.rs` (own port and state dir).
- A bug fix starts with a failing test when the bug can be reproduced in one.

## Commits

Conventional Commits with a scope: `feat(decode): ...`, `fix(server): ...`, scopes `server`, `capture`,
`analyzer`, `export`, `decode`, `cli`, `skill`, `build`. Short imperative summary, blank line, body with what and
why; reference feature IDs.

Commit on your own initiative on a session branch (`tpm work new SLUG`), each finished step one commit with
FEATURES.md and CHANGELOG.md updated in it. When the work is done and the user agrees, `tpm land` puts it on main.

## Versions

Landing bumps the version, not each work commit (tpm CLAUDE.md "Landing bumps the version"): session commits add
CHANGELOG entries under `[Unreleased]` without bumping; `tpm land` makes the `release: x.y.z` commit.
`saleae --version` prints version, git SHA and build time (`build.rs`, no extra crates).
