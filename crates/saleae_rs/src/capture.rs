//! Capture settings and running a capture to its end.

use crate::device;
use crate::error::{Error, Result};
use crate::export::abs;
use crate::fmt;
use crate::pb::{
    self, capture_configuration::CaptureMode, logic_device_configuration::EnabledChannels,
};
use crate::server::{CaptureRecord, Session};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Rising,
    Falling,
    /// High pulse (rising then falling edge); bound it with `min_pulse` / `max_pulse`.
    PulseHigh,
    /// Low pulse (falling then rising edge).
    PulseLow,
}

impl Edge {
    /// From `rising`, `falling`, `pulse-high` / `pulsehigh`, `pulse-low` / `pulselow` (case-insensitive).
    pub fn parse(s: &str) -> Result<Edge> {
        match s.to_ascii_lowercase().replace('-', "").as_str() {
            "rising" => Ok(Edge::Rising),
            "falling" => Ok(Edge::Falling),
            "pulsehigh" => Ok(Edge::PulseHigh),
            "pulselow" => Ok(Edge::PulseLow),
            _ => Err(Error::invalid(format!(
                "unknown trigger edge `{s}` (rising, falling, pulse-high, pulse-low)"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    High,
    Low,
}

/// Other channel state required at the trigger (`--link 2=high`).
#[derive(Debug, Clone, Copy)]
pub struct LinkedChannel {
    pub channel: u32,
    pub state: LinkState,
}

/// Stop on a digital trigger instead of a fixed time.
#[derive(Debug, Clone)]
pub struct TriggerSpec {
    pub channel: u32,
    pub edge: Edge,
    /// Seconds to keep recording after the trigger.
    pub after: f64,
    pub min_pulse: Option<f64>,
    pub max_pulse: Option<f64>,
    pub linked: Vec<LinkedChannel>,
    /// Give up waiting for the trigger after this long (the capture is stopped and kept).
    pub timeout: f64,
}

#[derive(Debug, Clone)]
pub struct CaptureOptions {
    /// Device id (serial); `None`: the first real device.
    pub device: Option<String>,
    /// Digital channels to record. A `decode` call adds the analyzer's channels on its own.
    pub digital: Vec<u32>,
    /// Analog channels to record (Logic 8 / Pro: same physical inputs as digital).
    pub analog: Vec<u32>,
    /// Digital sample rate; must be one the device supports for the enabled channels.
    pub rate: f64,
    pub analog_rate: f64,
    /// Logic level threshold in volts for Logic Pro 8/16 (1.2, 1.8 or 3.3); Logic 8 has a fixed one, so this
    /// must be `None` for it.
    pub threshold: Option<f64>,
    /// Capture length in seconds; ignored when `trigger` is set (see `TriggerSpec::after`).
    pub duration: f64,
    pub trigger: Option<TriggerSpec>,
    /// Keep only this much data before the trigger (trigger mode) or the last this much of a timed capture.
    pub trim: Option<f64>,
    /// Software glitch filter per channel: `(channel, pulse width seconds)`.
    pub glitch: Vec<(u32, f64)>,
    /// Capture buffer limit in MB (0: the server default).
    pub buffer_mb: u32,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        CaptureOptions {
            device: None,
            digital: vec![],
            analog: vec![],
            rate: 10e6,
            analog_rate: 1.5625e6,
            threshold: None,
            duration: 1.0,
            trigger: None,
            trim: None,
            glitch: vec![],
            buffer_mb: 0,
        }
    }
}

/// How the capture ended.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Completed,
    TriggerTimeout,
}

impl CaptureOptions {
    pub fn request(
        &self,
        extra_digital: &[u32],
    ) -> Result<(pb::StartCaptureRequest, Vec<u32>, Vec<u32>)> {
        let mut digital = self.digital.clone();
        digital.extend_from_slice(extra_digital);
        let analog = self.analog.clone();
        if digital.is_empty() && analog.is_empty() {
            digital = vec![0, 1, 2, 3];
        }
        digital.sort_unstable();
        digital.dedup();
        let glitch_filters = self
            .glitch
            .iter()
            .map(
                |&(channel_index, pulse_width_seconds)| pb::GlitchFilterEntry {
                    channel_index,
                    pulse_width_seconds,
                },
            )
            .collect();
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
                if !digital.contains(&t.channel) {
                    return Err(Error::invalid(format!(
                        "trigger channel {} is not among the recorded digital channels {digital:?}",
                        t.channel
                    )));
                }
                let linked_channels = t
                    .linked
                    .iter()
                    .map(|l| pb::DigitalTriggerLinkedChannel {
                        channel_index: l.channel,
                        state: match l.state {
                            LinkState::High => pb::DigitalTriggerLinkedChannelState::High,
                            LinkState::Low => pb::DigitalTriggerLinkedChannelState::Low,
                        } as i32,
                    })
                    .collect();
                CaptureMode::DigitalCaptureMode(pb::DigitalTriggerCaptureMode {
                    trigger_type: match t.edge {
                        Edge::Rising => pb::DigitalTriggerType::Rising,
                        Edge::Falling => pb::DigitalTriggerType::Falling,
                        Edge::PulseHigh => pb::DigitalTriggerType::PulseHigh,
                        Edge::PulseLow => pb::DigitalTriggerType::PulseLow,
                    } as i32,
                    after_trigger_seconds: t.after,
                    trim_data_seconds: self.trim.unwrap_or(0.0),
                    trigger_channel_index: t.channel,
                    min_pulse_width_seconds: t.min_pulse.unwrap_or(0.0),
                    max_pulse_width_seconds: t.max_pulse.unwrap_or(0.0),
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
                fmt::ranges(digital),
                fmt::rate(self.rate, "S/s")
            ));
        }
        if !analog.is_empty() {
            parts.push(format!(
                "A{} @ {}",
                fmt::ranges(analog),
                fmt::rate(self.analog_rate, "S/s")
            ));
        }
        match &self.trigger {
            None => parts.push(fmt::seconds(self.duration)),
            Some(t) => parts.push(format!("trigger {} +{}", t.channel, fmt::seconds(t.after))),
        }
        parts.join(", ")
    }
}

/// Starts the capture, waits for it to end, and records it in the session state.
pub async fn run(
    s: &mut Session,
    opts: &CaptureOptions,
    extra_digital: &[u32],
) -> Result<(CaptureRecord, End)> {
    let (mut req, digital, analog) = opts.request(extra_digital)?;
    let (device_id, device_type) = device::resolve(s, opts.device.as_deref()).await?;
    req.device_id = device_id;
    if device_type == device::DEVICE_TYPE_LOGIC_MSO {
        return Err(Error::invalid(
            "Logic MSO captures are not supported yet (FEATURES CAP-4)",
        ));
    }
    let pro = matches!(
        pb::DeviceType::try_from(device_type),
        Ok(pb::DeviceType::LogicPro8 | pb::DeviceType::LogicPro16)
    );
    if !pro
        && let Some(pb::start_capture_request::DeviceConfiguration::LogicDeviceConfiguration(c)) =
            req.device_configuration.as_mut()
    {
        if opts.threshold.is_some() {
            return Err(Error::invalid(format!(
                "{} has a fixed logic threshold; drop the threshold option",
                device::name(device_type)
            )));
        }
        c.digital_threshold_volts = 0.0;
    }
    let id = s
        .client
        .start_capture(req.clone())
        .await?
        .into_inner()
        .capture_info
        .ok_or_else(|| Error::server("start capture: server returned no capture info"))?
        .capture_id;
    let record = CaptureRecord {
        id,
        device: req.device_id.clone(),
        desc: opts.describe(&digital, &analog),
        digital,
        analog,
        analyzers: vec![],
    };
    s.state.captures.push(record.clone());
    s.state.save()?;

    let wait = match &opts.trigger {
        Some(t) => t.timeout,
        None => opts.duration + 30.0,
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
            ) && opts.trigger.is_some() =>
        {
            s.client
                .stop_capture(pb::StopCaptureRequest { capture_id: id })
                .await?;
            End::TriggerTimeout
        }
        Err(st) => return Err(st.into()),
    };
    Ok((record, end))
}

/// Loads a `.sal` capture file into the server and records it in the session state; returns its capture id.
pub async fn load(s: &mut Session, file: &Path) -> Result<u64> {
    let id = s
        .client
        .load_capture(pb::LoadCaptureRequest {
            filepath: abs(file)?,
        })
        .await?
        .into_inner()
        .capture_info
        .ok_or_else(|| Error::server("load capture: server returned no capture info"))?
        .capture_id;
    s.state.captures.push(CaptureRecord {
        id,
        device: "file".into(),
        desc: file.display().to_string(),
        digital: vec![],
        analog: vec![],
        analyzers: vec![],
    });
    s.state.save()?;
    Ok(id)
}

/// Saves a capture as a `.sal` file.
pub async fn save(s: &mut Session, capture: u64, file: &Path) -> Result<()> {
    s.client
        .save_capture(pb::SaveCaptureRequest {
            capture_id: capture,
            filepath: abs(file)?,
        })
        .await?;
    Ok(())
}

/// Stops (a no-op unless still running) and closes a capture, freeing its memory.
pub async fn close(s: &mut Session, capture: u64) -> Result<()> {
    s.client
        .stop_capture(pb::StopCaptureRequest {
            capture_id: capture,
        })
        .await?;
    s.client
        .close_capture(pb::CloseCaptureRequest {
            capture_id: capture,
        })
        .await?;
    s.state.captures.retain(|c| c.id != capture);
    s.state.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_parsing() {
        assert_eq!(Edge::parse("rising").unwrap(), Edge::Rising);
        assert_eq!(Edge::parse("Pulse-High").unwrap(), Edge::PulseHigh);
        assert!(Edge::parse("sideways").is_err());
    }

    #[test]
    fn request_defaults_to_first_four_channels() {
        let opts = CaptureOptions::default();
        let (req, digital, analog) = opts.request(&[]).unwrap();
        assert_eq!(digital, [0, 1, 2, 3]);
        assert!(analog.is_empty());
        assert!(req.device_configuration.is_some());
    }

    #[test]
    fn trigger_channel_must_be_recorded() {
        let opts = CaptureOptions {
            digital: vec![0, 1],
            trigger: Some(TriggerSpec {
                channel: 5,
                edge: Edge::Rising,
                after: 0.1,
                min_pulse: None,
                max_pulse: None,
                linked: vec![],
                timeout: 10.0,
            }),
            ..Default::default()
        };
        assert!(opts.request(&[]).is_err());
    }
}
