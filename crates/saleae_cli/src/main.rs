//! `saleae`: drive Saleae logic analyzers through the headless Logic 2 automation server (gRPC).
//!
//! The command tree and handlers live in `cli.rs`; this file only wires the modules, so that a build without the
//! Saleae proto (docs.rs has no network: `cfg(saleae_cli_stub)`, see `build.rs`) still compiles.

#[cfg(not(saleae_cli_stub))]
mod analyzer_args;
#[cfg(not(saleae_cli_stub))]
mod capture_args;
#[cfg(not(saleae_cli_stub))]
mod cli;
#[cfg(not(saleae_cli_stub))]
mod complete;
#[cfg(not(saleae_cli_stub))]
mod parse;

#[cfg(not(saleae_cli_stub))]
fn main() {
    cli::main()
}

#[cfg(saleae_cli_stub)]
fn main() {
    eprintln!("saleae: built without the Saleae automation API (stub build, see build.rs)");
    std::process::exit(2);
}
