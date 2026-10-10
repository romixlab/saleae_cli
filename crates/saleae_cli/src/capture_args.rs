//! Clap command line form for a capture, turned into [`saleae_rs::capture::CaptureOptions`] by [`CaptureArgs::options`].

use crate::{complete, parse};
use anyhow::{Context, Result, bail};
use clap::Args;
use clap_complete::ArgValueCandidates;
use saleae_rs::capture::{CaptureOptions, Edge, LinkState, LinkedChannel, TriggerSpec};

#[derive(Args, Debug, Clone)]
pub struct CaptureArgs {
    /// Device id (serial) from `saleae devices`; default: the first real device. Simulated: F4241 (Logic Pro 16),
    /// F4244 (Logic Pro 8), F4243 (Logic 8).
    #[arg(short, long, env = "SALEAE_DEVICE", add = ArgValueCandidates::new(complete::device_ids), global = true)]
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

impl CaptureArgs {
    pub fn options(&self) -> Result<CaptureOptions> {
        let mut glitch = vec![];
        for g in &self.glitch {
            let (ch, w) = parse::channel_pair(g)?;
            glitch.push((ch, parse::duration(&w)?));
        }
        let trigger = match &self.trigger {
            None => None,
            Some(t) => {
                let (ch, edge) = match t.split_once(':') {
                    Some((c, e)) => (c, Edge::parse(e)?),
                    None => (t.as_str(), Edge::Rising),
                };
                let channel: u32 = ch
                    .trim()
                    .parse()
                    .with_context(|| format!("bad trigger channel `{t}`"))?;
                let mut linked = vec![];
                for l in &self.link {
                    let (c, state) = parse::channel_pair(l)?;
                    let state = match state.to_ascii_lowercase().as_str() {
                        "high" | "1" => LinkState::High,
                        "low" | "0" => LinkState::Low,
                        s => bail!("linked channel state `{s}` is not high or low"),
                    };
                    linked.push(LinkedChannel { channel: c, state });
                }
                Some(TriggerSpec {
                    channel,
                    edge,
                    after: self.after,
                    min_pulse: self.min_pulse,
                    max_pulse: self.max_pulse,
                    linked,
                    timeout: self.timeout,
                })
            }
        };
        Ok(CaptureOptions {
            device: self.device.clone(),
            digital: self.digital.clone().unwrap_or_default().0,
            analog: self.analog.clone().unwrap_or_default().0,
            rate: self.rate,
            analog_rate: self.analog_rate,
            threshold: self.threshold,
            duration: self.duration,
            trigger,
            trim: self.trim,
            glitch,
            buffer_mb: self.buffer_mb,
        })
    }
}
