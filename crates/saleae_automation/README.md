# saleae_automation

Typed Rust API for Saleae logic analyzers (Logic 8, Logic Pro 8/16) over the headless Logic 2 automation server
(gRPC): install/locate/start/stop the server, run captures, add analyzers, export data and summarize analyzer
results.

This is the library behind the [`saleae`](https://crates.io/crates/saleae_cli) CLI
(`cargo install saleae_cli`); see the workspace [README](../../README.md) for the full picture, and
[AGENTS.md](../../AGENTS.md) / [FEATURES.md](../../FEATURES.md) for how the pieces fit together.

```text
let conn = saleae_automation::server::Conn::default();
let mut session = conn.session(None).await?;      // starts the server if none is running
let devices = session.client.get_devices(saleae_automation::pb::GetDevicesRequest {
    include_simulation_devices: true,
}).await?.into_inner().devices;
```

The gRPC API (`saleae.proto`, Apache-2.0, Copyright 2022 Saleae) is fetched at build time and compiled with
`protox`; see the workspace README for details and the `SALEAE_PROTO_DIR` / `DOCS_RS` build-time overrides.
