//! One-shot flows that combine a capture, an analyzer and a summary: capture + analyze + summarize + close in one
//! call (`decode`), and summarizing an analyzer already added to a capture (`summarize_analyzer`).

use crate::analyzer::{self, Kind, Protocol};
use crate::capture::{self, CaptureOptions, End};
use crate::error::Result;
use crate::export::{self, Radix};
use crate::server::{AnalyzerRecord, CaptureRecord, Session};
use crate::summary::{self, Summary};
use std::path::{Path, PathBuf};

pub struct DecodeOutcome {
    pub capture: CaptureRecord,
    pub analyzer: AnalyzerRecord,
    pub end: End,
    pub summary: Summary,
    pub saved: Option<PathBuf>,
}

/// Capture, add one analyzer, summarize and (unless `keep`) close, in one call.
#[allow(clippy::too_many_arguments)]
pub async fn decode(
    session: &mut Session,
    capture_opts: &CaptureOptions,
    protocol: &Protocol,
    label: Option<String>,
    limit: usize,
    csv_out: Option<&Path>,
    save_to: Option<&Path>,
    keep: bool,
) -> Result<DecodeOutcome> {
    let spec = protocol.spec()?;
    let (rec, end) = capture::run(session, capture_opts, &spec.channels).await?;
    let result = async {
        let a = analyzer::add(session, rec.id, &spec, label).await?;
        let summary = summarize_analyzer(session, rec.id, a.id, spec.kind, limit, csv_out).await?;
        let saved = match save_to {
            Some(file) => {
                capture::save(session, rec.id, file).await?;
                Some(file.to_path_buf())
            }
            None => None,
        };
        Result::Ok((a, summary, saved))
    }
    .await;
    if !keep {
        capture::close(session, rec.id).await?;
    }
    let (analyzer, summary, saved) = result?;
    Ok(DecodeOutcome {
        capture: rec,
        analyzer,
        end,
        summary,
        saved,
    })
}

/// Summarizes an analyzer already added to a capture (as `decode` does for the one it adds).
pub async fn summarize_analyzer(
    session: &mut Session,
    capture: u64,
    analyzer_id: u64,
    kind: Kind,
    limit: usize,
    csv_out: Option<&Path>,
) -> Result<Summary> {
    let dir = crate::server::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| crate::error::io_at(e, &dir))?;
    let tmp = dir.join(format!("table-{capture}-{analyzer_id}.csv"));
    export::table(
        session,
        capture,
        &[analyzer_id],
        &tmp,
        Radix::Hex,
        &[],
        None,
        &[],
        false,
    )
    .await?;
    let data = std::fs::read_to_string(&tmp).map_err(|e| crate::error::io_at(e, &tmp))?;
    if let Some(csv) = csv_out {
        std::fs::copy(&tmp, csv).map_err(|e| crate::error::io_at(e, csv))?;
    }
    let _ = std::fs::remove_file(&tmp);
    summary::summarize(kind, &data, limit)
}
