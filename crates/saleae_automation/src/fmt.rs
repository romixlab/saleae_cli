//! Human-readable formatting shared by capture descriptions and analyzer summaries.

/// Human form of a duration in seconds: `1.5 s`, `250 ms`, `12.4 us`, `80 ns`.
pub fn seconds(s: f64) -> String {
    let a = s.abs();
    let (v, u) = if a >= 1.0 || a == 0.0 {
        (s, "s")
    } else if a >= 1e-3 {
        (s * 1e3, "ms")
    } else if a >= 1e-6 {
        (s * 1e6, "us")
    } else {
        (s * 1e9, "ns")
    };
    let txt = format!("{v:.3}");
    let txt = txt.trim_end_matches('0').trim_end_matches('.');
    format!("{txt} {u}")
}

/// Human form of a rate: `10 MS/s`, `500 kS/s`.
pub fn rate(r: f64, unit: &str) -> String {
    let (v, p) = if r >= 1e9 {
        (r / 1e9, "G")
    } else if r >= 1e6 {
        (r / 1e6, "M")
    } else if r >= 1e3 {
        (r / 1e3, "k")
    } else {
        (r, "")
    };
    let txt = format!("{v:.3}");
    let txt = txt.trim_end_matches('0').trim_end_matches('.');
    format!("{txt} {p}{unit}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ranges() {
        assert_eq!(ranges(&[0, 1, 2, 3, 6, 8, 9]), "0-3,6,8-9");
        assert_eq!(ranges(&[5]), "5");
        assert_eq!(ranges(&[]), "");
    }

    #[test]
    fn formatting() {
        assert_eq!(seconds(0.25), "250 ms");
        assert_eq!(seconds(1.5), "1.5 s");
        assert_eq!(seconds(12.5e-6), "12.5 us");
        assert_eq!(rate(10e6, "S/s"), "10 MS/s");
        assert_eq!(rate(115200.0, "bps"), "115.2 kbps");
    }
}
