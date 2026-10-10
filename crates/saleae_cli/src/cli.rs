//! The command tree (clap derive) and the command handlers; `--json` output goes through [`Out::print`].

use crate::analyzer_args::Protocol;
use crate::capture_args::CaptureArgs;
use crate::{complete, parse};
use anyhow::{Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::ArgValueCandidates;
use saleae_automation::capture::End;
use saleae_automation::server::Conn;
use serde_json::{Value, json};
use std::path::PathBuf;
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
    #[arg(long, global = true, env = "SALEAE_ADDR", default_value = saleae_automation::server::DEFAULT_ADDR)]
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
        #[arg(long, default_value = saleae_automation::server::DEFAULT_BUILD)]
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

impl From<Radix> for saleae_automation::export::Radix {
    fn from(r: Radix) -> Self {
        match r {
            Radix::Hex => saleae_automation::export::Radix::Hex,
            Radix::Dec => saleae_automation::export::Radix::Dec,
            Radix::Bin => saleae_automation::export::Radix::Bin,
            Radix::Ascii => saleae_automation::export::Radix::Ascii,
        }
    }
}

/// Prints `msg` to stderr: the lib's progress callback for a server it started or installed.
fn progress(msg: &str) {
    eprintln!("{msg}");
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
            let mut s = conn.session(Some(&progress)).await?;
            let devices = saleae_automation::device::list(&mut s, real).await?;
            let mut text = String::new();
            let mut list = vec![];
            for d in &devices {
                let sim = if d.simulated { "  simulated" } else { "" };
                text += &format!("{:<8} {}{sim}\n", d.id, d.type_name);
                list.push(json!({ "id": d.id, "type": d.type_name, "simulated": d.simulated }));
            }
            if devices.iter().all(|d| d.simulated) {
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
            let state = saleae_automation::server::State::load(&conn.addr, pid);
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
            let opts = capture.options()?;
            let mut s = conn.session(Some(&progress)).await?;
            let (rec, end) = saleae_automation::capture::run(&mut s, &opts, &[]).await?;
            let mut text = format!("capture {}  {}  {}", rec.id, rec.device, rec.desc);
            if end == End::TriggerTimeout {
                text += "\ntrigger not seen within the timeout; capture stopped and kept";
            }
            let mut v =
                json!({ "capture": rec.id, "device": rec.device, "desc": rec.desc, "end": end });
            if let Some(dir) = export_raw {
                saleae_automation::export::raw(&mut s, rec.id, &dir, None, None, 1, false, false)
                    .await?;
                text += &format!("\nraw data in {}", dir.display());
                v["raw_dir"] = json!(dir);
            }
            if let Some(file) = save {
                saleae_automation::capture::save(&mut s, rec.id, &file).await?;
                text += &format!("\nsaved {}", file.display());
                v["saved"] = json!(file);
            }
            if close {
                saleae_automation::capture::close(&mut s, rec.id).await?;
                text += "\nclosed";
            }
            out.print(text, v);
            Ok(())
        }
        Cmd::Load { file } => {
            let mut s = conn.session(Some(&progress)).await?;
            let id = saleae_automation::capture::load(&mut s, &file).await?;
            out.print(
                format!("capture {id}  loaded {}", file.display()),
                json!({ "capture": id }),
            );
            Ok(())
        }
        Cmd::Save { capture, file } => {
            let mut s = conn.session(Some(&progress)).await?;
            saleae_automation::capture::save(&mut s, capture, &file).await?;
            out.print(
                format!("saved {}", file.display()),
                json!({ "saved": file }),
            );
            Ok(())
        }
        Cmd::Close { capture } => {
            let mut s = conn.session(Some(&progress)).await?;
            saleae_automation::capture::close(&mut s, capture).await?;
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
            let proto = protocol.to_lib()?;
            let spec = proto.spec()?;
            let opts = capture.options()?;
            let mut s = conn.session(Some(&progress)).await?;
            let outcome = saleae_automation::decode::decode(
                &mut s,
                &opts,
                &proto,
                None,
                sargs.limit,
                sargs.csv.as_deref(),
                save.as_deref(),
                keep,
            )
            .await?;
            let mut summary = outcome.summary;
            summary.header = format!(
                "{} on capture {} ({}, {}){}",
                spec.name,
                outcome.capture.id,
                outcome.capture.device,
                outcome.capture.desc,
                if outcome.end == End::TriggerTimeout {
                    ", trigger NOT seen"
                } else {
                    ""
                }
            );
            let mut v = summary.json();
            v["capture"] = json!(outcome.capture.id);
            v["analyzer"] = json!(outcome.analyzer.id);
            v["device"] = json!(outcome.capture.device);
            v["capture_desc"] = json!(outcome.capture.desc);
            v["end"] = json!(outcome.end);
            v["settings"] = spec
                .settings
                .iter()
                .map(|(k, val)| (k.clone(), saleae_automation::analyzer::setting_json(val)))
                .collect::<serde_json::Map<_, _>>()
                .into();
            let mut text = summary.text();
            if let Some(file) = &outcome.saved {
                text += &format!("saved {}\n", file.display());
                v["saved"] = json!(file);
            }
            if keep {
                text += &format!(
                    "capture {} kept open (analyzer {})\n",
                    outcome.capture.id, outcome.analyzer.id
                );
            }
            out.print(text, v);
            Ok(())
        }
        Cmd::Summarize {
            capture,
            analyzer,
            out: sargs,
        } => {
            let mut s = conn.session(Some(&progress)).await?;
            let kind = s
                .state
                .capture(capture)
                .and_then(|c| c.analyzers.iter().find(|a| a.id == analyzer))
                .map(|a| saleae_automation::analyzer::kind_of(&a.name))
                .unwrap_or(saleae_automation::analyzer::Kind::Other);
            let mut summary = saleae_automation::decode::summarize_analyzer(
                &mut s,
                capture,
                analyzer,
                kind,
                sargs.limit,
                sargs.csv.as_deref(),
            )
            .await?;
            summary.header = format!("analyzer {analyzer} on capture {capture}");
            out.print(summary.text(), summary.json());
            Ok(())
        }
    }
}

async fn server_cmd(conn: &Conn, out: &Out, c: ServerCmd) -> Result<()> {
    match c {
        ServerCmd::Install { build, url, zip } => {
            let url = match url {
                Some(u) => u,
                None => saleae_automation::server::download_url(&build)?,
            };
            let bin = saleae_automation::server::install(&url, zip.as_deref(), Some(&progress))?;
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
            let s = conn.session(Some(&progress)).await?;
            out.print(
                format!(
                    "server {} running at {} (pid {})",
                    s.app_version, conn.addr, s.server_pid
                ),
                json!({ "running": true, "pid": s.server_pid, "version": s.app_version, "started": true }),
            );
        }
        ServerCmd::Stop => match conn.probe().await {
            None => out.print(
                format!("no server at {}", conn.addr),
                json!({ "stopped": false }),
            ),
            Some((_, pid, _)) => {
                saleae_automation::server::kill(pid)?;
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
            let bin = saleae_automation::server::locate(conn.server_bin.as_deref())?;
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
            let mut s = conn.session(Some(&progress)).await?;
            let a = saleae_automation::analyzer::add(&mut s, capture, &spec, label).await?;
            out.print(
                format!("analyzer {}  {} on capture {capture}", a.id, a.label),
                json!({ "analyzer": a.id, "capture": capture, "name": a.name, "label": a.label }),
            );
        }
        AnalyzerCmd::Remove { capture, analyzer } => {
            let mut s = conn.session(Some(&progress)).await?;
            saleae_automation::analyzer::remove(&mut s, capture, analyzer).await?;
            out.print(
                format!("removed analyzer {analyzer}"),
                json!({ "removed": analyzer }),
            );
        }
        AnalyzerCmd::List => {
            let text = format!(
                "{}\n\nShorthands with typed flags: spi, i2c, serial (uart), can, lin, onewire; any other: \
                 `analyzer add other NAME --set KEY=VALUE` with the setting names Logic 2 shows.",
                saleae_automation::analyzer::BUNDLED.join("\n")
            );
            out.print(
                text,
                json!({ "analyzers": saleae_automation::analyzer::BUNDLED }),
            );
        }
    }
    Ok(())
}

async fn export_cmd(conn: &Conn, out: &Out, c: ExportCmd) -> Result<()> {
    let mut s = conn.session(Some(&progress)).await?;
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
            saleae_automation::export::raw(
                &mut s,
                capture,
                &dir,
                digital.map(|c| c.0),
                analog.map(|c| c.0),
                downsample,
                binary,
                iso,
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
            saleae_automation::export::table(
                &mut s,
                capture,
                &ids,
                &file,
                radix.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }
}
