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

/// One log line as a person reads it, for a run's herdr tab: a
/// `stream-json` record becomes what the agent said and the tools it called,
/// and a line that is not JSON is shown as it is.
pub fn render(line: &str) -> Vec<String> {
    let line = line.trim_end();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
        return vec![line.to_string()];
    };
    match v["type"].as_str() {
        Some("assistant") => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| match part["type"].as_str() {
                Some("text") => part["text"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| format!("{}\n", t.trim())),
                Some("tool_use") => {
                    let input = &part["input"];
                    let arg = ["command", "file_path", "pattern", "path", "url"]
                        .iter()
                        .find_map(|k| input[*k].as_str());
                    let name = part["name"].as_str().unwrap_or("?");
                    Some(match arg {
                        Some(a) => format!("> {name}: {}", one_line(a, 160)),
                        None => format!("> {name}"),
                    })
                }
                _ => None,
            })
            .collect(),
        Some("result") => {
            let cost = v["total_cost_usd"]
                .as_f64()
                .map_or(String::new(), |c| format!(", ${c:.2}"));
            let how = v["subtype"].as_str().unwrap_or("done");
            vec![format!("-- {how}{cost}")]
        }
        Some("system") if v["subtype"] == "init" => {
            let model = v["model"].as_str().unwrap_or("?");
            vec![format!("-- started, {model}")]
        }
        // Tool results and the rest: the next call says what came of them.
        _ => Vec::new(),
    }
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

pub fn age(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

/// A running agent: its run, how long it has run, how long its log has been
/// quiet, and what the log says.
pub struct Agent<'a> {
    pub run: &'a Run,
    pub running: String,
    pub quiet: String,
    pub progress: Progress,
}

/// The jobs not yet settled.
pub fn open_jobs(jobs: &[Job]) -> Vec<&Job> {
    jobs.iter()
        .filter(|j| matches!(j.state.as_str(), "queued" | "running" | "awaiting-approval"))
        .collect()
}

/// Each running run, with its progress read from its newest log.
pub fn agents<'a>(state: &Path, runs: &'a [Run], now: i64) -> Vec<Agent<'a>> {
    runs.iter()
        .filter(|r| r.state == RunState::Running)
        .map(|r| {
            let log = latest_log(state, r.id);
            let quiet = log
                .as_ref()
                .and_then(|l| std::fs::metadata(l).ok())
                .and_then(|m| m.modified().ok())
                .and_then(|m| m.elapsed().ok())
                .map_or("?".into(), |d| age(d.as_secs() as i64));
            Agent {
                run: r,
                running: r.dispatched_at.map_or("?".into(), |t| age(now - t)),
                quiet,
                progress: log
                    .and_then(|l| std::fs::read_to_string(l).ok())
                    .map(|text| progress(&text))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// One screen: the jobs not yet settled, then each running run.
pub fn screen(state: &Path, jobs: &[Job], runs: &[Run], now: i64) -> String {
    let mut out = String::new();
    let open = open_jobs(jobs);
    for j in &open {
        out.push_str(&format!(
            "job #{} {}: pma {}\n",
            j.id,
            j.state,
            j.args.join(" ")
        ));
    }
    let running = agents(state, runs, now);
    if running.is_empty() {
        out.push_str("no agent is running\n");
    }
    for a in running {
        let (r, since, quiet, p) = (a.run, a.running, a.quiet, a.progress);
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

    /// A herdr tab shows what the agent said and did, not the JSON.
    #[test]
    fn a_stream_json_log_renders_for_a_person() {
        let said = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Fixing it."},{"type":"tool_use","name":"Bash","input":{"command":"make\ntest"}}]}}"#;
        assert_eq!(render(said), ["Fixing it.\n", "> Bash: make test"]);
        let done = r#"{"type":"result","subtype":"success","total_cost_usd":0.123}"#;
        assert_eq!(render(done), ["-- success, $0.12"]);
        assert!(render(r#"{"type":"user","message":{}}"#).is_empty());
        assert_eq!(render("plain text\n"), ["plain text"]);
    }

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
