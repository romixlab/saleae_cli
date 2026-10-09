# saleae_cli

`saleae`: a Rust CLI and agent skill for Saleae logic analyzers via Saleae's headless Logic 2 automation server
(gRPC), no Logic 2 GUI needed.

```sh
cargo install saleae_cli                   # or `cargo install --path .` in a checkout
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
