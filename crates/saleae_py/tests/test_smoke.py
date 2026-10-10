"""Smoke test against the simulated devices (no real hardware needed), skipped if the automation server is not
installed (`saleae server install`, see AGENTS.md)."""

import os
import socket
import tempfile
from pathlib import Path

import pytest

import saleae_automation as saleae


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture
def session():
    addr = f"127.0.0.1:{_free_port()}"
    home = tempfile.mkdtemp(prefix="saleae-py-test-")
    os.environ["SALEAE_CLI_HOME"] = home
    # same override the CLI's --server-bin / SALEAE_SERVER_BIN reads, so this test can reuse an existing
    # install (`saleae server install`) without a fresh SALEAE_CLI_HOME hiding it.
    server_bin = os.environ.get("SALEAE_SERVER_BIN")
    try:
        s = saleae.Session(addr=addr, sim_only=True, server_bin=server_bin)
    except saleae.SaleaeError as e:
        pytest.skip(f"automation server not available: {e}")
        return
    yield s
    saleae.stop(s.server_pid)


def test_devices_lists_simulated(session):
    devices = session.devices()
    assert any(d["simulated"] for d in devices)


def test_capture_analyze_and_summarize(session):
    devices = session.devices()
    sim = next(d for d in devices if d["simulated"])
    rec = session.capture(device=sim["id"], digital=[0, 1], duration=0.1)
    assert rec["end"] == "completed"

    analyzer = session.add_analyzer(rec["capture"], "I2C", settings={"SDA": 0, "SCL": 1})
    summary = session.summarize(rec["capture"], analyzer, kind="i2c")
    assert "frames" in summary["json"]
    assert isinstance(summary["text"], str)

    session.close(rec["capture"])


def test_save_and_load_roundtrip(session, tmp_path: Path):
    devices = session.devices()
    sim = next(d for d in devices if d["simulated"])
    rec = session.capture(device=sim["id"], digital=[0, 1], duration=0.05)
    sal = tmp_path / "capture.sal"
    session.save(rec["capture"], sal)
    assert sal.exists()
    loaded = session.load(sal)
    assert isinstance(loaded, int)
    session.close(rec["capture"])
    session.close(loaded)


def test_bad_setting_type_raises(session):
    devices = session.devices()
    sim = next(d for d in devices if d["simulated"])
    rec = session.capture(device=sim["id"], digital=[0, 1], duration=0.05)
    with pytest.raises(saleae.InvalidInputError):
        session.add_analyzer(rec["capture"], "I2C", settings={"SDA": object()})
    session.close(rec["capture"])
