//! Shell completions, produced by the `saleae` binary itself when the shell calls it with `COMPLETE=<shell>` set
//! (see [CompleteEnv]): `source <(COMPLETE=bash saleae)`, `COMPLETE=fish saleae | source`, ...
//!
//! Device ids come from the running server (simulated devices included), capture and analyzer ids from the CLI
//! state, analyzer names from the bundled list. The Bash adapter ([Bash]) is the one from wire_weaver_cli, so values
//! with spaces ("Async Serial") complete as one word.

use clap::Command;
use clap_complete::CompleteEnv;
use clap_complete::CompletionCandidate;
use clap_complete::env::{Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use saleae_automation::server::{self, State};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Answers a shell completion request and exits if there is one, otherwise does nothing.
pub(crate) fn complete(factory: fn() -> Command) {
    CompleteEnv::with_factory(factory)
        .shells(Shells(&[&Bash, &Elvish, &Fish, &Powershell, &Zsh]))
        .complete();
}

/// Devices of an already running server (never starts one), else the simulated ones it always has.
pub(crate) fn device_ids() -> Vec<CompletionCandidate> {
    let addr = std::env::var("SALEAE_ADDR").unwrap_or_else(|_| server::DEFAULT_ADDR.into());
    let listed = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()
        .and_then(|rt| {
            rt.block_on(saleae_automation::device::probe_quick(
                &addr,
                Duration::from_millis(500),
            ))
        });
    let devices = listed.unwrap_or_else(|| {
        vec![
            ("F4241".into(), "Logic Pro 16 (simulated)".into()),
            ("F4244".into(), "Logic Pro 8 (simulated)".into()),
            ("F4243".into(), "Logic 8 (simulated)".into()),
        ]
    });
    devices
        .into_iter()
        .map(|(id, help)| CompletionCandidate::new(id).help(Some(help.into())))
        .collect()
}

/// Captures this CLI created in the running server.
pub(crate) fn capture_ids() -> Vec<CompletionCandidate> {
    State::load_unchecked()
        .captures
        .into_iter()
        .map(|c| {
            CompletionCandidate::new(c.id.to_string())
                .help(Some(format!("{} {}", c.device, c.desc).into()))
        })
        .collect()
}

pub(crate) fn analyzer_ids() -> Vec<CompletionCandidate> {
    State::load_unchecked()
        .captures
        .into_iter()
        .flat_map(|c| {
            c.analyzers.into_iter().map(move |a| {
                CompletionCandidate::new(a.id.to_string())
                    .help(Some(format!("{} on capture {}", a.label, c.id).into()))
            })
        })
        .collect()
}

pub(crate) fn analyzer_names() -> Vec<CompletionCandidate> {
    saleae_automation::analyzer::BUNDLED
        .iter()
        .map(|n| CompletionCandidate::new(*n))
        .collect()
}

/// Bash adapter that quotes completions.
///
/// Bash inserts completions verbatim and splits the word being completed on `COMP_WORDBREAKS` (quotes, `@`, `:`,
/// `=`, ...), while clap_complete's adapter passes neither quoting nor these splits on, so values with spaces
/// broke into several words and values like `--api name@^0.1` could not be completed. Here the raw command line is
/// split by `saleae` instead, and each candidate is turned into the text bash should put in place of its current word:
/// only the part after the last word break, escaped for an already open quote or single-quoted when needed.
struct Bash;

impl EnvCompleter for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn is(&self, name: &str) -> bool {
        name == "bash"
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> Result<(), std::io::Error> {
        let script = r#"
_saleae_complete_NAME() {
    local IFS=$'\013'
    if compopt +o nospace 2> /dev/null; then
        local space=false
    else
        local space=true
    fi
    COMPREPLY=( $( \
        _SALEAE_COMP_LINE="${COMP_LINE:0:COMP_POINT}" \
        _SALEAE_COMP_WORDBREAKS="$COMP_WORDBREAKS" \
        _CLAP_IFS="$IFS" \
        VAR="bash" \
        'COMPLETER' -- "$1" \
    ) )
    if [[ $? != 0 ]]; then
        unset COMPREPLY
    elif [[ $space == false ]] && [[ "${COMPREPLY-}" =~ [=/:]\'?$ ]]; then
        compopt -o nospace
    fi
}
if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -o nospace -o bashdefault -o nosort -F _saleae_complete_NAME BIN
else
    complete -o nospace -o bashdefault -F _saleae_complete_NAME BIN
fi
"#
        .replace("NAME", &name.replace('-', "_"))
        .replace("BIN", bin)
        .replace("COMPLETER", &completer.replace('\'', r"'\''"))
        .replace("VAR", var);
        writeln!(buf, "{script}")
    }

    fn write_complete(
        &self,
        cmd: &mut Command,
        _args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> Result<(), std::io::Error> {
        let line = std::env::var("_SALEAE_COMP_LINE").unwrap_or_default();
        let wordbreaks = std::env::var("_SALEAE_COMP_WORDBREAKS").unwrap_or_default();
        let ifs = std::env::var("_CLAP_IFS").unwrap_or_else(|_| "\n".into());
        let (words, current) = split_line(&line, &wordbreaks);
        let index = words.len() - 1;
        let args = words.into_iter().map(OsString::from).collect();
        let completions = clap_complete::engine::complete(cmd, args, index, current_dir)?;
        let replies: Vec<_> = completions
            .iter()
            .filter_map(|c| current.reply(&c.get_value().to_string_lossy()))
            .collect();
        write!(buf, "{}", replies.join(&ifs))
    }
}

/// The word being completed, as seen by bash.
#[derive(Debug, PartialEq)]
struct CurrentWord {
    /// The word without quotes and escapes.
    value: String,
    /// Quote left open, if any.
    open_quote: Option<char>,
    /// Length of the start of `value` that bash keeps: before the open quote or the last word break.
    kept: usize,
}

/// Splits a command line (up to the cursor) into shell words, the last one being the word being completed.
fn split_line(line: &str, wordbreaks: &str) -> (Vec<String>, CurrentWord) {
    let mut words = vec![];
    let mut word = String::new();
    let mut in_word = false;
    let mut quote = None;
    let mut kept = 0;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                // bash treats the closing quote as a word break
                kept = word.len();
            }
            (Some('"'), '\\') => match chars.next() {
                Some(e @ ('"' | '\\' | '$' | '`')) => word.push(e),
                Some(e) => {
                    word.push('\\');
                    word.push(e);
                }
                None => word.push('\\'),
            },
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
                kept = word.len();
            }
            (None, '\\') => {
                in_word = true;
                if let Some(e) = chars.next() {
                    word.push(e);
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
                kept = 0;
            }
            (None, c) => {
                in_word = true;
                if wordbreaks.contains(c) {
                    // bash keeps `@` and `$` (its rl_special_prefixes) as the start of the next word
                    kept = word.len()
                        + if matches!(c, '@' | '$') {
                            0
                        } else {
                            c.len_utf8()
                        };
                }
                word.push(c);
            }
        }
    }
    words.push(word.clone());
    let current = CurrentWord {
        value: word,
        open_quote: quote,
        kept,
    };
    (words, current)
}

impl CurrentWord {
    /// Text to put in place of bash's current word to complete `candidate`, `None` if it doesn't continue the word.
    fn reply(&self, candidate: &str) -> Option<String> {
        let rest = candidate.strip_prefix(&self.value[..self.kept])?;
        Some(match self.open_quote {
            Some('"') => rest
                .chars()
                .flat_map(|c| match c {
                    '"' | '\\' | '$' | '`' => vec!['\\', c],
                    c => vec![c],
                })
                .collect(),
            Some(_) => rest.replace('\'', r"'\''"),
            None => quote(rest),
        })
    }
}

/// Single-quotes a value containing characters the shell would interpret (spaces, quotes, `$`, ...).
fn quote(v: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_.,:/@%+=^~".contains(c);
    if v.chars().all(plain) {
        v.to_string()
    } else {
        format!("'{}'", v.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDBREAKS: &str = "\"'><=;|&(:@";

    fn reply(line: &str, candidate: &str) -> Option<String> {
        split_line(line, WORDBREAKS).1.reply(candidate)
    }

    #[test]
    fn splits_words() {
        let (words, current) = split_line(r#"saleae  --label 'a b' "c\"d" e\ f g"#, WORDBREAKS);
        assert_eq!(words, ["saleae", "--label", "a b", "c\"d", "e f", "g"]);
        assert_eq!(current.value, "g");
        let (words, current) = split_line("saleae --label ", WORDBREAKS);
        assert_eq!(words, ["saleae", "--label", ""]);
        assert_eq!(current.value, "");
    }

    #[test]
    fn quotes_values_needing_it() {
        assert_eq!(
            reply("saleae --label N", "Nucleo on the desk").unwrap(),
            "'Nucleo on the desk'"
        );
        assert_eq!(reply("saleae --label ", "it's").unwrap(), r"'it'\''s'");
        assert_eq!(reply("saleae --serial 21", "2100AB").unwrap(), "2100AB");
    }

    #[test]
    fn escapes_for_open_quote() {
        assert_eq!(
            reply("saleae --label 'Nu", "Nucleo on the desk").unwrap(),
            "Nucleo on the desk"
        );
        assert_eq!(
            reply(r#"saleae --label "Nu"#, r#"Nu "x" $y"#).unwrap(),
            r#"Nu \"x\" \$y"#
        );
        assert_eq!(
            reply("saleae --label 'a b' --label 'it", "it's").unwrap(),
            r"it'\''s"
        );
    }

    #[test]
    fn replaces_after_word_break() {
        assert_eq!(
            reply("saleae --api blinky_api@", "blinky_api@^0.1.0").unwrap(),
            "@^0.1.0"
        );
        assert_eq!(
            reply("saleae --vid-pid c0de:", "c0de:cafe").unwrap(),
            "cafe"
        );
        assert_eq!(
            reply("saleae --label=Nu", "--label=Nucleo on the desk").unwrap(),
            "'Nucleo on the desk'"
        );
        assert_eq!(reply("saleae --api blinky_api@", "other"), None);
    }
}
