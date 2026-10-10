# saleae_cli: the Saleae logic analyzer library, CLI and Python module.

py_venv := "/tmp/saleae_py-venv"

default:
    @just --list

# Tests, Rust and Python (tests/sim.rs and the Python smoke test skip without `saleae server install`)
test: test-py
    cargo test --workspace --quiet

# Build the Python module (crates/saleae_py) into a venv under /tmp and run its pytest smoke test
[working-directory('crates/saleae_py')]
test-py:
    UV_PROJECT_ENVIRONMENT={{ py_venv }} uv run --quiet --group dev pytest -q

# Format check and clippy without warnings, all crates
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

# Build and install saleae on this PC; completion: source <(COMPLETE=bash saleae)
install:
    cargo install --quiet --path crates/saleae_cli --locked

# Bring this PC up to main
deploy:
    tpm repos pull saleae_rs
    just install
