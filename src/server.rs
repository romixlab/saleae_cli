//! The headless automation server: installing it, finding it, starting it in the background and connecting.
//!
//! Captures live inside the server process, so the CLI keeps one server running across commands: the first
//! command that needs it starts it detached (unless `--no-launch`), `saleae server stop` ends it. A small state
//! file remembers the captures and analyzers this CLI created, tied to the server's process id, so later
//! commands (and shell completion) can refer to them.

use crate::pb::GetAppInfoRequest;
use crate::pb::manager_client::ManagerClient;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tonic::transport::Channel;

pub type Client = ManagerClient<Channel>;

pub const DEFAULT_ADDR: &str = "127.0.0.1:10430";
pub const SERVER_BIN: &str = if cfg!(windows) {
    "logic_automation_server.exe"
} else {
    "logic_automation_server"
};

/// Build that `saleae server install` fetches by default: the Logic MSO preview release from
/// <https://discuss.saleae.com/t/headless-logic2-automartion-support-for-logic-mso/3798> (server 2.4.45-insider.1,
/// API 1.2.0).
pub const DEFAULT_BUILD: &str = "407561e0";
pub const DOWNLOAD_BASE: &str = "https://downloads.saleae.com/logic_automation_server";

/// Where the CLI keeps the installed server, its log and its state.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SALEAE_CLI_HOME") {
        return PathBuf::from(d);
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("saleae_cli")
}

fn installed_bin() -> PathBuf {
    data_dir().join("server/automation_server").join(SERVER_BIN)
}

/// Platform part of the release zip name, `None` where Saleae ships no build.
pub fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        _ => return None,
    })
}

pub fn download_url(build: &str) -> Result<String> {
    let p = platform().context("Saleae ships no automation server for this platform")?;
    Ok(format!(
        "{DOWNLOAD_BASE}/{build}/logic_automation_server-{p}.zip"
    ))
}

/// The server binary: `--server-bin`/`SALEAE_SERVER_BIN`, then the one `saleae server install` put in the data
/// dir, then `logic_automation_server` on `PATH`.
pub fn locate(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        bail!("server binary {} does not exist", p.display());
    }
    let installed = installed_bin();
    if installed.is_file() {
        return Ok(installed);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(SERVER_BIN);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    bail!(
        "Saleae automation server not found: run `saleae server install`, or pass --server-bin / set \
         SALEAE_SERVER_BIN (looked in {} and PATH)",
        installed.display()
    )
}

/// Downloads (with `curl`, so the CLI carries no TLS stack) or takes a local zip, and unpacks it into the data
/// dir, replacing an earlier install. Returns the server binary.
pub fn install(url: &str, zip_file: Option<&Path>) -> Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let zip_path = match zip_file {
        Some(z) => z.to_path_buf(),
        None => {
            let tmp = dir.join("download.zip");
            eprintln!("downloading {url}");
            let status = Command::new("curl")
                .args(["-fL", "--progress-bar", "-o"])
                .arg(&tmp)
                .arg(url)
                .status()
                .context("run curl (needed to download; or pass --zip with a downloaded file)")?;
            if !status.success() {
                bail!("download of {url} failed ({status})");
            }
            tmp
        }
    };
    let server_dir = dir.join("server");
    if server_dir.exists() {
        std::fs::remove_dir_all(&server_dir)
            .with_context(|| format!("remove old install {}", server_dir.display()))?;
    }
    let file =
        std::fs::File::open(&zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("read server zip")?;
    archive
        .extract(&server_dir)
        .with_context(|| format!("unpack into {}", server_dir.display()))?;
    if zip_file.is_none() {
        let _ = std::fs::remove_file(&zip_path);
    }
    let bin = installed_bin();
    if !bin.is_file() {
        bail!("zip did not contain automation_server/{SERVER_BIN}");
    }
    Ok(bin)
}

/// Remembered between CLI invocations: which server we talk to and what we created in it.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    /// `launch_pid` of the server the captures below belong to.
    pub server_pid: u64,
    pub addr: String,
    pub captures: Vec<CaptureRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureRecord {
    pub id: u64,
    pub device: String,
    /// Short description of the capture settings (`D0-3 @ 10 MS/s, 1 s`).
    pub desc: String,
    pub digital: Vec<u32>,
    pub analog: Vec<u32>,
    #[serde(default)]
    pub analyzers: Vec<AnalyzerRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerRecord {
    pub id: u64,
    pub name: String,
    pub label: String,
}

fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

impl State {
    /// The saved state; empty when there is none or it belongs to another server process.
    pub fn load(addr: &str, server_pid: u64) -> State {
        let s: State = std::fs::read(state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if s.server_pid == server_pid && s.addr == addr {
            s
        } else {
            State {
                server_pid,
                addr: addr.to_string(),
                captures: vec![],
            }
        }
    }

    /// The saved state without checking it against a running server (for shell completion).
    pub fn load_unchecked() -> State {
        std::fs::read(state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(data_dir())?;
        std::fs::write(state_path(), serde_json::to_vec_pretty(self)?).context("write state")
    }

    pub fn capture(&self, id: u64) -> Option<&CaptureRecord> {
        self.captures.iter().find(|c| c.id == id)
    }

    pub fn capture_mut(&mut self, id: u64) -> Option<&mut CaptureRecord> {
        self.captures.iter_mut().find(|c| c.id == id)
    }
}

/// How to reach the server.
#[derive(Debug, Clone)]
pub struct Conn {
    pub addr: String,
    pub no_launch: bool,
    pub server_bin: Option<PathBuf>,
    /// Passed to an auto-started server: only simulated devices.
    pub no_usb: bool,
}

/// A connected client, with the server's process id and the CLI state for it.
pub struct Session {
    pub client: Client,
    pub server_pid: u64,
    pub app_version: String,
    pub state: State,
}

async fn try_connect(addr: &str) -> Result<(Client, u64, String)> {
    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
        .connect_timeout(Duration::from_millis(500));
    let channel = endpoint.connect().await?;
    let mut client = ManagerClient::new(channel).max_decoding_message_size(64 << 20);
    let info = client
        .get_app_info(GetAppInfoRequest {})
        .await?
        .into_inner()
        .app_info
        .unwrap_or_default();
    Ok((client, info.launch_pid, info.application_version))
}

fn is_local(addr: &str) -> bool {
    addr.starts_with("127.") || addr.starts_with("localhost:") || addr.starts_with("[::1]")
}

impl Conn {
    /// Connects, starting a background server first when none answers on a local address.
    pub async fn session(&self) -> Result<Session> {
        let (client, server_pid, app_version) = match try_connect(&self.addr).await {
            Ok(c) => c,
            Err(e) if self.no_launch || !is_local(&self.addr) => {
                return Err(e).with_context(|| format!("no automation server at {}", self.addr));
            }
            Err(_) => {
                let pid = self.start_detached()?;
                eprintln!(
                    "started headless Saleae server (pid {pid}) on {}; `saleae server stop` ends it",
                    self.addr
                );
                self.wait_ready(Duration::from_secs(20)).await?
            }
        };
        let state = State::load(&self.addr, server_pid);
        Ok(Session {
            client,
            server_pid,
            app_version,
            state,
        })
    }

    pub async fn probe(&self) -> Option<(Client, u64, String)> {
        try_connect(&self.addr).await.ok()
    }

    async fn wait_ready(&self, timeout: Duration) -> Result<(Client, u64, String)> {
        let start = Instant::now();
        loop {
            match try_connect(&self.addr).await {
                Ok(c) => return Ok(c),
                Err(e) if start.elapsed() > timeout => {
                    return Err(e).with_context(|| {
                        format!(
                            "server did not come up on {} within {timeout:?}; see {}",
                            self.addr,
                            log_path().display()
                        )
                    });
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }

    /// Starts the server in its own process group, logging to the data dir, and returns its pid.
    pub fn start_detached(&self) -> Result<u32> {
        let bin = locate(self.server_bin.as_deref())?;
        let (host, port) = self
            .addr
            .rsplit_once(':')
            .with_context(|| format!("address `{}` needs host:port", self.addr))?;
        std::fs::create_dir_all(data_dir())?;
        let log = std::fs::File::create(log_path()).context("create server log")?;
        let mut cmd = Command::new(&bin);
        cmd.arg("--port")
            .arg(port)
            .arg("--host")
            .arg(host.trim_matches(['[', ']']));
        if self.no_usb {
            cmd.arg("--disable-usb-scanning");
        }
        if let Some(dir) = bin.parent() {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("start {}", bin.display()))?;
        Ok(child.id())
    }

    /// Foreground run: the server replaces nothing, the CLI waits for it and passes its exit code on.
    pub fn run_foreground(&self) -> Result<i32> {
        let bin = locate(self.server_bin.as_deref())?;
        let (host, port) = self
            .addr
            .rsplit_once(':')
            .context("address needs host:port")?;
        let mut cmd = Command::new(&bin);
        cmd.arg("--port")
            .arg(port)
            .arg("--host")
            .arg(host.trim_matches(['[', ']']));
        if self.no_usb {
            cmd.arg("--disable-usb-scanning");
        }
        let status = cmd
            .status()
            .with_context(|| format!("run {}", bin.display()))?;
        Ok(status.code().unwrap_or(1))
    }
}

pub fn log_path() -> PathBuf {
    data_dir().join("server.log")
}

/// Ends the server with the given pid, after checking that it is the automation server.
pub fn kill(pid: u64) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if !comm.trim().starts_with("logic_automatio") {
            bail!(
                "pid {pid} is `{}`, not the automation server; not killing it",
                comm.trim()
            );
        }
    }
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .status()
    } else {
        Command::new("kill").arg(pid.to_string()).status()
    }
    .context("run kill")?;
    if !status.success() {
        bail!("kill {pid} failed");
    }
    Ok(())
}
