//! Clap command line forms for the typed analyzer shorthands (SPI, I2C, Async Serial, CAN, LIN, 1-Wire) and the
//! generic form for any other analyzer; turned into [`saleae_rs::analyzer::Protocol`] (and from there into a
//! [`saleae_rs::analyzer::Spec`]) by [`Protocol::spec`].

use crate::parse;
use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use saleae_rs::analyzer;

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

impl RawSettings {
    fn overrides(&self) -> Result<analyzer::Overrides> {
        self.set.iter().map(|kv| parse::setting(kv)).collect()
    }
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

impl From<Parity> for analyzer::Parity {
    fn from(p: Parity) -> Self {
        match p {
            Parity::None => analyzer::Parity::None,
            Parity::Even => analyzer::Parity::Even,
            Parity::Odd => analyzer::Parity::Odd,
        }
    }
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

impl Protocol {
    /// Turns the parsed CLI arguments into the lib's typed [`saleae_rs::analyzer::Protocol`].
    pub fn to_lib(&self) -> Result<analyzer::Protocol> {
        let p = match self {
            Protocol::Spi(a) => analyzer::Protocol::Spi(analyzer::SpiOptions {
                clock: a.clock,
                mosi: a.mosi,
                miso: a.miso,
                enable: a.enable,
                mode: a.mode,
                bits: a.bits,
                lsb_first: a.lsb_first,
                cs_active_high: a.cs_active_high,
                overrides: a.raw.overrides()?,
            }),
            Protocol::I2c(a) => analyzer::Protocol::I2c(analyzer::I2cOptions {
                sda: a.sda,
                scl: a.scl,
                overrides: a.raw.overrides()?,
            }),
            Protocol::Serial(a) => analyzer::Protocol::Serial(analyzer::SerialOptions {
                channel: a.channel,
                baud: a.baud,
                bits: a.bits,
                stop: a.stop,
                parity: a.parity.into(),
                msb_first: a.msb_first,
                inverted: a.inverted,
                overrides: a.raw.overrides()?,
            }),
            Protocol::Can(a) => analyzer::Protocol::Can(analyzer::CanOptions {
                channel: a.channel,
                bitrate: a.bitrate,
                inverted: a.inverted,
                overrides: a.raw.overrides()?,
            }),
            Protocol::Lin(a) => analyzer::Protocol::Lin(analyzer::LinOptions {
                channel: a.channel,
                bitrate: a.bitrate,
                lin_version: a.lin_version,
                overrides: a.raw.overrides()?,
            }),
            Protocol::OneWire(a) => analyzer::Protocol::OneWire(analyzer::OneWireOptions {
                channel: a.channel,
                overrides: a.raw.overrides()?,
            }),
            Protocol::Other(a) => analyzer::Protocol::Other(analyzer::OtherOptions {
                name: a.name.clone(),
                channels: a.channels.clone().unwrap_or_default().0,
                overrides: a.raw.overrides()?,
            }),
        };
        Ok(p)
    }

    /// Resolves the CLI arguments directly into an [`analyzer::Spec`] ready for `AddAnalyzer`.
    pub fn spec(&self) -> Result<analyzer::Spec> {
        Ok(self.to_lib()?.spec()?)
    }
}
