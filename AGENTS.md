# Working on saleae_cli

Guidance for AI agents and contributors. Read this before changing code.

saleae_cli is the `saleae` Rust library, the `saleae` command line tool and agent skill on top of it, and the
`saleae` Python module, for Saleae logic analyzers (Logic 8, Logic Pro 8/16; Logic MSO planned). It talks gRPC to
Saleae's headless automation server (`logic_automation_server`, a preview Saleae ships since Aug 2026 with the
same API as the Logic 2 GUI's automation, no GUI needed):
https://discuss.saleae.com/t/headless-logic2-automation-support/3793 and, for MSO,
https://discuss.saleae.com/t/headless-logic2-automartion-support-for-logic-mso/3798. docs.saleae.com still says
there is no headless mode; the forum posts are right. Python API reference (same concepts):
https://saleae.github.io/logic2-automation/.

## FEATURES.md is the source of truth

[FEATURES.md](FEATURES.md) lists every feature with its status, every known bug, and what is planned, with stable
IDs per area (`LIB`, `SRV`, `CAP`, `ANA`, `EXP`, `DEC`, `CLI`, `PY`, `SKILL`).

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

Cargo workspace, edition 2024, one version for all crates (`[workspace.package]`), the same layout as the other
instrument tools (tpm CLAUDE.md "Code repos": tools are a library first):

- `crates/saleae_automation` — the library, the typed API; everything that talks gRPC lives here (not `saleae`:
  that crates.io name is someone else's older, unrelated package, the legacy Logic 1 socket API). Errors are
  `saleae_automation::Error` (thiserror), never `anyhow`.
  - `build.rs` — downloads Saleae's `saleae.proto` (Apache-2.0, from the `saleae/logic2-automation` GitHub repo
    at a pinned commit, checked against a pinned SHA-256, system `curl`) into `OUT_DIR` and compiles it with
    `protox` (pure Rust, no `protoc`) into a tonic client (`saleae_automation::pb`). `SALEAE_PROTO_DIR=DIR` uses
    `DIR/saleae/grpc/saleae.proto` instead (offline; or the API 1.2 proto from the server zip for MSO work);
    `DOCS_RS` builds the crate with no modules (`cfg(saleae_stub_proto)`) since docs.rs has no network. Nothing
    of Saleae's is committed; the published proto is API 1.0.0, so API 1.2 things (Logic MSO, `SetReporting`)
    are matched by number or left out, and the code must build against both protos (wildcard match arms).
  - `server.rs` — install, locate, start/stop the server, connect (`Conn::session`, with an optional progress
    callback), and the state file that remembers captures/analyzers per server pid.
  - `device.rs` — device listing and resolution (`DEVICE_TYPE_LOGIC_MSO`, `probe_quick` for completion).
  - `capture.rs` — capture options (`CaptureOptions`) to `StartCaptureRequest`, running a capture to its end,
    save/load/close.
  - `analyzer.rs` — protocol options (`Protocol`) to analyzer names and settings, add/remove. Setting names and
    option texts must match the analyzer plugins exactly; the server's error lists valid setting names, and
    `strings Analyzers/lib<name>_analyzer.so` shows the option texts.
  - `export.rs` — raw and data-table export.
  - `summary.rs` — compact summaries of the analyzer data tables (what `decode` prints).
  - `decode.rs` — the one-shot flows combining the above: `decode` (capture + analyzer + summary + optional
    save/close) and `summarize_analyzer`.
- `crates/saleae_cli` — the thin `saleae` binary (same name is fine; it's the binary inside this crate, not a
  separate crates.io package): `src/cli.rs` the command tree (clap derive) and handlers,
  `--json` output through `Out::print`; `src/capture_args.rs` / `src/analyzer_args.rs` turn clap args into the
  library's `CaptureOptions` / `Protocol`; `src/parse.rs` duration/rate/channel-list/setting string parsing;
  `src/complete.rs` dynamic shell completion, its Bash adapter copied from `wire_weaver_cli` (keep in sync);
  `build.rs` `GIT_SHA` and `BUILD_TIME` for `--version` (its own `DOCS_RS` stub cfg, `saleae_cli_stub`, since
  build-script cfgs don't cross crates). `anyhow` only here.
- `crates/saleae_py` — the `saleae_automation` Python module (also not `saleae`: PyPI has the same name clash),
  PyO3, abi3 for Python >= 3.9, built by maturin from `pyproject.toml`; `publish = false` on crates.io, it ships
  as a wheel: `Session` (devices, capture, add_analyzer/remove_analyzer with a settings dict, summarize,
  save/load/close), module functions `install()` / `stop()`, exceptions under `SaleaeError` mapped from
  `saleae_automation::Error`. The typed SPI/I2C/... shorthands the CLI has are not wrapped yet (CLI-6); analyzers
  go through the generic settings-dict path. `saleae_automation.pyi` (type stubs and docstrings, shipped by
  maturin) must match `src/lib.rs`; `tests/test_smoke.py` is the pytest smoke test.
- `skills/saleae/SKILL.md` — the agent skill. Update it when commands or their output change.
- `crates/saleae_cli/tests/sim.rs` — end to end against the server's simulated devices.

## Commands

```sh
cargo build
cargo run -p saleae_cli -- devices      # starts the server in the background on first use
cargo run -p saleae_cli -- decode i2c --sda 0 --scl 1 -d F4241 -t 200ms
cargo run -p saleae_cli -- server stop
just lint                               # fmt check + clippy -D warnings on every crate
just test                               # cargo test --workspace + the Python smoke test
just test-py                            # builds the Python module into /tmp/saleae_py-venv (uv + maturin), runs pytest
just install                            # puts `saleae` in ~/.cargo/bin
```

Before declaring a change done: `just lint` and `just test`. `tests/sim.rs` and the Python smoke test need
`saleae server install` (else they skip, printing why). Run new or changed commands against the simulated
devices (F4241 Logic Pro 16, F4244 Logic Pro 8, F4243 Logic 8); they produce random edges, not protocol traffic,
so decoded content can only be checked with a real device on a real bus. Say so when a change couldn't be
checked that way.

## Hardware and server safety

- Captures only read inputs, so running one on a connected real device is harmless; never change udev rules,
  drivers or anything needing sudo without asking: print the command for the user.
- Use a separate `--addr` port (and `SALEAE_CLI_HOME` for a separate state dir) when testing, so you don't stop
  or confuse a server the user is using. `saleae server stop` only kills a process named like the server.
- The server is Saleae's closed-source preview build under their license (`License.txt` in the zip: no
  redistribution of the software, no reverse engineering). Don't commit the server, its analyzers, its proto or
  `.sal` internals, and don't decode Saleae's internal formats; the published Apache-2.0 proto is the API.

## Dependencies

Pure Rust only (tpm "Code repos" rule): the library has tonic without TLS (the server is local, plain HTTP/2),
prost, protox instead of `protoc`, zip with the zlib-rs backend only, sha2 for the proto checksum, thiserror; the
CLI anyhow, clap and clap_complete; the Python crate pyo3. Downloads (the server zip at run time, the proto at
build time) use the system `curl` rather than a TLS stack in the binary or build script (rustls needs ring or
aws-lc, both with C/assembly). Check new dependencies with `cargo tree` for `-sys` crates and `cc`/`bindgen`
build deps (today: only `dirs-sys` and `linux-raw-sys`, both pure Rust).

## Publishing

Crates `saleae_automation` (library) and `saleae_cli` (binary `saleae`) on crates.io, repo
`github.com/romixlab/saleae_cli`. The Python wheel `saleae-automation` (module `saleae_automation`) from
`crates/saleae_py` (maturin, abi3) is not published yet. `cargo package` must pass
for both crates (it builds the packaged crate, so the proto download must work); `tpm land` bumps the version,
the user runs `cargo publish` (library first, the CLI's path dependency needs the published version). Public
repo: nothing private in commits, docs or the skill (no internal hosts, paths, ids).

## Code conventions

- Errors: `saleae_automation::Error` (thiserror) in the library, one variant per case a caller may tell apart
  (not found, invalid input, an RPC failure with the server's own message, I/O, ...); `anyhow` with
  `.context(...)` only in the CLI; in Python each `Error` variant maps to a `SaleaeError` subclass (`to_py` in
  `saleae_py`). No `unwrap`/`expect` on data from the server or the user.
- The CLI stays thin: new behaviour goes into the library first, then the CLI and the Python module call it.
- Every command supports `--json`; keep text output short and line-oriented (agents read it).
- Paths sent to the server must be absolute (`saleae_automation::export::abs`, used internally): the server runs
  in its own working directory.

## Tests

- Pure logic (parsing, analyzer settings, summaries): Rust unit tests next to the code, in the crate that owns
  it (string parsing in `saleae_cli`, everything else in `saleae_automation`). A summary change gets a test with
  a data table in Logic 2's CSV format.
- Server behaviour: `crates/saleae_cli/tests/sim.rs` (own port and state dir).
- The Python module: `crates/saleae_py/tests/test_smoke.py` (pytest, against the simulated devices; skips with a
  reason when no server is installed).
- A bug fix starts with a failing test when the bug can be reproduced in one.

## Commits

Conventional Commits with a scope: `feat(decode): ...`, `fix(server): ...`, scopes `lib`, `server`, `capture`,
`analyzer`, `export`, `decode`, `cli`, `py`, `skill`, `build`. Short imperative summary, blank line, body with
what and why; reference feature IDs.

Commit on your own initiative on a session branch (`tpm work new SLUG`), each finished step one commit with
FEATURES.md and CHANGELOG.md updated in it. When the work is done and the user agrees, `tpm land` puts it on main.

## Versions

Landing bumps the version, not each work commit (tpm CLAUDE.md "Landing bumps the version"): session commits add
CHANGELOG entries under `[Unreleased]` without bumping; `tpm land` makes the `release: x.y.z` commit.
`saleae --version` prints version, git SHA and build time (`build.rs`, no extra crates).
