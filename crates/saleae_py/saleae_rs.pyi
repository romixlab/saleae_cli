"""Saleae logic analyzers over the headless Logic 2 automation server (gRPC).

Typed stub for the `saleae_rs` extension module (see `README.md` in this directory for an example).
Mirrors the `saleae_rs` Rust crate's typed API; the CLI (`saleae`, from the `saleae_cli` crate) covers
the same server, but adds shell completion and a few typed analyzer shorthands this binding does not have yet.
"""

from pathlib import Path

DEFAULT_ADDR: str

class SaleaeError(Exception): ...
class NotFoundError(SaleaeError): ...
class InvalidInputError(SaleaeError): ...
class RpcError(SaleaeError): ...
class ServerError(SaleaeError): ...
class IoError(SaleaeError): ...

class Session:
    """A connection to the headless automation server; starts it in the background on first use."""

    app_version: str
    server_pid: int

    def __init__(
        self,
        addr: str | None = None,
        no_launch: bool = False,
        sim_only: bool = False,
        server_bin: str | Path | None = None,
    ) -> None: ...
    def devices(self, real_only: bool = False) -> list[dict]:
        """Real devices, and simulated ones unless `real_only`: `[{"id", "type", "simulated"}, ...]`."""

    def capture(
        self,
        device: str | None = None,
        digital: list[int] | None = None,
        analog: list[int] | None = None,
        rate: float = 10e6,
        analog_rate: float = 1.5625e6,
        threshold: float | None = None,
        duration: float = 1.0,
        buffer_mb: int = 0,
    ) -> dict:
        """Records a timed capture; returns `{"capture", "device", "desc", "end"}`."""

    def add_analyzer(
        self,
        capture: int,
        name: str,
        settings: dict | None = None,
        channels: list[int] | None = None,
        label: str | None = None,
    ) -> int:
        """Adds an analyzer by its Logic 2 name, with a settings dict; returns the analyzer id."""

    def remove_analyzer(self, capture: int, analyzer: int) -> None: ...
    def summarize(
        self,
        capture: int,
        analyzer: int,
        kind: str = "other",
        limit: int = 40,
        csv: str | Path | None = None,
        mosi: bool = True,
        miso: bool = True,
    ) -> dict:
        """`kind`: `"i2c"`, `"spi"`, `"serial"`, `"can"` or `"other"`. Returns `{"text", "json"}`."""

    def save(self, capture: int, file: str | Path) -> None: ...
    def load(self, file: str | Path) -> int: ...
    def close(self, capture: int) -> None: ...

def install(
    build: str | None = None, url: str | None = None, zip: str | Path | None = None
) -> Path:
    """Installs the headless automation server for this platform; returns the server binary path."""

def stop(pid: int) -> None:
    """Ends the server with the given pid, after checking that it is the automation server."""
