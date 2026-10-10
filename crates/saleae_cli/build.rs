//! Sets `GIT_SHA` (short, `-dirty` with local changes) and `BUILD_TIME` (UTC) for `saleae --version` (both
//! `unknown` without git). The gRPC proto itself is fetched and compiled by `saleae`'s own `build.rs`; on
//! `DOCS_RS` that one builds `saleae` as a stub with no modules, so this crate also builds a stub binary (same
//! `saleae_cli_stub` cfg trick, decided independently here since build-script cfgs don't cross crates).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rustc-check-cfg=cfg(saleae_cli_stub)");
    if std::env::var_os("DOCS_RS").is_some() {
        println!("cargo:rustc-cfg=saleae_cli_stub");
        return;
    }
    // .git is a file in a worktree: ask git where HEAD and the index live
    for file in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", file]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    println!("cargo:rustc-env=GIT_SHA={}", git_sha());
    println!("cargo:rustc-env=BUILD_TIME={}", build_time());
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_sha() -> String {
    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return "unknown".into();
    };
    match git(&["status", "--porcelain"]) {
        Some(s) if !s.is_empty() => format!("{sha}-dirty"),
        _ => sha,
    }
}

/// "3 Oct 2026 18:20 UTC", without a date crate
fn build_time() -> String {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return "unknown".into();
    };
    let secs = now.as_secs() as i64;
    let (days, rest) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{day} {} {year} {:02}:{:02} UTC",
        MONTHS[(month - 1) as usize],
        rest / 3600,
        rest % 3600 / 60
    )
}
