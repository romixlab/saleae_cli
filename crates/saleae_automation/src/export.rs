//! Exporting raw channel data and analyzer data tables.

use crate::error::{Error, Result};
use crate::pb;
use crate::server::Session;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Absolute path, since the server runs in its own working directory.
pub(crate) fn abs(p: &Path) -> Result<String> {
    Ok(std::path::absolute(p)?.to_string_lossy().into_owned())
}

/// Raw channel data, one CSV (or binary) file per channel type, into `dir` (created). `digital`/`analog`
/// default to the channels the capture was recorded with.
#[allow(clippy::too_many_arguments)]
pub async fn raw(
    s: &mut Session,
    capture: u64,
    dir: &Path,
    digital: Option<Vec<u32>>,
    analog: Option<Vec<u32>>,
    downsample: u64,
    binary: bool,
    iso: bool,
) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| crate::error::io_at(e, dir))?;
    let rec = s.state.capture(capture);
    let digital = digital
        .or_else(|| rec.map(|r| r.digital.clone()))
        .unwrap_or_default();
    let analog = analog
        .or_else(|| rec.map(|r| r.analog.clone()))
        .unwrap_or_default();
    if digital.is_empty() && analog.is_empty() {
        return Err(Error::invalid(format!(
            "no channels known for capture {capture}: pass digital / analog channels explicitly"
        )));
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
            .await?;
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
            .await?;
    }
    Ok(())
}

/// Analyzer results as one CSV table.
#[allow(clippy::too_many_arguments)]
pub async fn table(
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
        .await?;
    Ok(())
}
