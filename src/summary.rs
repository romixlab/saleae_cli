//! Compact summaries of analyzer data tables (the CSV `ExportDataTableCsv` writes, hex radix), short enough for an
//! agent to read: I2C transactions, SPI transfers per chip-select, serial text, CAN frames, and a generic row list.
//!
//! Frame types and columns are the ones Logic 2 analyzers emit (see the HLA frame docs on support.saleae.com):
//! I2C `start`/`address`/`data`/`stop` with `address`, `read`, `ack`, `data`; SPI `enable`/`result`/`disable`
//! with `mosi`, `miso`; Async Serial `data` with `data`, `error`; CAN `identifier_field`, `control_field`,
//! `data_field`, `crc_field`, `ack_field`, `can_error`.

use crate::analyzers::Kind;
use crate::parse::fmt_seconds;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct Summary {
    pub header: String,
    pub lines: Vec<String>,
    pub json: Value,
}

impl Summary {
    pub fn text(&self) -> String {
        let mut t = format!("{}\n", self.header);
        for l in &self.lines {
            t += l;
            t.push('\n');
        }
        t
    }

    pub fn json(&self) -> Value {
        let mut v = self.json.clone();
        v["header"] = json!(self.header);
        v
    }
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

/// One data table row.
#[derive(Debug, Clone)]
struct Row {
    ty: String,
    t: f64,
    dur: f64,
    cols: BTreeMap<String, String>,
}

impl Row {
    fn get(&self, k: &str) -> Option<&str> {
        self.cols
            .get(k)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    fn flag(&self, k: &str) -> Option<bool> {
        self.get(k)
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
    }

    fn byte(&self, k: &str) -> Option<u64> {
        self.get(k).and_then(number)
    }
}

/// `0x1F`, `0b101`, `31`, `'A'` into a number.
fn number(v: &str) -> Option<u64> {
    let v = v.trim();
    if let Some(h) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if let Some(b) = v.strip_prefix("0b") {
        u64::from_str_radix(b, 2).ok()
    } else if v.len() == 3 && v.starts_with('\'') && v.ends_with('\'') {
        v.chars().nth(1).map(|c| c as u64)
    } else {
        v.parse().ok()
    }
}

/// Hex digits without prefix, two per byte.
fn hex(v: u64) -> String {
    if v <= 0xFF {
        format!("{v:02X}")
    } else {
        format!("{v:X}")
    }
}

fn parse_rows(data: &str) -> Result<Vec<Row>> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(data.as_bytes());
    let headers = rdr.headers().context("data table has no header")?.clone();
    let idx = |name: &str| headers.iter().position(|h| h == name);
    let (Some(ty), Some(t)) = (idx("type"), idx("start_time")) else {
        anyhow::bail!(
            "unexpected data table columns: {}",
            headers.iter().collect::<Vec<_>>().join(",")
        );
    };
    let dur = idx("duration");
    let fixed = ["name", "type", "start_time", "duration"];
    let mut rows = vec![];
    for rec in rdr.records() {
        let rec = rec.context("bad data table row")?;
        let mut cols = BTreeMap::new();
        for (i, h) in headers.iter().enumerate() {
            if !fixed.contains(&h)
                && let Some(v) = rec.get(i)
                && !v.is_empty()
            {
                cols.insert(h.to_string(), v.to_string());
            }
        }
        rows.push(Row {
            ty: rec.get(ty).unwrap_or_default().to_string(),
            t: rec.get(t).and_then(|v| v.parse().ok()).unwrap_or(0.0),
            dur: dur
                .and_then(|d| rec.get(d))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            cols,
        });
    }
    Ok(rows)
}

pub fn summarize(kind: Kind, data: &str, limit: usize) -> Result<Summary> {
    let rows = parse_rows(data)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *counts.entry(r.ty.clone()).or_default() += 1;
    }
    let errors: usize = rows
        .iter()
        .filter(|r| r.ty.contains("error") || r.get("error").is_some())
        .count();
    let mut lines = vec![];
    let span = match (rows.first(), rows.last()) {
        (Some(a), Some(b)) => format!(", {} .. {}", fmt_seconds(a.t), fmt_seconds(b.t + b.dur)),
        _ => String::new(),
    };
    let count_txt: Vec<_> = counts.iter().map(|(k, v)| format!("{k} {v}")).collect();
    lines.push(format!(
        "{} frames ({}){span}{}",
        rows.len(),
        if count_txt.is_empty() {
            "none".to_string()
        } else {
            count_txt.join(", ")
        },
        if errors > 0 {
            format!(", {errors} with errors")
        } else {
            String::new()
        }
    ));
    let (items, extra) = match kind {
        Kind::I2c => i2c(&rows),
        Kind::Spi { mosi, miso } => spi(&rows, mosi, miso),
        Kind::Serial => serial(&rows),
        Kind::Can => can(&rows),
        Kind::Other => generic(&rows),
    };
    lines.extend(extra);
    let total = items.len();
    for it in items.iter().take(limit) {
        lines.push(it.0.clone());
    }
    if total > limit {
        lines.push(format!(
            "... {} more (raise --limit, or --csv FILE for the full table)",
            total - limit
        ));
    }
    if total == 0 && !rows.is_empty() {
        lines.push(
            "no complete transactions decoded, only the frames counted above: check channel assignment, wiring, \
             analyzer settings and sample rate"
                .into(),
        );
    }
    if rows.is_empty() {
        lines.push(
            "nothing decoded: check channels, wiring, ground, logic threshold (-V), sample rate (>= 4x the bit \
             rate) and that the bus was active during the capture"
                .into(),
        );
    }
    let json = json!({
        "frames": rows.len(),
        "counts": counts,
        "errors": errors,
        "start_s": rows.first().map(|r| r.t),
        "end_s": rows.last().map(|r| r.t + r.dur),
        "items": items.iter().take(limit).map(|i| i.1.clone()).collect::<Vec<_>>(),
        "items_total": total,
        "truncated": total > limit,
    });
    Ok(Summary {
        header: String::new(),
        lines,
        json,
    })
}

type Items = (Vec<(String, Value)>, Vec<String>);

/// I2C: one item per START..STOP, repeated starts inside: `W 0x50 ACK: 00 10 | R 0x50: AB CD NAK`.
fn i2c(rows: &[Row]) -> Items {
    let mut items = vec![];
    let mut addr_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut cur: Option<(f64, Vec<String>, Vec<Value>)> = None;
    let mut seg = String::new();
    let flush_seg = |seg: &mut String, parts: &mut Vec<String>| {
        if !seg.is_empty() {
            parts.push(std::mem::take(seg));
        }
    };
    for r in rows {
        match r.ty.as_str() {
            "start" => {
                if let Some((_, parts, _)) = cur.as_mut() {
                    flush_seg(&mut seg, parts);
                } else {
                    cur = Some((r.t, vec![], vec![]));
                }
            }
            "address" => {
                let (t0, parts, segs) = cur.get_or_insert_with(|| (r.t, vec![], vec![]));
                let _ = t0;
                flush_seg(&mut seg, parts);
                let read = r.flag("read").unwrap_or(false);
                let ack = r.flag("ack").unwrap_or(true);
                let addr = r
                    .get("address")
                    .map(|a| number(a).map(|n| format!("0x{n:02X}")).unwrap_or(a.into()));
                let addr = addr.unwrap_or_else(|| "?".into());
                let e = addr_counts.entry(addr.clone()).or_default();
                if ack {
                    e.0 += 1
                } else {
                    e.1 += 1
                }
                seg = format!(
                    "{} {addr}{}",
                    if read { "R" } else { "W" },
                    if ack { "" } else { " NAK" }
                );
                if ack {
                    seg.push(':');
                }
                segs.push(json!({ "address": addr, "read": read, "ack": ack, "data": [] }));
            }
            "data" => {
                let (_, _, segs) = cur.get_or_insert_with(|| (r.t, vec![], vec![]));
                let b = r.byte("data").map(hex).unwrap_or_else(|| "??".into());
                seg.push(' ');
                seg.push_str(&b);
                if r.flag("ack") == Some(false) {
                    seg.push_str(" NAK");
                }
                if let Some(Value::Array(d)) = segs.last_mut().and_then(|s| s.get_mut("data")) {
                    d.push(json!(b));
                }
            }
            "stop" => {
                if let Some((t0, mut parts, segs)) = cur.take() {
                    flush_seg(&mut seg, &mut parts);
                    let text = if parts.is_empty() {
                        "START STOP (no address)".into()
                    } else {
                        parts.join(" | ")
                    };
                    items.push((
                        format!("{:>10}  {text}", fmt_seconds(t0)),
                        json!({ "t": t0, "segments": segs }),
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some((t0, mut parts, segs)) = cur.take() {
        flush_seg(&mut seg, &mut parts);
        if !parts.is_empty() {
            items.push((
                format!("{:>10}  {} (no STOP)", fmt_seconds(t0), parts.join(" | ")),
                json!({ "t": t0, "segments": segs, "no_stop": true }),
            ));
        }
    }
    let mut extra = vec![];
    if !addr_counts.is_empty() {
        let a: Vec<_> = addr_counts
            .iter()
            .map(|(k, (ack, nak))| {
                if *nak > 0 {
                    format!("{k} ({ack} ACK, {nak} NAK)")
                } else {
                    format!("{k} ({ack})")
                }
            })
            .collect();
        extra.push(format!("addresses: {}", a.join(", ")));
    }
    (items, extra)
}

/// SPI: one item per chip-select window (or per 16 transfers without an enable line).
fn spi(rows: &[Row], show_mosi: bool, show_miso: bool) -> Items {
    let mut items = vec![];
    let mut cur: Option<(f64, Vec<String>, Vec<String>)> = None;
    let has_enable = rows.iter().any(|r| r.ty == "enable");
    let push = |items: &mut Vec<(String, Value)>, t0: f64, mosi: &[String], miso: &[String]| {
        if mosi.is_empty() && miso.is_empty() {
            return;
        }
        let mut text = format!("{:>10} ", fmt_seconds(t0));
        if show_mosi && mosi.iter().any(|m| !m.is_empty()) {
            text += &format!(" MOSI: {}", mosi.join(" "));
        }
        if show_miso && miso.iter().any(|m| !m.is_empty()) {
            text += &format!(" MISO: {}", miso.join(" "));
        }
        let mut v = json!({ "t": t0 });
        if show_mosi {
            v["mosi"] = json!(mosi);
        }
        if show_miso {
            v["miso"] = json!(miso);
        }
        items.push((text, v));
    };
    for r in rows {
        match r.ty.as_str() {
            "enable" => {
                if let Some((t0, mo, mi)) = cur.take() {
                    push(&mut items, t0, &mo, &mi);
                }
                cur = Some((r.t, vec![], vec![]));
            }
            "result" => {
                let (_, mo, mi) = cur.get_or_insert_with(|| (r.t, vec![], vec![]));
                mo.push(r.byte("mosi").map(hex).unwrap_or_default());
                mi.push(r.byte("miso").map(hex).unwrap_or_default());
                if !has_enable && mo.len() >= 16 {
                    let (t0, mo, mi) = cur.take().unwrap_or_default();
                    push(&mut items, t0, &mo, &mi);
                }
            }
            "disable" => {
                if let Some((t0, mo, mi)) = cur.take() {
                    push(&mut items, t0, &mo, &mi);
                }
            }
            _ => {}
        }
    }
    if let Some((t0, mo, mi)) = cur.take() {
        push(&mut items, t0, &mo, &mi);
    }
    (items, vec![])
}

/// Async serial: the bytes as text lines (escaped), or hex rows when the data is mostly binary.
fn serial(rows: &[Row]) -> Items {
    let data: Vec<&Row> = rows.iter().filter(|r| r.ty == "data").collect();
    let bytes: Vec<u64> = data.iter().filter_map(|r| r.byte("data")).collect();
    let mut extra = vec![];
    let mut errs: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &data {
        if let Some(e) = r.get("error") {
            *errs.entry(e).or_default() += 1;
        }
    }
    if !errs.is_empty() {
        let e: Vec<_> = errs.iter().map(|(k, v)| format!("{k} {v}")).collect();
        extra.push(format!(
            "errors: {} (wrong baud rate, inverted signal, or noise if most frames have them)",
            e.join(", ")
        ));
    }
    let printable = bytes
        .iter()
        .filter(|&&b| {
            (0x20..0x7F).contains(&b) || b == b'\n' as u64 || b == b'\r' as u64 || b == b'\t' as u64
        })
        .count();
    let mut items = vec![];
    if !bytes.is_empty() && printable * 10 >= bytes.len() * 8 {
        // text: split on newlines
        let mut line = String::new();
        let mut t0 = None;
        for (r, &b) in data.iter().zip(&bytes) {
            t0.get_or_insert(r.t);
            match b {
                0x0A => {
                    let t = t0.take().unwrap_or(r.t);
                    items.push((
                        format!("{:>10}  {line}", fmt_seconds(t)),
                        json!({ "t": t, "text": line }),
                    ));
                    line.clear();
                }
                0x0D => {}
                0x20..=0x7E => line.push(b as u8 as char),
                b => line += &format!("\\x{b:02X}"),
            }
        }
        if !line.is_empty() {
            let t = t0.unwrap_or(0.0);
            items.push((
                format!("{:>10}  {line}", fmt_seconds(t)),
                json!({ "t": t, "text": line }),
            ));
        }
        extra.push("text (one line per newline):".into());
    } else {
        for chunk in data.chunks(16) {
            let t = chunk[0].t;
            let hexes: Vec<String> = chunk
                .iter()
                .map(|r| r.byte("data").map(hex).unwrap_or("??".into()))
                .collect();
            items.push((
                format!("{:>10}  {}", fmt_seconds(t), hexes.join(" ")),
                json!({ "t": t, "bytes": hexes }),
            ));
        }
    }
    (items, extra)
}

/// CAN: one item per frame, `0x123 [3] 01 02 03 ACK`.
fn can(rows: &[Row]) -> Items {
    let mut items = vec![];
    let mut cur: Option<(f64, Value, String)> = None;
    let finish = |items: &mut Vec<(String, Value)>, c: Option<(f64, Value, String)>| {
        if let Some((t, v, text)) = c {
            items.push((format!("{:>10}  {text}", fmt_seconds(t)), v));
        }
    };
    for r in rows {
        match r.ty.as_str() {
            "identifier_field" => {
                finish(&mut items, cur.take());
                let id = r
                    .get("identifier")
                    .and_then(number)
                    .map(|n| format!("0x{n:03X}"))
                    .unwrap_or("?".into());
                let ext = r.flag("extended").unwrap_or(false);
                let rtr = r.flag("remote_frame").unwrap_or(false);
                let text = format!(
                    "{id}{}{}",
                    if ext { " EXT" } else { "" },
                    if rtr { " RTR" } else { "" }
                );
                cur = Some((
                    r.t,
                    json!({ "t": r.t, "id": id, "extended": ext, "rtr": rtr, "data": [] }),
                    text,
                ));
            }
            "control_field" => {
                if let Some((_, v, text)) = cur.as_mut() {
                    let n = r.get("num_data_bytes").unwrap_or("?");
                    *text += &format!(" [{n}]");
                    v["dlc"] = json!(n);
                }
            }
            "data_field" => {
                if let Some((_, v, text)) = cur.as_mut() {
                    let b = r.byte("data").map(hex).unwrap_or("??".into());
                    *text += &format!(" {b}");
                    if let Some(Value::Array(d)) = v.get_mut("data") {
                        d.push(json!(b));
                    }
                }
            }
            "ack_field" => {
                if let Some((_, v, text)) = cur.as_mut() {
                    let ack = r.flag("ack").unwrap_or(false);
                    *text += if ack { " ACK" } else { " NO-ACK" };
                    v["ack"] = json!(ack);
                }
            }
            "can_error" => {
                finish(&mut items, cur.take());
                items.push((
                    format!("{:>10}  ERROR", fmt_seconds(r.t)),
                    json!({ "t": r.t, "error": true }),
                ));
            }
            _ => {}
        }
    }
    finish(&mut items, cur.take());
    (items, vec![])
}

/// Any analyzer: one item per row, `type key=value ...`.
fn generic(rows: &[Row]) -> Items {
    let items = rows
        .iter()
        .map(|r| {
            let cols: Vec<String> = r.cols.iter().map(|(k, v)| format!("{k}={v}")).collect();
            (
                format!("{:>10}  {} {}", fmt_seconds(r.t), r.ty, cols.join(" ")),
                json!({ "t": r.t, "type": r.ty, "cols": r.cols }),
            )
        })
        .collect();
    (items, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    const I2C: &str = r#"name,type,start_time,duration,"ack","address","read","data"
"I2C","start",0.0010000,0.0000001,,,,
"I2C","address",0.0010100,0.0000900,true,0x50,false,
"I2C","data",0.0011000,0.0000900,true,,,0x00
"I2C","data",0.0012000,0.0000900,true,,,0x10
"I2C","start",0.0013000,0.0000001,,,,
"I2C","address",0.0013100,0.0000900,true,0x50,true,
"I2C","data",0.0014000,0.0000900,true,,,0xAB
"I2C","data",0.0015000,0.0000900,false,,,0xCD
"I2C","stop",0.0016000,0.0000001,,,,
"I2C","start",0.0020000,0.0000001,,,,
"I2C","address",0.0020100,0.0000900,false,0x3C,false,
"I2C","stop",0.0021000,0.0000001,,,,
"#;

    #[test]
    fn i2c_transactions() {
        let s = summarize(Kind::I2c, I2C, 10).unwrap();
        let t = s.text();
        assert!(t.contains("W 0x50: 00 10 | R 0x50: AB CD NAK"), "{t}");
        assert!(t.contains("W 0x3C NAK"), "{t}");
        assert!(
            t.contains("addresses: 0x3C (0 ACK, 1 NAK), 0x50 (2)"),
            "{t}"
        );
        assert_eq!(s.json["items_total"], 2);
    }

    const SPI: &str = r#"name,type,start_time,duration,"mosi","miso"
"SPI","enable",0.001,0.0000001,,
"SPI","result",0.0011,0.000001,0x9F,0xFF
"SPI","result",0.0012,0.000001,0x00,0xEF
"SPI","result",0.0013,0.000001,0x00,0x40
"SPI","disable",0.0014,0.0000001,,
"#;

    #[test]
    fn spi_transfers() {
        let s = summarize(
            Kind::Spi {
                mosi: true,
                miso: true,
            },
            SPI,
            10,
        )
        .unwrap();
        assert!(
            s.text().contains("MOSI: 9F 00 00 MISO: FF EF 40"),
            "{}",
            s.text()
        );
    }

    #[test]
    fn serial_text_and_errors() {
        let mut csv = String::from("name,type,start_time,duration,\"data\",\"error\"\n");
        for (i, b) in b"OK\r\nboot v1.2\n".iter().enumerate() {
            csv += &format!(
                "\"Async Serial\",\"data\",{},0.0001,0x{b:02X},\n",
                i as f64 * 0.001
            );
        }
        csv += "\"Async Serial\",\"data\",1.0,0.0001,0x00,\"framing\"\n";
        let s = summarize(Kind::Serial, &csv, 10).unwrap();
        let t = s.text();
        assert!(t.contains("  OK\n"), "{t}");
        assert!(t.contains("boot v1.2"), "{t}");
        assert!(t.contains("errors: framing 1"), "{t}");
    }

    #[test]
    fn serial_binary_as_hex() {
        let mut csv = String::from("name,type,start_time,duration,\"data\",\"error\"\n");
        for i in 0..20u8 {
            csv += &format!("\"Async Serial\",\"data\",{i},0.0001,0x{:02X},\n", i * 7);
        }
        let s = summarize(Kind::Serial, &csv, 10).unwrap();
        assert!(s.text().contains("00 07 0E 15"), "{}", s.text());
    }

    #[test]
    fn can_frames() {
        let csv = r#"name,type,start_time,duration,"identifier","extended","remote_frame","num_data_bytes","data","crc","ack"
"CAN","identifier_field",0.001,0.00002,0x123,false,false,,,,
"CAN","control_field",0.00102,0.00001,,,,2,,,
"CAN","data_field",0.00103,0.00001,,,,,0x01,,
"CAN","data_field",0.00104,0.00001,,,,,0x02,,
"CAN","crc_field",0.00105,0.00001,,,,,,0x1234,
"CAN","ack_field",0.00106,0.000001,,,,,,,true
"#;
        let s = summarize(Kind::Can, csv, 10).unwrap();
        assert!(s.text().contains("0x123 [2] 01 02 ACK"), "{}", s.text());
    }

    #[test]
    fn empty_table_hints() {
        let s = summarize(Kind::I2c, "name,type,start_time,duration\n", 10).unwrap();
        assert!(s.text().contains("nothing decoded"));
    }

    #[test]
    fn limit_truncates() {
        let s = summarize(Kind::I2c, I2C, 1).unwrap();
        assert!(s.text().contains("... 1 more"));
        assert_eq!(s.json["truncated"], true);
    }
}
