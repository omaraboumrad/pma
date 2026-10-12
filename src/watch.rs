//! `pma watch`: what running agents are doing, read from their logs. The
//! runner observes; no agent reports. A `claude` log is `stream-json`, one
//! record per event; any other worker's log is shown by its last line.

use std::path::{Path, PathBuf};

use crate::store::{Job, Run, RunState};

/// What a run's log says so far.
#[derive(Debug, Default, PartialEq)]
pub struct Progress {
    pub tool_calls: usize,
    /// The last tool used, with its main argument.
    pub last_tool: Option<String>,
    /// The last thing the agent said, or a non-JSON log's last line.
    pub last_text: Option<String>,
}

/// Reads a `stream-json` log, or the tail of any other.
pub fn progress(log: &str) -> Progress {
    let mut p = Progress::default();
    let mut json = false;
    for line in log.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        json = true;
        if v["type"] != "assistant" {
            continue;
        }
        for part in v["message"]["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("tool_use") => {
                    p.tool_calls += 1;
                    let input = &part["input"];
                    let arg = ["command", "file_path", "pattern", "path", "url"]
                        .iter()
                        .find_map(|k| input[*k].as_str());
                    let name = part["name"].as_str().unwrap_or("?");
                    p.last_tool = Some(match arg {
                        Some(a) => format!("{name}: {}", one_line(a, 80)),
                        None => name.to_string(),
                    });
                }
                Some("text") => {
                    if let Some(t) = part["text"].as_str().filter(|t| !t.trim().is_empty()) {
                        p.last_text = Some(one_line(t, 100));
                    }
                }
                _ => {}
            }
        }
    }
    if !json {
        p.last_text = log
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map(|l| one_line(l, 100));
    }
    p
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &flat[..i]),
        None => flat,
    }
}

/// The newest `agent-<n>.log` of a run.
fn latest_log(state: &Path, run: i64) -> Option<PathBuf> {
    let dir = state.join("runs").join(run.to_string());
    (1..)
        .map(|n| dir.join(format!("agent-{n}.log")))
        .take_while(|p| p.exists())
        .last()
}

fn age(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// One screen: the jobs not yet settled, then each running run.
pub fn screen(state: &Path, jobs: &[Job], runs: &[Run], now: i64) -> String {
    let mut out = String::new();
    let open: Vec<&Job> = jobs
        .iter()
        .filter(|j| matches!(j.state.as_str(), "queued" | "running" | "awaiting-approval"))
        .collect();
    for j in &open {
        out.push_str(&format!(
            "job #{} {}: pma {}\n",
            j.id,
            j.state,
            j.args.join(" ")
        ));
    }
    let running: Vec<&Run> = runs
        .iter()
        .filter(|r| r.state == RunState::Running)
        .collect();
    if running.is_empty() {
        out.push_str("no agent is running\n");
    }
    for r in running {
        let since = r.dispatched_at.map_or("?".into(), |t| age(now - t));
        let log = latest_log(state, r.id);
        let quiet = log
            .as_ref()
            .and_then(|l| std::fs::metadata(l).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.elapsed().ok())
            .map_or("?".into(), |d| age(d.as_secs() as i64));
        let p = log
            .and_then(|l| std::fs::read_to_string(l).ok())
            .map(|text| progress(&text))
            .unwrap_or_default();
        out.push_str(&format!(
            "#{} {} {}  {}  running {since}, last output {quiet} ago, {} tool calls\n",
            r.id,
            r.project,
            r.node.as_deref().unwrap_or("dispatch"),
            r.agent,
            p.tool_calls
        ));
        if let Some(t) = p.last_tool {
            out.push_str(&format!("    {t}\n"));
        }
        if let Some(t) = p.last_text {
            out.push_str(&format!("    > {t}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_read_for_tools_and_text() {
        let log = r#"{"type":"system","subtype":"init"}
{"type":"assistant","message":{"content":[{"type":"text","text":"Reading the\n code."},{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"..."}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"make test"}}]}}
"#;
        assert_eq!(
            progress(log),
            Progress {
                tool_calls: 2,
                last_tool: Some("Bash: make test".into()),
                last_text: Some("Reading the code.".into()),
            }
        );
    }

    #[test]
    fn a_plain_log_shows_its_last_line() {
        let p = progress("building\n\ncompiling foo\n\n");
        assert_eq!(p.tool_calls, 0);
        assert_eq!(p.last_text.as_deref(), Some("compiling foo"));
    }

    #[test]
    fn long_text_is_cut_on_a_character() {
        assert_eq!(one_line("ééé", 2), "éé...");
    }
}
