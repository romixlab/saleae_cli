//! Protocol analyzers: typed shorthands for the common ones (SPI, I2C, Async Serial, CAN, LIN, 1-Wire), turned into
//! the setting names and option texts the Saleae analyzers expect, and a generic form for any other analyzer.

use crate::parse;
use crate::pb::AnalyzerSettingValue;
use crate::pb::analyzer_setting_value::Value;
use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use std::collections::HashMap;

/// Analyzers bundled with the headless server (`Analyzers/` next to it), by the name `AddAnalyzer` takes.
pub const BUNDLED: &[&str] = &[
    "Async Serial",
    "I2C",
    "SPI",
    "CAN",
    "LIN",
    "1-Wire",
    "I2S / PCM",
    "Manchester",
    "Modbus",
    "SWD",
    "JTAG",
    "MDIO",
    "SMBus",
    "USB LS and FS",
    "DMX-512",
    "HD44780",
    "HDLC",
    "HDMI CEC",
    "MIDI",
    "PS/2 Keyboard/Mouse",
    "Simple Parallel",
    "BiSS C",
    "Atmel SWI",
    "Async RGB LED",
    "MCS-04 (4004)",
];

#[derive(Subcommand, Debug, Clone)]
pub enum Protocol {
    /// SPI: clock plus MOSI and/or MISO, optional enable (chip select).
    Spi(SpiArgs),
    /// I2C: SDA and SCL.
    I2c(I2cArgs),
    /// Async serial (UART): one data line.
    #[command(visible_aliases = ["uart", "async"])]
    Serial(SerialArgs),
    /// CAN: one line (RX of the transceiver, or CAN H with --inverted).
    Can(CanArgs),
    /// LIN: one line.
    Lin(LinArgs),
    /// 1-Wire: one data line.
    #[command(name = "onewire", visible_alias = "1wire")]
    OneWire(OneWireArgs),
    /// Any other analyzer by its Logic 2 name, configured with --set (see `saleae analyzer list`).
    Other(OtherArgs),
}

/// Extra raw settings, applied after the shorthand ones (overriding them).
#[derive(Args, Debug, Clone, Default)]
pub struct RawSettings {
    /// Analyzer setting as shown in Logic 2, repeatable: `--set "Bits per Transfer=16 Bits per Transfer"`.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    pub set: Vec<String>,
}

#[derive(Args, Debug, Clone)]
pub struct SpiArgs {
    /// Clock channel.
    #[arg(long, visible_alias = "clk", visible_alias = "sck")]
    pub clock: u32,
    /// MOSI channel.
    #[arg(long)]
    pub mosi: Option<u32>,
    /// MISO channel.
    #[arg(long)]
    pub miso: Option<u32>,
    /// Enable / chip select channel.
    #[arg(long, visible_alias = "cs")]
    pub enable: Option<u32>,
    /// SPI mode 0-3 (CPOL = mode / 2, CPHA = mode % 2).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
    pub mode: u8,
    /// Bits per transfer (1-64).
    #[arg(long, default_value_t = 8)]
    pub bits: u32,
    /// Least significant bit first.
    #[arg(long)]
    pub lsb_first: bool,
    /// Enable line is active high.
    #[arg(long)]
    pub cs_active_high: bool,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(Args, Debug, Clone)]
pub struct I2cArgs {
    #[arg(long)]
    pub sda: u32,
    #[arg(long)]
    pub scl: u32,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Args, Debug, Clone)]
pub struct SerialArgs {
    /// Data channel (TX or RX line of the device).
    #[arg(long, visible_alias = "rx", visible_alias = "tx")]
    pub channel: u32,
    /// Bit rate (`115200`, `1M`).
    #[arg(long, default_value = "115200", value_parser = parse::rate)]
    pub baud: f64,
    /// Data bits per frame (1-64).
    #[arg(long, default_value_t = 8)]
    pub bits: u32,
    /// Stop bits: 1, 1.5 or 2.
    #[arg(long, default_value_t = 1.0)]
    pub stop: f64,
    #[arg(long, value_enum, default_value_t = Parity::None)]
    pub parity: Parity,
    /// Most significant bit first (UART is LSB first).
    #[arg(long)]
    pub msb_first: bool,
    /// Inverted signal (idle low, e.g. RS-232 levels after a non-inverting buffer).
    #[arg(long)]
    pub inverted: bool,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(Args, Debug, Clone)]
pub struct CanArgs {
    #[arg(long, visible_alias = "rx")]
    pub channel: u32,
    /// Bit rate (`500k`, `1M`).
    #[arg(long, default_value = "500k", value_parser = parse::rate)]
    pub bitrate: f64,
    /// The probe is on CAN H instead of the transceiver's RX.
    #[arg(long)]
    pub inverted: bool,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(Args, Debug, Clone)]
pub struct LinArgs {
    #[arg(long)]
    pub channel: u32,
    #[arg(long, default_value = "19200", value_parser = parse::rate)]
    pub bitrate: f64,
    /// LIN specification version, 1 or 2 (checksum type).
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=2))]
    pub lin_version: u8,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(Args, Debug, Clone)]
pub struct OneWireArgs {
    #[arg(long)]
    pub channel: u32,
    #[command(flatten)]
    pub raw: RawSettings,
}

#[derive(Args, Debug, Clone)]
pub struct OtherArgs {
    /// Analyzer name as Logic 2 shows it, e.g. "Manchester", "I2S / PCM".
    #[arg(add = clap_complete::ArgValueCandidates::new(crate::complete::analyzer_names))]
    pub name: String,
    /// Digital channels the analyzer reads, enabled for `decode` captures (the channel settings themselves go in
    /// --set, e.g. `--set Manchester=0`; a wrong name makes the server list the valid ones).
    #[arg(long, value_parser = parse::channel_list)]
    pub channels: Option<parse::Channels>,
    #[command(flatten)]
    pub raw: RawSettings,
}

/// An analyzer ready for `AddAnalyzer`.
#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub settings: HashMap<String, AnalyzerSettingValue>,
    /// Digital channels it reads.
    pub channels: Vec<u32>,
    /// Which summary fits its data table.
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Which data lines are wired (an unwired one decodes as all zeros).
    Spi {
        mosi: bool,
        miso: bool,
    },
    I2c,
    Serial,
    Can,
    Other,
}

fn int(v: impl Into<i64>) -> AnalyzerSettingValue {
    AnalyzerSettingValue {
        value: Some(Value::Int64Value(v.into())),
    }
}

fn text(v: impl Into<String>) -> AnalyzerSettingValue {
    AnalyzerSettingValue {
        value: Some(Value::StringValue(v.into())),
    }
}

fn bits_text(bits: u32, per: &str) -> String {
    match bits {
        1 => format!("1 Bit per {per}"),
        8 => format!("8 Bits per {per} (Standard)"),
        n => format!("{n} Bits per {per}"),
    }
}

impl Protocol {
    pub fn spec(&self) -> Result<Spec> {
        let mut s: Vec<(&str, AnalyzerSettingValue)> = vec![];
        let (name, channels, kind, raw) = match self {
            Protocol::Spi(a) => {
                if a.mosi.is_none() && a.miso.is_none() {
                    bail!("SPI needs --mosi and/or --miso");
                }
                s.push(("Clock", int(a.clock)));
                let mut ch = vec![a.clock];
                for (key, v) in [("MOSI", a.mosi), ("MISO", a.miso), ("Enable", a.enable)] {
                    if let Some(c) = v {
                        s.push((key, int(c)));
                        ch.push(c);
                    }
                }
                s.push(("Bits per Transfer", text(bits_text(a.bits, "Transfer"))));
                s.push((
                    "Significant Bit",
                    text(if a.lsb_first {
                        "Least Significant Bit First"
                    } else {
                        "Most Significant Bit First (Standard)"
                    }),
                ));
                s.push((
                    "Clock State",
                    text(if a.mode >= 2 {
                        "Clock is High when inactive (CPOL = 1)"
                    } else {
                        "Clock is Low when inactive (CPOL = 0)"
                    }),
                ));
                s.push((
                    "Clock Phase",
                    text(if a.mode % 2 == 1 {
                        "Data is Valid on Clock Trailing Edge (CPHA = 1)"
                    } else {
                        "Data is Valid on Clock Leading Edge (CPHA = 0)"
                    }),
                ));
                if a.enable.is_some() {
                    s.push((
                        "Enable Line",
                        text(if a.cs_active_high {
                            "Enable line is Active High"
                        } else {
                            "Enable line is Active Low (Standard)"
                        }),
                    ));
                }
                (
                    "SPI",
                    ch,
                    Kind::Spi {
                        mosi: a.mosi.is_some(),
                        miso: a.miso.is_some(),
                    },
                    &a.raw,
                )
            }
            Protocol::I2c(a) => {
                if a.sda == a.scl {
                    bail!("SDA and SCL must be different channels");
                }
                s.push(("SDA", int(a.sda)));
                s.push(("SCL", int(a.scl)));
                ("I2C", vec![a.sda, a.scl], Kind::I2c, &a.raw)
            }
            Protocol::Serial(a) => {
                s.push(("Input Channel", int(a.channel)));
                s.push(("Bit Rate (Bits/s)", int(a.baud.round() as i64)));
                s.push(("Bits per Frame", text(bits_text(a.bits, "Transfer"))));
                let stop = if a.stop == 1.0 {
                    "1 Stop Bit (Standard)".to_string()
                } else {
                    format!("{} Stop Bits", a.stop)
                };
                s.push(("Stop Bits", text(stop)));
                s.push((
                    "Parity Bit",
                    text(match a.parity {
                        Parity::None => "No Parity Bit (Standard)",
                        Parity::Even => "Even Parity Bit",
                        Parity::Odd => "Odd Parity Bit",
                    }),
                ));
                s.push((
                    "Significant Bit",
                    text(if a.msb_first {
                        "Most Significant Bit Sent First"
                    } else {
                        "Least Significant Bit Sent First (Standard)"
                    }),
                ));
                s.push((
                    "Signal inversion",
                    text(if a.inverted {
                        "Inverted"
                    } else {
                        "Non Inverted (Standard)"
                    }),
                ));
                s.push(("Mode", text("Normal")));
                ("Async Serial", vec![a.channel], Kind::Serial, &a.raw)
            }
            Protocol::Can(a) => {
                s.push(("CAN", int(a.channel)));
                s.push(("Bit Rate (Bits/s)", int(a.bitrate.round() as i64)));
                s.push((
                    "Inverted (CAN High)",
                    AnalyzerSettingValue {
                        value: Some(Value::BoolValue(a.inverted)),
                    },
                ));
                ("CAN", vec![a.channel], Kind::Can, &a.raw)
            }
            Protocol::Lin(a) => {
                s.push(("Serial", int(a.channel)));
                s.push(("Bit Rate (Bits/s)", int(a.bitrate.round() as i64)));
                s.push(("LIN Version", text(format!("Version {}.x", a.lin_version))));
                ("LIN", vec![a.channel], Kind::Other, &a.raw)
            }
            Protocol::OneWire(a) => {
                s.push(("1-Wire", int(a.channel)));
                ("1-Wire", vec![a.channel], Kind::Other, &a.raw)
            }
            Protocol::Other(a) => {
                let kind = crate::summary::kind_of(&a.name);
                (
                    a.name.as_str(),
                    a.channels.clone().unwrap_or_default().0,
                    kind,
                    &a.raw,
                )
            }
        };
        let mut settings: HashMap<String, AnalyzerSettingValue> =
            s.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        for kv in &raw.set {
            let (k, v) = parse::setting(kv)?;
            settings.insert(k, v);
        }
        let mut channels = channels;
        channels.sort_unstable();
        channels.dedup();
        Ok(Spec {
            name: name.to_string(),
            settings,
            channels,
            kind,
        })
    }
}

/// Setting value as plain JSON, for `--json` output.
pub fn setting_json(v: &AnalyzerSettingValue) -> serde_json::Value {
    match &v.value {
        Some(Value::StringValue(s)) => s.clone().into(),
        Some(Value::Int64Value(i)) => (*i).into(),
        Some(Value::BoolValue(b)) => (*b).into(),
        Some(Value::DoubleValue(d)) => (*d).into(),
        // API 1.2 (SALEAE_PROTO_DIR from the server zip) adds MsoChannelValue; unreachable with the 1.0 proto
        #[allow(unreachable_patterns)]
        Some(other) => format!("{other:?}").into(),
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct T {
        #[command(subcommand)]
        p: Protocol,
    }

    fn spec(args: &[&str]) -> Spec {
        let mut v = vec!["t"];
        v.extend_from_slice(args);
        T::parse_from(v).p.spec().unwrap()
    }

    fn get<'a>(s: &'a Spec, k: &str) -> &'a Value {
        s.settings[k].value.as_ref().unwrap()
    }

    #[test]
    fn spi_mode_3() {
        let s = spec(&[
            "spi", "--clk", "1", "--mosi", "0", "--cs", "2", "--mode", "3",
        ]);
        assert_eq!(s.name, "SPI");
        assert_eq!(s.channels, [0, 1, 2]);
        assert_eq!(get(&s, "Clock"), &Value::Int64Value(1));
        assert_eq!(
            get(&s, "Clock State"),
            &Value::StringValue("Clock is High when inactive (CPOL = 1)".into())
        );
        assert_eq!(
            get(&s, "Clock Phase"),
            &Value::StringValue("Data is Valid on Clock Trailing Edge (CPHA = 1)".into())
        );
        assert!(!s.settings.contains_key("MISO"));
    }

    #[test]
    fn serial_and_overrides() {
        let s = spec(&[
            "uart",
            "--rx",
            "3",
            "--baud",
            "1M",
            "--parity",
            "even",
            "--set",
            "Mode=Normal",
        ]);
        assert_eq!(s.name, "Async Serial");
        assert_eq!(get(&s, "Bit Rate (Bits/s)"), &Value::Int64Value(1_000_000));
        assert_eq!(
            get(&s, "Parity Bit"),
            &Value::StringValue("Even Parity Bit".into())
        );
        let s = spec(&[
            "serial",
            "--channel",
            "0",
            "--set",
            "Bit Rate (Bits/s)=9600",
        ]);
        assert_eq!(get(&s, "Bit Rate (Bits/s)"), &Value::Int64Value(9600));
    }

    #[test]
    fn spi_needs_data_line() {
        let p = T::parse_from(["t", "spi", "--clock", "1"]).p;
        assert!(p.spec().is_err());
    }
}
