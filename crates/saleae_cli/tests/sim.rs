//! End to end against the simulated devices of a real headless server: start it, list devices, decode, stop.
//! Skipped (passes with a note) when no server binary is installed (`saleae server install`, or
//! `SALEAE_SERVER_BIN`). Uses its own port and state dir, so it doesn't touch a server already running.

use std::process::Command;

const PORT: &str = "127.0.0.1:10477";

fn saleae(home: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_saleae"))
        .args(["--addr", PORT, "--sim-only"])
        .args(args)
        .env("SALEAE_CLI_HOME", home)
        .output()
        .expect("run saleae");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn decode_on_simulated_device() {
    let home = std::env::temp_dir().join(format!("saleae_cli_test_{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // the server binary from the normal install, unless SALEAE_SERVER_BIN points elsewhere
    if std::env::var_os("SALEAE_SERVER_BIN").is_none() {
        let installed = saleae_rs::server::data_dir()
            .join("server/automation_server")
            .join(saleae_rs::server::SERVER_BIN);
        if !installed.is_file() {
            eprintln!("skipped: no automation server installed (run `saleae server install`)");
            return;
        }
        // SAFETY: single-threaded at this point of the test
        unsafe { std::env::set_var("SALEAE_SERVER_BIN", installed) };
    }

    let (ok, out, err) = saleae(&home, &["--json", "devices"]);
    assert!(ok, "devices failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        v["devices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == "F4241" && d["simulated"] == true),
        "{out}"
    );

    let (ok, out, err) = saleae(
        &home,
        &[
            "decode", "spi", "--clk", "1", "--mosi", "0", "-d", "F4241", "-t", "100ms",
        ],
    );
    assert!(ok, "decode failed: {err}");
    assert!(out.starts_with("SPI on capture"), "{out}");
    assert!(out.contains("frames"), "{out}");

    let (ok, out, err) = saleae(
        &home,
        &[
            "--json", "decode", "uart", "--rx", "0", "-d", "F4243", "-t", "100ms",
        ],
    );
    assert!(ok, "decode failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["device"], "F4243");
    assert!(v["frames"].as_u64().is_some(), "{out}");

    let (ok, _, err) = saleae(&home, &["server", "stop"]);
    assert!(ok, "stop failed: {err}");
    let _ = std::fs::remove_dir_all(&home);
}
