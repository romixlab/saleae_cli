//! Protocol analyzers: typed options for the common ones (SPI, I2C, Async Serial, CAN, LIN, 1-Wire), turned into
//! the setting names and option texts the Saleae analyzers expect, and a generic form for any other analyzer.

use crate::error::{Error, Result};
use crate::pb;
use crate::pb::AnalyzerSettingValue;
use crate::pb::analyzer_setting_value::Value;
use crate::server::{AnalyzerRecord, Session};
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

/// Extra raw settings, applied after the typed ones (overriding them); key is the setting name as Logic 2 shows
/// it, value an already-typed [`AnalyzerSettingValue`].
pub type Overrides = HashMap<String, AnalyzerSettingValue>;

#[derive(Debug, Clone)]
pub enum Protocol {
    /// SPI: clock plus MOSI and/or MISO, optional enable (chip select).
    Spi(SpiOptions),
    /// I2C: SDA and SCL.
    I2c(I2cOptions),
    /// Async serial (UART): one data line.
    Serial(SerialOptions),
    /// CAN: one line (RX of the transceiver, or CAN H with `inverted`).
    Can(CanOptions),
    /// LIN: one line.
    Lin(LinOptions),
    /// 1-Wire: one data line.
    OneWire(OneWireOptions),
    /// Any other analyzer by its Logic 2 name, configured entirely through `overrides`.
    Other(OtherOptions),
}

#[derive(Debug, Clone)]
pub struct SpiOptions {
    pub clock: u32,
    pub mosi: Option<u32>,
    pub miso: Option<u32>,
    pub enable: Option<u32>,
    /// SPI mode 0-3 (CPOL = mode / 2, CPHA = mode % 2).
    pub mode: u8,
    /// Bits per transfer (1-64).
    pub bits: u32,
    pub lsb_first: bool,
    /// Enable line is active high.
    pub cs_active_high: bool,
    pub overrides: Overrides,
}

#[derive(Debug, Clone)]
pub struct I2cOptions {
    pub sda: u32,
    pub scl: u32,
    pub overrides: Overrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone)]
pub struct SerialOptions {
    /// Data channel (TX or RX line of the device).
    pub channel: u32,
    /// Bit rate.
    pub baud: f64,
    /// Data bits per frame (1-64).
    pub bits: u32,
    /// Stop bits: 1, 1.5 or 2.
    pub stop: f64,
    pub parity: Parity,
    /// Most significant bit first (UART is LSB first).
    pub msb_first: bool,
    /// Inverted signal (idle low, e.g. RS-232 levels after a non-inverting buffer).
    pub inverted: bool,
    pub overrides: Overrides,
}

#[derive(Debug, Clone)]
pub struct CanOptions {
    pub channel: u32,
    pub bitrate: f64,
    /// The probe is on CAN H instead of the transceiver's RX.
    pub inverted: bool,
    pub overrides: Overrides,
}

#[derive(Debug, Clone)]
pub struct LinOptions {
    pub channel: u32,
    pub bitrate: f64,
    /// LIN specification version, 1 or 2 (checksum type).
    pub lin_version: u8,
    pub overrides: Overrides,
}

#[derive(Debug, Clone)]
pub struct OneWireOptions {
    pub channel: u32,
    pub overrides: Overrides,
}

#[derive(Debug, Clone)]
pub struct OtherOptions {
    /// Analyzer name as Logic 2 shows it, e.g. "Manchester", "I2S / PCM".
    pub name: String,
    /// Digital channels the analyzer reads, enabled for `decode` captures (the channel settings themselves go in
    /// `overrides`, e.g. `Manchester=0`; a wrong name makes the server list the valid ones).
    pub channels: Vec<u32>,
    pub overrides: Overrides,
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

pub fn kind_of(name: &str) -> Kind {
    match name {
        "SPI" => Kind::Spi {
            mosi: true,
            miso: true,
        },
        "I2C" => Kind::I2c,
        "Async Serial" => Kind::Serial,
        "CAN" => Kind::Can,
        _ => Kind::Other,
    }
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
        let (name, channels, kind, overrides) = match self {
            Protocol::Spi(a) => {
                if a.mosi.is_none() && a.miso.is_none() {
                    return Err(Error::invalid("SPI needs mosi and/or miso"));
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
                    &a.overrides,
                )
            }
            Protocol::I2c(a) => {
                if a.sda == a.scl {
                    return Err(Error::invalid("SDA and SCL must be different channels"));
                }
                s.push(("SDA", int(a.sda)));
                s.push(("SCL", int(a.scl)));
                ("I2C", vec![a.sda, a.scl], Kind::I2c, &a.overrides)
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
                ("Async Serial", vec![a.channel], Kind::Serial, &a.overrides)
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
                ("CAN", vec![a.channel], Kind::Can, &a.overrides)
            }
            Protocol::Lin(a) => {
                s.push(("Serial", int(a.channel)));
                s.push(("Bit Rate (Bits/s)", int(a.bitrate.round() as i64)));
                s.push(("LIN Version", text(format!("Version {}.x", a.lin_version))));
                ("LIN", vec![a.channel], Kind::Other, &a.overrides)
            }
            Protocol::OneWire(a) => {
                s.push(("1-Wire", int(a.channel)));
                ("1-Wire", vec![a.channel], Kind::Other, &a.overrides)
            }
            Protocol::Other(a) => {
                let kind = kind_of(&a.name);
                (a.name.as_str(), a.channels.clone(), kind, &a.overrides)
            }
        };
        let mut settings: HashMap<String, AnalyzerSettingValue> =
            s.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        for (k, v) in overrides {
            settings.insert(k.clone(), v.clone());
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

/// Adds an analyzer to a capture and records it in the session state; returns its id and label.
pub async fn add(
    s: &mut Session,
    capture: u64,
    spec: &Spec,
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
        .await?
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

/// Removes an analyzer from a capture.
pub async fn remove(s: &mut Session, capture: u64, analyzer: u64) -> Result<()> {
    s.client
        .remove_analyzer(pb::RemoveAnalyzerRequest {
            capture_id: capture,
            analyzer_id: analyzer,
        })
        .await?;
    if let Some(c) = s.state.capture_mut(capture) {
        c.analyzers.retain(|a| a.id != analyzer);
    }
    s.state.save()
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

    #[test]
    fn spi_mode_3() {
        let s = Protocol::Spi(SpiOptions {
            clock: 1,
            mosi: Some(0),
            miso: None,
            enable: Some(2),
            mode: 3,
            bits: 8,
            lsb_first: false,
            cs_active_high: false,
            overrides: Overrides::new(),
        })
        .spec()
        .unwrap();
        assert_eq!(s.name, "SPI");
        assert_eq!(s.channels, [0, 1, 2]);
        assert_eq!(s.settings["Clock"].value, Some(Value::Int64Value(1)));
        assert_eq!(
            s.settings["Clock State"].value,
            Some(Value::StringValue(
                "Clock is High when inactive (CPOL = 1)".into()
            ))
        );
        assert_eq!(
            s.settings["Clock Phase"].value,
            Some(Value::StringValue(
                "Data is Valid on Clock Trailing Edge (CPHA = 1)".into()
            ))
        );
        assert!(!s.settings.contains_key("MISO"));
    }

    #[test]
    fn serial_and_overrides() {
        let mut overrides = Overrides::new();
        overrides.insert("Mode".into(), text("Normal"));
        let s = Protocol::Serial(SerialOptions {
            channel: 3,
            baud: 1_000_000.0,
            bits: 8,
            stop: 1.0,
            parity: Parity::Even,
            msb_first: false,
            inverted: false,
            overrides,
        })
        .spec()
        .unwrap();
        assert_eq!(s.name, "Async Serial");
        assert_eq!(
            s.settings["Bit Rate (Bits/s)"].value,
            Some(Value::Int64Value(1_000_000))
        );
        assert_eq!(
            s.settings["Parity Bit"].value,
            Some(Value::StringValue("Even Parity Bit".into()))
        );
        let mut overrides = Overrides::new();
        overrides.insert("Bit Rate (Bits/s)".into(), int(9600));
        let s = Protocol::Serial(SerialOptions {
            channel: 0,
            baud: 115200.0,
            bits: 8,
            stop: 1.0,
            parity: Parity::None,
            msb_first: false,
            inverted: false,
            overrides,
        })
        .spec()
        .unwrap();
        assert_eq!(
            s.settings["Bit Rate (Bits/s)"].value,
            Some(Value::Int64Value(9600))
        );
    }

    #[test]
    fn spi_needs_data_line() {
        let p = Protocol::Spi(SpiOptions {
            clock: 1,
            mosi: None,
            miso: None,
            enable: None,
            mode: 0,
            bits: 8,
            lsb_first: false,
            cs_active_high: false,
            overrides: Overrides::new(),
        });
        assert!(p.spec().is_err());
    }
}
