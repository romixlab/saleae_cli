# saleae_cli

Drive Saleae logic analyzers via Saleae's headless Logic 2 automation server (gRPC), no Logic 2 GUI needed: a
Rust library, a CLI and agent skill on top, and a Python module for notebooks and test benches.

| crate | what |
|---|---|
| [`crates/saleae_automation`](crates/saleae_automation) | the library: server lifecycle, typed capture/analyzer/export calls, summaries |
| [`crates/saleae_cli`](crates/saleae_cli) | the `saleae` command line tool |
| [`crates/saleae_py`](crates/saleae_py) | the `saleae_automation` Python module (PyO3 + maturin) |

```sh
cargo install --path crates/saleae_cli     # or `cargo install saleae_cli` once published
saleae server install                      # Saleae's preview server for this platform (Linux/macOS/Windows)
saleae devices                             # real devices, plus simulated F4241 / F4244 / F4243
saleae decode i2c --sda 0 --scl 1 -t 500ms # capture + I2C analyzer + compact summary
saleae decode uart --rx 2 --baud 115200 --trigger 2:falling --timeout 30s
saleae --json decode spi --clk 1 --mosi 0 --miso 2 --cs 3 -r 50M
saleae server stop
source <(COMPLETE=bash saleae)             # shell completion (zsh, fish too)
```

Linux, real devices without root: `sudo cp ~/.local/share/saleae_cli/server/automation_server/99-SaleaeLogic.rules
/etc/udev/rules.d/`, then replug.

## Python

```sh
pip install ./crates/saleae_py             # builds with maturin; needs Rust
```

```python
import saleae_automation as saleae

s = saleae.Session(sim_only=True)                       # starts the server in the background on first use
sim = next(d for d in s.devices() if d["simulated"])     # F4241 / F4244 / F4243 without real hardware
rec = s.capture(device=sim["id"], digital=[0, 1], duration=0.5)
analyzer = s.add_analyzer(rec["capture"], "I2C", settings={"SDA": 0, "SCL": 1})
print(s.summarize(rec["capture"], analyzer, kind="i2c")["text"])
s.close(rec["capture"])
```

`saleae_automation` (not `saleae`: that PyPI name is someone else's older, unrelated package, the legacy Logic 1
socket API). Errors are `SaleaeError` subclasses (`NotFoundError`, `InvalidInputError`, `RpcError`, ...); type
stubs (`saleae_automation.pyi`) ship with the module. The typed SPI/I2C/... shorthands the CLI has are not
wrapped yet; analyzers go through `add_analyzer`'s settings dict (setting names as Logic 2 shows them, `saleae
analyzer list`).

## Rust

```rust
let conn = saleae_automation::server::Conn::default();
let mut session = conn.session(None).await?;             // starts the server in the background on first use
let devices = saleae_automation::device::list(&mut session, false).await?;
```

(the crate is `saleae_automation`, not `saleae`: that crates.io name is the same older, unrelated package)

- Agent skill: [skills/saleae/SKILL.md](skills/saleae/SKILL.md)
- Features and status: [FEATURES.md](FEATURES.md); changes: [CHANGELOG.md](CHANGELOG.md); dev rules:
  [AGENTS.md](AGENTS.md)

## API, license and related crates

The gRPC API (`saleae.proto`, Apache-2.0, Copyright 2022 Saleae) is fetched at build time from
[saleae/logic2-automation](https://github.com/saleae/logic2-automation) at a pinned commit (tag `v1.0.11`, API
1.0.0) and checked against a pinned SHA-256, with the system `curl`; nothing of Saleae's is in this repo. `SALEAE_PROTO_DIR=DIR` builds from `DIR/saleae/grpc/saleae.proto`
instead, offline or with the newer API 1.2 proto that ships in Saleae's server zip (`automation_server/proto`).
The server itself is Saleae's closed-source preview, downloaded by `saleae server install` under Saleae's license.

saleae_cli is licensed under MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)).

Related: [saleae-logic2-automation-mcp](https://crates.io/crates/saleae-logic2-automation-mcp) exposes the same
automation API as MCP tools for the Logic 2 GUI; [saleae-importer](https://crates.io/crates/saleae-importer) and
[saleae-csv](https://crates.io/crates/saleae-csv) read Logic 2 binary and CSV exports; the old
[saleae](https://crates.io/crates/saleae) crate (2020) talked to the legacy Logic 1 socket API.
