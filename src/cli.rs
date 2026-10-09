//! The command tree (clap derive) and the command handlers; `--json` output goes through [`Out::print`].

use crate::analyzers::{self, Protocol};
use crate::capture::{self, CaptureArgs, End};
use crate::server::{self, AnalyzerRecord, Conn, Session};
use crate::{complete, parse, pb, summary};
use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::ArgValueCandidates;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_SHA"),
    ", built ",
    env!("BUILD_TIME"),
    ")"
);

#[derive(Parser, Debug)]
#[command(name = "saleae", version = VERSION, about, long_about = "\
Drive Saleae logic analyzers through the headless Logic 2 automation server (gRPC).

The server keeps captures in memory, so the CLI starts it in the background on first use and keeps it running
(`saleae server stop` ends it). Typical use: `saleae devices`, then `saleae decode ... i2c --sda 0 --scl 1`.
Shell completion: `source <(COMPLETE=bash saleae)` (or zsh, fish).")]
struct Cli {
    /// Automation server address.
    #[arg(long, global = true, env = "SALEAE_ADDR", default_value = server::DEFAULT_ADDR)]
    addr: String,
    /// Machine-readable JSON on stdout.
    #[arg(long, global = true)]
    json: bool,
    /// Don't start a server when none is running.
    #[arg(long, global = true)]
    no_launch: bool,
    /// Server binary to start (default: the one `saleae server install` put in place, then PATH).
    #[arg(long, global = true, env = "SALEAE_SERVER_BIN")]
    server_bin: Option<PathBuf>,
    /// Start the server without USB scanning: only simulated devices.
    #[arg(long, global = true)]
    sim_only: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Install, start, stop and inspect the headless automation server.
    #[command(subcommand)]
    Server(ServerCmd),
    /// List devices, simulated ones included.
    Devices {
        /// Only real (USB) devices.
        #[arg(long)]
        real: bool,
    },
    /// Server, and the captures and analyzers this CLI opened in it.
    Status,
    /// Record a capture and keep it open in the server; prints its id.
    Capture {
        #[command(flatten)]
        capture: CaptureArgs,
        /// Also save it as a .sal file (opens in Logic 2).
        #[arg(long)]
        save: Option<PathBuf>,
        /// Also export raw data as CSV into this directory.
        #[arg(long)]
        export_raw: Option<PathBuf>,
        /// Close it afterwards (with --save / --export-raw).
        #[arg(long)]
        close: bool,
    },
    /// Load a .sal capture file into the server; prints its capture id.
    Load { file: PathBuf },
    /// Save a capture as a .sal file.
    Save {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        file: PathBuf,
    },
    /// Close a capture and free its memory.
    Close {
        #[arg(add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
    },
    /// Add, remove and list protocol analyzers.
    #[command(subcommand)]
    Analyzer(AnalyzerCmd),
    /// Export raw samples or analyzer tables.
    #[command(subcommand)]
    Export(ExportCmd),
    /// Capture, decode with one analyzer and print a compact summary: the one-shot command for agents.
    ///
    /// Example: `saleae decode -d F4241 -t 500ms i2c --sda 0 --scl 1`.
    Decode {
        #[command(flatten)]
        capture: CaptureArgs,
        #[command(flatten)]
        out: SummaryArgs,
        /// Save the capture as .sal too.
        #[arg(long, global = true)]
        save: Option<PathBuf>,
        /// Keep the capture open in the server afterwards (for more analyzers or exports).
        #[arg(long, global = true)]
        keep: bool,
        #[command(subcommand)]
        protocol: Protocol,
    },
    /// Summarize an analyzer already added to a capture (as `decode` prints it).
    Summarize {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        #[arg(short, long, add = ArgValueCandidates::new(complete::analyzer_ids))]
        analyzer: u64,
        #[command(flatten)]
        out: SummaryArgs,
    },
}

#[derive(Args, Debug, Clone)]
struct SummaryArgs {
    /// Show at most this many transactions / frames.
    #[arg(long, default_value_t = 40, global = true)]
    limit: usize,
    /// Also write the analyzer's full data table (hex) as CSV here.
    #[arg(long, global = true)]
    csv: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum ServerCmd {
    /// Download the server for this platform and unpack it into the CLI data dir.
    Install {
        /// Release build id from the Saleae forum post (part of the download URL).
        #[arg(long, default_value = server::DEFAULT_BUILD)]
        build: String,
        /// Full download URL instead of --build.
        #[arg(long)]
        url: Option<String>,
        /// An already downloaded logic_automation_server-<platform>.zip.
        #[arg(long)]
        zip: Option<PathBuf>,
    },
    /// Start the server in the background (or in the foreground with --foreground).
    Start {
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the server.
    Stop,
    /// Whether a server is running, its version and pid.
    Status,
    /// Print the server binary the CLI would start.
    Path,
}

#[derive(Subcommand, Debug)]
enum AnalyzerCmd {
    /// Add an analyzer to a capture; prints its id.
    Add {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        /// Label shown in exports (default: the analyzer name).
        #[arg(long)]
        label: Option<String>,
        #[command(subcommand)]
        protocol: Protocol,
    },
    /// Remove an analyzer from a capture.
    Remove {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        #[arg(add = ArgValueCandidates::new(complete::analyzer_ids))]
        analyzer: u64,
    },
    /// Analyzers bundled with the server (names for `analyzer add other NAME`).
    List,
}

#[derive(Subcommand, Debug)]
enum ExportCmd {
    /// Raw channel data, one CSV (or binary) file per channel type, into a directory.
    Raw {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        /// Output directory (created).
        #[arg(long)]
        dir: PathBuf,
        /// Digital channels (default: all recorded).
        #[arg(short = 'D', long, value_parser = parse::channel_list)]
        digital: Option<parse::Channels>,
        /// Analog channels (default: all recorded).
        #[arg(short = 'A', long, value_parser = parse::channel_list)]
        analog: Option<parse::Channels>,
        /// Keep every Nth analog sample (1-1000000).
        #[arg(long, default_value_t = 1)]
        downsample: u64,
        /// Saleae binary format instead of CSV.
        #[arg(long)]
        binary: bool,
        /// ISO 8601 timestamps instead of seconds.
        #[arg(long)]
        iso: bool,
    },
    /// Analyzer results as one CSV table.
    Table {
        #[arg(short, long, add = ArgValueCandidates::new(complete::capture_ids))]
        capture: u64,
        /// Analyzers to include, repeatable (default: all the CLI added to the capture).
        #[arg(short, long, add = ArgValueCandidates::new(complete::analyzer_ids))]
        analyzer: Vec<u64>,
        /// Output CSV file.
        #[arg(short, long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = Radix::Hex)]
        radix: Radix,
        /// Only these columns, repeatable.
        #[arg(long)]
        column: Vec<String>,
        /// Only rows matching this text (in all columns, or in --filter-column ones).
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        filter_column: Vec<String>,
        #[arg(long)]
        iso: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq)]
pub enum Radix {
    Hex,
    Dec,
    Bin,
    Ascii,
}

impl Radix {
    fn pb(self) -> pb::RadixType {
        match self {
            Radix::Hex => pb::RadixType::Hexadecimal,
            Radix::Dec => pb::RadixType::Decimal,
            Radix::Bin => pb::RadixType::Binary,
            Radix::Ascii => pb::RadixType::Ascii,
        }
    }
}

/// `DeviceType` value of a Logic MSO. It is only in API 1.2 (the proto in Saleae's server zip), not in the
/// published 1.0 proto this crate builds against, so it is matched as a number. TODO(CAP-4): use the enum.
pub const DEVICE_TYPE_LOGIC_MSO: i32 = 7;

pub fn device_name(t: i32) -> &'static str {
    if t == DEVICE_TYPE_LOGIC_MSO {
        return "Logic MSO";
    }
    match pb::DeviceType::try_from(t).unwrap_or(pb::DeviceType::Unspecified) {
        pb::DeviceType::Logic => "Logic",
        pb::DeviceType::Logic4 => "Logic 4",
        pb::DeviceType::Logic8 => "Logic 8",
        pb::DeviceType::Logic16 => "Logic 16",
        pb::DeviceType::LogicPro8 => "Logic Pro 8",
        pb::DeviceType::LogicPro16 => "Logic Pro 16",
        _ => "unknown",
    }
}

/// gRPC errors as one readable line (the server puts the useful part in the message).
pub fn rpc_error(s: tonic::Status) -> anyhow::Error {
    anyhow::anyhow!("server: {} ({:?})", s.message(), s.code())
}

/// Device ids and names of a server already running at `addr`, for completion; `None` when none answers quickly.
pub async fn list_devices_quick(addr: &str) -> Option<Vec<(String, String)>> {
    let fut = async {
        let ep = tonic::transport::Endpoint::from_shared(format!("http://{addr}")).ok()?;
        let mut c = pb::manager_client::ManagerClient::new(ep.connect().await.ok()?);
        let devices = c
            .get_devices(pb::GetDevicesRequest {
                include_simulation_devices: true,
            })
            .await
            .ok()?
            .into_inner()
            .devices;
        Some(
            devices
                .into_iter()
                .map(|d| {
                    let sim = if d.is_simulation { " (simulated)" } else { "" };
                    (d.device_id, format!("{}{sim}", device_name(d.device_type)))
                })
                .collect(),
        )
    };
    tokio::time::timeout(Duration::from_millis(500), fut)
        .await
        .ok()
        .flatten()
}

/// Absolute path for the server, which runs in its own working directory.
fn abs(p: &Path) -> Result<String> {
    Ok(std::path::absolute(p)?.to_string_lossy().into_owned())
}

struct Out {
    json: bool,
}

impl Out {
    /// Prints `text` for people or `value` for `--json`.
    fn print(&self, text: impl AsRef<str>, value: Value) {
        if self.json {
            println!("{value}");
        } else {
            let t = text.as_ref();
            if !t.is_empty() {
                println!("{}", t.trim_end());
            }
        }
    }
}

pub fn main() {
    complete::complete(Cli::command);
    let cli = Cli::parse();
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    let json = cli.json;
    if let Err(e) = rt.block_on(run(cli)) {
        if json {
            println!("{}", json!({ "error": format!("{e:#}") }));
        }
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let conn = Conn {
        addr: cli.addr.clone(),
        no_launch: cli.no_launch,
        server_bin: cli.server_bin.clone(),
        no_usb: cli.sim_only,
    };
    let out = Out { json: cli.json };
    match cli.cmd {
        Cmd::Server(c) => server_cmd(&conn, &out, c).await,
        Cmd::Devices { real } => {
            let mut s = conn.session().await?;
            let devices = s
                .client
                .get_devices(pb::GetDevicesRequest {
                    include_simulation_devices: !real,
                })
                .await
                .map_err(rpc_error)?
                .into_inner()
                .devices;
            let mut text = String::new();
            let mut list = vec![];
            for d in &devices {
                let sim = if d.is_simulation { "  simulated" } else { "" };
                text += &format!("{:<8} {}{sim}\n", d.device_id, device_name(d.device_type));
                list.push(json!({
                    "id": d.device_id, "type": device_name(d.device_type), "simulated": d.is_simulation,
                }));
            }
            if devices.iter().all(|d| d.is_simulation) {
                text += "(no real device: plug one in; on Linux install the udev rules, see `saleae server install`)\n";
            }
            out.print(text, json!({ "devices": list }));
            Ok(())
        }
        Cmd::Status => {
            let Some((_, pid, version)) = conn.probe().await else {
                out.print(
                    format!("no server at {}", conn.addr),
                    json!({ "running": false, "addr": conn.addr }),
                );
                return Ok(());
            };
            let state = server::State::load(&conn.addr, pid);
            let mut text = format!("server {version} at {} (pid {pid})\n", conn.addr);
            for c in &state.captures {
                text += &format!("capture {}  {}  {}\n", c.id, c.device, c.desc);
                for a in &c.analyzers {
                    text += &format!("  analyzer {}  {} ({})\n", a.id, a.label, a.name);
                }
            }
            if state.captures.is_empty() {
                text += "no open captures\n";
            }
            out.print(
                text,
                json!({ "running": true, "addr": conn.addr, "pid": pid, "version": version,
                        "captures": state.captures }),
            );
            Ok(())
        }
        Cmd::Capture {
            capture,
            save,
            export_raw,
            close,
        } => {
            let mut s = conn.session().await?;
            let (rec, end) = capture::run(&mut s, &capture, &[]).await?;
            let mut text = format!("capture {}  {}  {}", rec.id, rec.device, rec.desc);
            if end == End::TriggerTimeout {
                text += "\ntrigger not seen within the timeout; capture stopped and kept";
            }
            let mut v =
                json!({ "capture": rec.id, "device": rec.device, "desc": rec.desc, "end": end });
            if let Some(dir) = export_raw {
                export_raw_data(&mut s, rec.id, &dir, None, None, 1, false, false).await?;
                text += &format!("\nraw data in {}", dir.display());
                v["raw_dir"] = json!(dir);
            }
            if let Some(file) = save {
                save_capture(&mut s, rec.id, &file).await?;
                text += &format!("\nsaved {}", file.display());
                v["saved"] = json!(file);
            }
            if close {
                close_capture(&mut s, rec.id).await?;
                text += "\nclosed";
            }
            out.print(text, v);
            Ok(())
        }
        Cmd::Load { file } => {
            let mut s = conn.session().await?;
            let id = s
                .client
                .load_capture(pb::LoadCaptureRequest {
                    filepath: abs(&file)?,
                })
                .await
                .map_err(rpc_error)?
                .into_inner()
                .capture_info
                .context("no capture info")?
                .capture_id;
            s.state.captures.push(server::CaptureRecord {
                id,
                device: "file".into(),
                desc: file.display().to_string(),
                digital: vec![],
                analog: vec![],
                analyzers: vec![],
            });
            s.state.save()?;
            out.print(
                format!("capture {id}  loaded {}", file.display()),
                json!({ "capture": id }),
            );
            Ok(())
        }
        Cmd::Save { capture, file } => {
            let mut s = conn.session().await?;
            save_capture(&mut s, capture, &file).await?;
            out.print(
                format!("saved {}", file.display()),
                json!({ "saved": file }),
            );
            Ok(())
        }
        Cmd::Close { capture } => {
            let mut s = conn.session().await?;
            close_capture(&mut s, capture).await?;
            out.print(
                format!("closed capture {capture}"),
                json!({ "closed": capture }),
            );
            Ok(())
        }
        Cmd::Analyzer(c) => analyzer_cmd(&conn, &out, c).await,
        Cmd::Export(c) => export_cmd(&conn, &out, c).await,
        Cmd::Decode {
            capture,
            out: sargs,
            save,
            keep,
            protocol,
        } => {
            let spec = protocol.spec()?;
            let mut s = conn.session().await?;
            let (rec, end) = capture::run(&mut s, &capture, &spec.channels).await?;
            let result = async {
                let a = add_analyzer(&mut s, rec.id, &spec, None).await?;
                let mut sum = summarize(&mut s, rec.id, a.id, spec.kind, &sargs).await?;
                sum.header = format!(
                    "{} on capture {} ({}, {}){}",
                    spec.name,
                    rec.id,
                    rec.device,
                    rec.desc,
                    if end == End::TriggerTimeout {
                        ", trigger NOT seen"
                    } else {
                        ""
                    }
                );
                let mut v = sum.json();
                v["capture"] = json!(rec.id);
                v["analyzer"] = json!(a.id);
                v["device"] = json!(rec.device);
                v["capture_desc"] = json!(rec.desc);
                v["end"] = json!(end);
                v["settings"] = spec
                    .settings
                    .iter()
                    .map(|(k, val)| (k.clone(), analyzers::setting_json(val)))
                    .collect::<serde_json::Map<_, _>>()
                    .into();
                let mut text = sum.text();
                if let Some(file) = &save {
                    save_capture(&mut s, rec.id, file).await?;
                    text += &format!("saved {}\n", file.display());
                    v["saved"] = json!(file);
                }
                if keep {
                    text += &format!("capture {} kept open (analyzer {})\n", rec.id, a.id);
                }
                anyhow::Ok((text, v))
            }
            .await;
            if !keep {
                close_capture(&mut s, rec.id).await?;
            }
            let (text, v) = result?;
            out.print(text, v);
            Ok(())
        }
        Cmd::Summarize {
            capture,
            analyzer,
            out: sargs,
        } => {
            let mut s = conn.session().await?;
            let kind = s
                .state
                .capture(capture)
                .and_then(|c| c.analyzers.iter().find(|a| a.id == analyzer))
                .map(|a| summary::kind_of(&a.name))
                .unwrap_or(analyzers::Kind::Other);
            let mut sum = summarize(&mut s, capture, analyzer, kind, &sargs).await?;
            sum.header = format!("analyzer {analyzer} on capture {capture}");
            out.print(sum.text(), sum.json());
            Ok(())
        }
    }
}

async fn server_cmd(conn: &Conn, out: &Out, c: ServerCmd) -> Result<()> {
    match c {
        ServerCmd::Install { build, url, zip } => {
            let url = match url {
                Some(u) => u,
                None => server::download_url(&build)?,
            };
            let bin = server::install(&url, zip.as_deref())?;
            let mut text = format!("installed {}\n", bin.display());
            if cfg!(target_os = "linux") {
                let rules = bin.with_file_name("99-SaleaeLogic.rules");
                text += &format!(
                    "for real devices without root, once: sudo cp {} /etc/udev/rules.d/ && sudo udevadm control \
                     --reload-rules, then replug the device\n",
                    rules.display()
                );
            }
            out.print(text, json!({ "installed": bin }));
        }
        ServerCmd::Start { foreground } => {
            if let Some((_, pid, version)) = conn.probe().await {
                out.print(
                    format!(
                        "server {version} already running at {} (pid {pid})",
                        conn.addr
                    ),
                    json!({ "running": true, "pid": pid, "version": version, "started": false }),
                );
                return Ok(());
            }
            if foreground {
                std::process::exit(conn.run_foreground()?);
            }
            let s = conn.session().await?;
            out.print(
                format!("server {} running at {} (pid {})", s.app_version, conn.addr, s.server_pid),
                json!({ "running": true, "pid": s.server_pid, "version": s.app_version, "started": true }),
            );
        }
        ServerCmd::Stop => match conn.probe().await {
            None => out.print(
                format!("no server at {}", conn.addr),
                json!({ "stopped": false }),
            ),
            Some((_, pid, _)) => {
                server::kill(pid)?;
                // it shuts down cleanly (closes USB), which takes a moment
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while conn.probe().await.is_some() {
                    if std::time::Instant::now() > deadline {
                        bail!("server (pid {pid}) still answers 10 s after being asked to stop");
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                out.print(
                    format!("stopped server (pid {pid})"),
                    json!({ "stopped": true, "pid": pid }),
                );
            }
        },
        ServerCmd::Status => match conn.probe().await {
            None => out.print(
                format!("no server at {}", conn.addr),
                json!({ "running": false, "addr": conn.addr }),
            ),
            Some((_, pid, version)) => out.print(
                format!("server {version} at {} (pid {pid})", conn.addr),
                json!({ "running": true, "addr": conn.addr, "pid": pid, "version": version }),
            ),
        },
        ServerCmd::Path => {
            let bin = server::locate(conn.server_bin.as_deref())?;
            out.print(bin.display().to_string(), json!({ "path": bin }));
        }
    }
    Ok(())
}

async fn analyzer_cmd(conn: &Conn, out: &Out, c: AnalyzerCmd) -> Result<()> {
    match c {
        AnalyzerCmd::Add {
            capture,
            label,
            protocol,
        } => {
            let spec = protocol.spec()?;
            let mut s = conn.session().await?;
            let a = add_analyzer(&mut s, capture, &spec, label).await?;
            out.print(
                format!("analyzer {}  {} on capture {capture}", a.id, a.label),
                json!({ "analyzer": a.id, "capture": capture, "name": a.name, "label": a.label }),
            );
        }
        AnalyzerCmd::Remove { capture, analyzer } => {
            let mut s = conn.session().await?;
            s.client
                .remove_analyzer(pb::RemoveAnalyzerRequest {
                    capture_id: capture,
                    analyzer_id: analyzer,
                })
                .await
                .map_err(rpc_error)?;
            if let Some(c) = s.state.capture_mut(capture) {
                c.analyzers.retain(|a| a.id != analyzer);
            }
            s.state.save()?;
            out.print(
                format!("removed analyzer {analyzer}"),
                json!({ "removed": analyzer }),
            );
        }
        AnalyzerCmd::List => {
            let text = format!(
                "{}\n\nShorthands with typed flags: spi, i2c, serial (uart), can, lin, onewire; any other: \
                 `analyzer add other NAME --set KEY=VALUE` with the setting names Logic 2 shows.",
                analyzers::BUNDLED.join("\n")
            );
            out.print(text, json!({ "analyzers": analyzers::BUNDLED }));
        }
    }
    Ok(())
}

async fn add_analyzer(
    s: &mut Session,
    capture: u64,
    spec: &analyzers::Spec,
    label: Option<String>,
) -> Result<AnalyzerRecord> {
    let label = label.unwrap_or_else(|| spec.name.clone());
    let id = s
        .client
        .add_analyzer(pb::AddAnalyzerRequest {
            capture_id: capture,
            analyzer_name: spec.name.clone(),
            analyzer_label: label.clone(),
            settings: spec.settings.clone(),
        })
        .await
        .map_err(rpc_error)
        .with_context(|| format!("add {} analyzer", spec.name))?
        .into_inner()
        .analyzer_id;
    let rec = AnalyzerRecord {
        id,
        name: spec.name.clone(),
        label,
    };
    if let Some(c) = s.state.capture_mut(capture) {
        c.analyzers.push(rec.clone());
        s.state.save()?;
    }
    Ok(rec)
}

async fn summarize(
    s: &mut Session,
    capture: u64,
    analyzer: u64,
    kind: analyzers::Kind,
    args: &SummaryArgs,
) -> Result<summary::Summary> {
    let tmp = server::data_dir().join(format!("table-{capture}-{analyzer}.csv"));
    std::fs::create_dir_all(server::data_dir())?;
    export_table(
        s,
        capture,
        &[analyzer],
        &tmp,
        Radix::Hex,
        &[],
        None,
        &[],
        false,
    )
    .await?;
    let data = std::fs::read_to_string(&tmp).with_context(|| format!("read {}", tmp.display()))?;
    if let Some(csv) = &args.csv {
        std::fs::copy(&tmp, csv).with_context(|| format!("write {}", csv.display()))?;
    }
    let _ = std::fs::remove_file(&tmp);
    summary::summarize(kind, &data, args.limit)
}

async fn export_cmd(conn: &Conn, out: &Out, c: ExportCmd) -> Result<()> {
    let mut s = conn.session().await?;
    match c {
        ExportCmd::Raw {
            capture,
            dir,
            digital,
            analog,
            downsample,
            binary,
            iso,
        } => {
            export_raw_data(
                &mut s, capture, &dir, digital, analog, downsample, binary, iso,
            )
            .await?;
            let files: Vec<String> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path().display().to_string())
                        .collect()
                })
                .unwrap_or_default();
            out.print(
                format!("exported to {}\n{}", dir.display(), files.join("\n")),
                json!({ "dir": dir, "files": files }),
            );
        }
        ExportCmd::Table {
            capture,
            analyzer,
            out: file,
            radix,
            column,
            filter,
            filter_column,
            iso,
        } => {
            let ids = if analyzer.is_empty() {
                s.state
                    .capture(capture)
                    .map(|c| c.analyzers.iter().map(|a| a.id).collect())
                    .unwrap_or_default()
            } else {
                analyzer
            };
            if ids.is_empty() {
                bail!("no analyzers: pass --analyzer ID (see `saleae status`)");
            }
            export_table(
                &mut s,
                capture,
                &ids,
                &file,
                radix,
                &column,
                filter,
                &filter_column,
                iso,
            )
            .await?;
            out.print(
                format!("exported {}", file.display()),
                json!({ "file": file, "analyzers": ids }),
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn export_raw_data(
    s: &mut Session,
    capture: u64,
    dir: &Path,
    digital: Option<parse::Channels>,
    analog: Option<parse::Channels>,
    downsample: u64,
    binary: bool,
    iso: bool,
) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let rec = s.state.capture(capture);
    let digital = digital
        .map(|c| c.0)
        .or_else(|| rec.map(|r| r.digital.clone()))
        .unwrap_or_default();
    let analog = analog
        .map(|c| c.0)
        .or_else(|| rec.map(|r| r.analog.clone()))
        .unwrap_or_default();
    if digital.is_empty() && analog.is_empty() {
        bail!("no channels known for capture {capture}: pass --digital / --analog");
    }
    let channels = pb::LogicChannels {
        digital_channels: digital,
        analog_channels: analog,
    };
    let directory = abs(dir)?;
    if binary {
        s.client
            .export_raw_data_binary(pb::ExportRawDataBinaryRequest {
                capture_id: capture,
                directory,
                channels: Some(pb::export_raw_data_binary_request::Channels::LogicChannels(
                    channels,
                )),
                analog_downsample_ratio: downsample,
            })
            .await
            .map_err(rpc_error)?;
    } else {
        s.client
            .export_raw_data_csv(pb::ExportRawDataCsvRequest {
                capture_id: capture,
                directory,
                channels: Some(pb::export_raw_data_csv_request::Channels::LogicChannels(
                    channels,
                )),
                analog_downsample_ratio: downsample,
                iso8601_timestamp: iso,
            })
            .await
            .map_err(rpc_error)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn export_table(
    s: &mut Session,
    capture: u64,
    analyzers: &[u64],
    file: &Path,
    radix: Radix,
    columns: &[String],
    filter: Option<String>,
    filter_columns: &[String],
    iso: bool,
) -> Result<()> {
    s.client
        .export_data_table_csv(pb::ExportDataTableCsvRequest {
            capture_id: capture,
            filepath: abs(file)?,
            analyzers: analyzers
                .iter()
                .map(|&id| pb::DataTableAnalyzerConfiguration {
                    analyzer_id: id,
                    radix_type: radix.pb() as i32,
                })
                .collect(),
            iso8601_timestamp: iso,
            export_columns: columns.to_vec(),
            filter: filter.map(|q| pb::DataTableFilter {
                query: q,
                columns: filter_columns.to_vec(),
            }),
        })
        .await
        .map_err(rpc_error)
        .context("export data table")?;
    Ok(())
}

async fn save_capture(s: &mut Session, capture: u64, file: &Path) -> Result<()> {
    s.client
        .save_capture(pb::SaveCaptureRequest {
            capture_id: capture,
            filepath: abs(file)?,
        })
        .await
        .map_err(rpc_error)
        .context("save capture")?;
    Ok(())
}

/// Stops (a no-op unless still running) and closes a capture.
async fn close_capture(s: &mut Session, capture: u64) -> Result<()> {
    s.client
        .stop_capture(pb::StopCaptureRequest {
            capture_id: capture,
        })
        .await
        .map_err(rpc_error)
        .context("stop capture")?;
    s.client
        .close_capture(pb::CloseCaptureRequest {
            capture_id: capture,
        })
        .await
        .map_err(rpc_error)
        .context("close capture")?;
    s.state.captures.retain(|c| c.id != capture);
    s.state.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }
}
