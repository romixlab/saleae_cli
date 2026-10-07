//! Capture settings from the command line, and running a capture to its end.

use crate::parse;
use crate::pb::{
    self, capture_configuration::CaptureMode, logic_device_configuration::EnabledChannels,
};
use crate::server::{CaptureRecord, Session};
use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use std::time::Duration;

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Rising,
    Falling,
    /// High pulse (rising then falling edge); bound it with --min-pulse / --max-pulse.
    PulseHigh,
    /// Low pulse (falling then rising edge).
    PulseLow,
}

#[derive(Args, Debug, Clone)]
pub struct CaptureArgs {
    /// Device id (serial) from `saleae devices`; default: the first real device. Simulated: F4241 (Logic Pro 16),
    /// F4244 (Logic Pro 8), F4243 (Logic 8).
    #[arg(short, long, env = "SALEAE_DEVICE", add = clap_complete::ArgValueCandidates::new(crate::complete::device_ids), global = true)]
    pub device: Option<String>,
    /// Digital channels to record: `0-3,6`. `decode` adds the analyzer's channels on its own.
    #[arg(short = 'D', long, value_parser = parse::channel_list, global = true)]
    pub digital: Option<parse::Channels>,
    /// Analog channels to record: `0,1` (Logic 8 / Pro: same physical inputs as digital).
    #[arg(short = 'A', long, value_parser = parse::channel_list, global = true)]
    pub analog: Option<parse::Channels>,
    /// Digital sample rate: `10M`, `500M`. Must be one the device supports for the enabled channels.
    #[arg(short, long, default_value = "10M", value_parser = parse::rate, global = true)]
    pub rate: f64,
    /// Analog sample rate: `1M`, `50M`.
    #[arg(long, default_value = "1.5625M", value_parser = parse::rate, global = true)]
    pub analog_rate: f64,
    /// Logic level threshold in volts for Logic Pro 8/16: 1.2, 1.8 or 3.3 (default 3.3; Logic 8 has a fixed one).
    #[arg(short = 'V', long, global = true)]
    pub threshold: Option<f64>,
    /// Capture length: `1s`, `200ms`. Ignored with --trigger (see --after).
    #[arg(short = 't', long, default_value = "1s", value_parser = parse::duration, global = true)]
    pub duration: f64,
    /// Stop on a digital trigger instead of a fixed time: `CH` or `CH:EDGE` (edges: rising, falling, pulse-high,
    /// pulse-low; default rising).
    #[arg(long, value_name = "CH[:EDGE]", global = true)]
    pub trigger: Option<String>,
    /// Seconds to keep recording after the trigger.
    #[arg(long, default_value = "100ms", value_parser = parse::duration, global = true)]
    pub after: f64,
    /// Keep only this much data before the trigger (trigger mode) or the last this much of a timed capture.
    #[arg(long, value_parser = parse::duration, global = true)]
    pub trim: Option<f64>,
    /// Minimum pulse width for pulse triggers.
    #[arg(long, value_parser = parse::duration, global = true)]
    pub min_pulse: Option<f64>,
    /// Maximum pulse width for pulse triggers.
    #[arg(long, value_parser = parse::duration, global = true)]
    pub max_pulse: Option<f64>,
    /// Other channel state required at the trigger, repeatable: `2=high`, `3=low`.
    #[arg(long, value_name = "CH=high|low", global = true)]
    pub link: Vec<String>,
    /// Give up waiting for a trigger after this long (the capture is stopped and kept).
    #[arg(long, default_value = "10s", value_parser = parse::duration, global = true)]
    pub timeout: f64,
    /// Software glitch filter, repeatable: `CH=WIDTH`, e.g. `0=100ns`.
    #[arg(long, value_name = "CH=WIDTH", global = true)]
    pub glitch: Vec<String>,
    /// Capture buffer limit in MB (0: the server default).
    #[arg(long, default_value_t = 0, global = true)]
    pub buffer_mb: u32,
}

/// How the capture ended.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Completed,
    TriggerTimeout,
}

impl CaptureArgs {
    pub fn request(
        &self,
        extra_digital: &[u32],
    ) -> Result<(pb::StartCaptureRequest, Vec<u32>, Vec<u32>)> {
        let mut digital = self.digital.clone().unwrap_or_default().0;
        digital.extend_from_slice(extra_digital);
        let analog = self.analog.clone().unwrap_or_default().0;
        if digital.is_empty() && analog.is_empty() {
            digital = vec![0, 1, 2, 3];
        }
        digital.sort_unstable();
        digital.dedup();
        let mut glitch_filters = vec![];
        for g in &self.glitch {
            let (ch, w) = parse::channel_pair(g)?;
            glitch_filters.push(pb::GlitchFilterEntry {
                channel_index: ch,
                pulse_width_seconds: parse::duration(&w)?,
            });
        }
        let device_cfg = pb::LogicDeviceConfiguration {
            enabled_channels: Some(EnabledChannels::LogicChannels(pb::LogicChannels {
                digital_channels: digital.clone(),
                analog_channels: analog.clone(),
            })),
            digital_sample_rate: if digital.is_empty() {
                0
            } else {
                self.rate.round() as u32
            },
            analog_sample_rate: if analog.is_empty() {
                0
            } else {
                self.analog_rate.round() as u32
            },
            digital_threshold_volts: self.threshold.unwrap_or(3.3),
            glitch_filters,
        };
        let mode = match &self.trigger {
            None => CaptureMode::TimedCaptureMode(pb::TimedCaptureMode {
                duration_seconds: self.duration,
                trim_data_seconds: self.trim.unwrap_or(0.0),
            }),
            Some(t) => {
                let (ch, edge) = match t.split_once(':') {
                    Some((c, e)) => (c, Edge::from_str(e, true).map_err(|e| anyhow::anyhow!(e))?),
                    None => (t.as_str(), Edge::Rising),
                };
                let ch: u32 = ch
                    .trim()
                    .parse()
                    .with_context(|| format!("bad trigger channel `{t}`"))?;
                if !digital.contains(&ch) {
                    bail!(
                        "trigger channel {ch} is not among the recorded digital channels {digital:?}"
                    );
                }
                let mut linked_channels = vec![];
                for l in &self.link {
                    let (c, state) = parse::channel_pair(l)?;
                    let state = match state.to_ascii_lowercase().as_str() {
                        "high" | "1" => pb::DigitalTriggerLinkedChannelState::High,
                        "low" | "0" => pb::DigitalTriggerLinkedChannelState::Low,
                        s => bail!("linked channel state `{s}` is not high or low"),
                    };
                    linked_channels.push(pb::DigitalTriggerLinkedChannel {
                        channel_index: c,
                        state: state as i32,
                    });
                }
                CaptureMode::DigitalCaptureMode(pb::DigitalTriggerCaptureMode {
                    trigger_type: match edge {
                        Edge::Rising => pb::DigitalTriggerType::Rising,
                        Edge::Falling => pb::DigitalTriggerType::Falling,
                        Edge::PulseHigh => pb::DigitalTriggerType::PulseHigh,
                        Edge::PulseLow => pb::DigitalTriggerType::PulseLow,
                    } as i32,
                    after_trigger_seconds: self.after,
                    trim_data_seconds: self.trim.unwrap_or(0.0),
                    trigger_channel_index: ch,
                    min_pulse_width_seconds: self.min_pulse.unwrap_or(0.0),
                    max_pulse_width_seconds: self.max_pulse.unwrap_or(0.0),
                    linked_channels,
                })
            }
        };
        let req = pb::StartCaptureRequest {
            device_id: self.device.clone().unwrap_or_default(),
            device_configuration: Some(
                pb::start_capture_request::DeviceConfiguration::LogicDeviceConfiguration(
                    device_cfg,
                ),
            ),
            capture_configuration: Some(pb::CaptureConfiguration {
                buffer_size_megabytes: self.buffer_mb,
                capture_mode: Some(mode),
            }),
        };
        Ok((req, digital, analog))
    }

    pub fn describe(&self, digital: &[u32], analog: &[u32]) -> String {
        let mut parts = vec![];
        if !digital.is_empty() {
            parts.push(format!(
                "D{} @ {}",
                ranges(digital),
                parse::fmt_rate(self.rate, "S/s")
            ));
        }
        if !analog.is_empty() {
            parts.push(format!(
                "A{} @ {}",
                ranges(analog),
                parse::fmt_rate(self.analog_rate, "S/s")
            ));
        }
        match &self.trigger {
            None => parts.push(parse::fmt_seconds(self.duration)),
            Some(t) => parts.push(format!("trigger {t} +{}", parse::fmt_seconds(self.after))),
        }
        parts.join(", ")
    }
}

/// `0-3,6` from a sorted channel list.
pub fn ranges(ch: &[u32]) -> String {
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i < ch.len() {
        let mut j = i;
        while j + 1 < ch.len() && ch[j + 1] == ch[j] + 1 {
            j += 1;
        }
        out.push(if j > i {
            format!("{}-{}", ch[i], ch[j])
        } else {
            ch[i].to_string()
        });
        i = j + 1;
    }
    out.join(",")
}

/// Picks the device: the given id, else the first real one; explains the simulated ones when none is plugged in.
/// Returns its id and type.
/// Device id and raw `DeviceType` value (API 1.2 types like Logic MSO are not in the enum, see `DEVICE_TYPE_LOGIC_MSO`).
async fn resolve_device(s: &mut Session, device: &Option<String>) -> Result<(String, i32)> {
    let devices = s
        .client
        .get_devices(pb::GetDevicesRequest {
            include_simulation_devices: true,
        })
        .await?
        .into_inner()
        .devices;
    let ty = |d: &pb::Device| d.device_type;
    if let Some(d) = device {
        let Some(found) = devices.iter().find(|x| x.device_id.eq_ignore_ascii_case(d)) else {
            let known: Vec<_> = devices.iter().map(|x| x.device_id.as_str()).collect();
            bail!("device {d} not found; available: {}", known.join(", "));
        };
        return Ok((found.device_id.clone(), ty(found)));
    }
    if let Some(real) = devices.iter().find(|d| !d.is_simulation) {
        return Ok((real.device_id.clone(), ty(real)));
    }
    let sims: Vec<_> = devices
        .iter()
        .map(|d| {
            format!(
                "{} ({})",
                d.device_id,
                crate::cli::device_name(d.device_type)
            )
        })
        .collect();
    bail!(
        "no real Saleae device connected (check USB and udev rules); pass --device for a simulated one: {}",
        sims.join(", ")
    )
}

/// Starts the capture, waits for it to end, and records it in the CLI state.
pub async fn run(
    s: &mut Session,
    args: &CaptureArgs,
    extra_digital: &[u32],
) -> Result<(CaptureRecord, End)> {
    let (mut req, digital, analog) = args.request(extra_digital)?;
    let (device_id, device_type) = resolve_device(s, &args.device).await?;
    req.device_id = device_id;
    if device_type == crate::cli::DEVICE_TYPE_LOGIC_MSO {
        bail!("Logic MSO captures are not supported by this CLI yet (FEATURES CAP-4)");
    }
    let pro = matches!(
        pb::DeviceType::try_from(device_type),
        Ok(pb::DeviceType::LogicPro8 | pb::DeviceType::LogicPro16)
    );
    if !pro
        && let Some(pb::start_capture_request::DeviceConfiguration::LogicDeviceConfiguration(c)) =
            req.device_configuration.as_mut()
    {
        if args.threshold.is_some() {
            bail!(
                "{} has a fixed logic threshold; drop -V/--threshold",
                crate::cli::device_name(device_type)
            );
        }
        c.digital_threshold_volts = 0.0;
    }
    let id = s
        .client
        .start_capture(req.clone())
        .await
        .map_err(crate::cli::rpc_error)
        .context("start capture")?
        .into_inner()
        .capture_info
        .context("server returned no capture info")?
        .capture_id;
    let record = CaptureRecord {
        id,
        device: req.device_id.clone(),
        desc: args.describe(&digital, &analog),
        digital,
        analog,
        analyzers: vec![],
    };
    s.state.captures.push(record.clone());
    s.state.save()?;

    let wait = if args.trigger.is_some() {
        args.timeout
    } else {
        args.duration + 30.0
    };
    let mut w = tonic::Request::new(pb::WaitCaptureRequest { capture_id: id });
    w.set_timeout(Duration::from_secs_f64(wait.max(0.1)));
    let end = match s.client.wait_capture(w).await {
        Ok(_) => End::Completed,
        // tonic reports its own client-side timeout as Cancelled, the server's as DeadlineExceeded
        Err(st)
            if matches!(
                st.code(),
                tonic::Code::DeadlineExceeded | tonic::Code::Cancelled
            ) && args.trigger.is_some() =>
        {
            s.client
                .stop_capture(pb::StopCaptureRequest { capture_id: id })
                .await
                .map_err(crate::cli::rpc_error)
                .context("stop capture after trigger timeout")?;
            End::TriggerTimeout
        }
        Err(st) => return Err(crate::cli::rpc_error(st)).context("wait for capture"),
    };
    Ok((record, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ranges() {
        assert_eq!(ranges(&[0, 1, 2, 3, 6, 8, 9]), "0-3,6,8-9");
        assert_eq!(ranges(&[5]), "5");
        assert_eq!(ranges(&[]), "");
    }
}
