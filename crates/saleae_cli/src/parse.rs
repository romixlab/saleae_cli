//! Parsers for command line values: durations (`10ms`), rates (`10M`), channel lists (`0-3,6`) and analyzer
//! setting values.

use anyhow::{Context, Result, bail};
use saleae_rs::pb::AnalyzerSettingValue;
use saleae_rs::pb::analyzer_setting_value::Value;

/// Seconds from `1.5`, `250ms`, `10us`, `100ns`, `2s` or `1m` (minutes).
pub fn duration(s: &str) -> Result<f64> {
    let s = s.trim();
    let split = s
        .find(|c: char| c.is_ascii_alphabetic() || c == 'µ')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num
        .trim()
        .parse()
        .with_context(|| format!("bad duration `{s}`"))?;
    let scale = match unit.trim() {
        "" | "s" => 1.0,
        "ms" => 1e-3,
        "us" | "µs" => 1e-6,
        "ns" => 1e-9,
        "m" | "min" => 60.0,
        u => bail!("bad duration unit `{u}` in `{s}` (s, ms, us, ns, min)"),
    };
    Ok(v * scale)
}

/// Samples or bits per second from `10M`, `500k`, `115200`, `1e6`, `2.5MHz`, `500kbps`.
pub fn rate(s: &str) -> Result<f64> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let t = lower
        .trim_end_matches("bps")
        .trim_end_matches("hz")
        .trim_end_matches("sps")
        .trim_end_matches("s/s")
        .trim_end_matches("sa/s");
    let (num, scale) = match t.chars().last() {
        Some('k') => (&t[..t.len() - 1], 1e3),
        Some('m') => (&t[..t.len() - 1], 1e6),
        Some('g') => (&t[..t.len() - 1], 1e9),
        _ => (t, 1.0),
    };
    let v: f64 = num
        .trim()
        .parse()
        .with_context(|| format!("bad rate `{s}`"))?;
    Ok(v * scale)
}

/// A channel list argument (`0-3,6`); a newtype so clap takes it as one value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Channels(pub Vec<u32>);

pub fn channel_list(s: &str) -> Result<Channels> {
    channels(s).map(Channels)
}

/// Channel indices from `0-3,6`, sorted and deduplicated.
pub fn channels(s: &str) -> Result<Vec<u32>> {
    let mut out = vec![];
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            let a: u32 = a
                .trim()
                .parse()
                .with_context(|| format!("bad channel range `{part}`"))?;
            let b: u32 = b
                .trim()
                .parse()
                .with_context(|| format!("bad channel range `{part}`"))?;
            if a > b {
                bail!("bad channel range `{part}`");
            }
            out.extend(a..=b);
        } else {
            out.push(
                part.parse()
                    .with_context(|| format!("bad channel `{part}`"))?,
            );
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// `KEY=VALUE` into an analyzer setting. Integers become int64 (channel numbers, bit rates), `true`/`false`
/// booleans, other numbers doubles, anything else a string (the option text as shown in Logic 2).
/// A value in quotes (`'="8"'`) stays a string.
pub fn setting(s: &str) -> Result<(String, AnalyzerSettingValue)> {
    let (k, v) = s
        .split_once('=')
        .with_context(|| format!("setting `{s}` is not KEY=VALUE"))?;
    Ok((k.trim().to_string(), setting_value(v.trim())))
}

pub fn setting_value(v: &str) -> AnalyzerSettingValue {
    let value = if let Some(q) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Value::StringValue(q.to_string())
    } else if let Ok(i) = v.parse::<i64>() {
        Value::Int64Value(i)
    } else if v == "true" || v == "false" {
        Value::BoolValue(v == "true")
    } else if let Ok(f) = v.parse::<f64>() {
        Value::DoubleValue(f)
    } else {
        Value::StringValue(v.to_string())
    };
    AnalyzerSettingValue { value: Some(value) }
}

/// `CH=VALUE` pairs such as `--glitch 0=1us` or `--link 2=high`.
pub fn channel_pair(s: &str) -> Result<(u32, String)> {
    let (c, v) = s
        .split_once('=')
        .with_context(|| format!("`{s}` is not CHANNEL=VALUE"))?;
    Ok((
        c.trim()
            .parse()
            .with_context(|| format!("bad channel in `{s}`"))?,
        v.trim().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration("2").unwrap(), 2.0);
        assert_eq!(duration("250ms").unwrap(), 0.25);
        assert!((duration("10us").unwrap() - 10e-6).abs() < 1e-15);
        assert_eq!(duration("1min").unwrap(), 60.0);
        assert!(duration("5 parsecs").is_err());
    }

    #[test]
    fn rates() {
        assert_eq!(rate("10M").unwrap(), 10e6);
        assert_eq!(rate("500k").unwrap(), 500e3);
        assert_eq!(rate("115200").unwrap(), 115200.0);
        assert_eq!(rate("1e6").unwrap(), 1e6);
        assert_eq!(rate("2.5MHz").unwrap(), 2.5e6);
        assert_eq!(rate("500kbps").unwrap(), 500e3);
        assert_eq!(rate("50 MS/s").unwrap(), 50e6);
    }

    #[test]
    fn channel_lists() {
        assert_eq!(channels("0-3,6,2").unwrap(), [0, 1, 2, 3, 6]);
        assert_eq!(channels("5").unwrap(), [5]);
        assert!(channels("3-1").is_err());
    }

    #[test]
    fn settings() {
        let (k, v) = setting("Clock=1").unwrap();
        assert_eq!(k, "Clock");
        assert_eq!(v.value, Some(Value::Int64Value(1)));
        let (_, v) = setting("Bits per Transfer=8 Bits per Transfer (Standard)").unwrap();
        assert_eq!(
            v.value,
            Some(Value::StringValue("8 Bits per Transfer (Standard)".into()))
        );
        let (_, v) = setting("X=\"8\"").unwrap();
        assert_eq!(v.value, Some(Value::StringValue("8".into())));
        let (_, v) = setting("Inverted=true").unwrap();
        assert_eq!(v.value, Some(Value::BoolValue(true)));
    }
}
