//! Runs agents in Herdr.app: `pma integrate herdr-app`. One space per project,
//! one tab per run. The app arranges spaces and tabs (`herdr-app do`);
//! herdr-core (`hc`) runs the terminals. Both are driven through their CLIs,
//! which find their own sockets. Design: docs/dev/herdr-integration.md.
//!
//! A pane is stored as `hc:<address>`, so a run started under one integration
//! is still closed and focused by the right one after the setting changes.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::Value;

const PREFIX: &str = "hc:";
const READ_TICK: Duration = Duration::from_secs(1);
/// As for herdr: this many failed reconnects, two seconds apart, mean the core
/// has gone, and its terminals with it.
const SERVER_GONE: u32 = 15;

/// Whether `pane` names an herdr-core terminal rather than a herdr pane.
pub fn is_app(pane: &str) -> bool {
    pane.starts_with(PREFIX)
}

fn address(pane: &str) -> &str {
    pane.strip_prefix(PREFIX).unwrap_or(pane)
}

/// `{"error": "..."}` from `herdr-app`, `{"error": {"message": ...}}` from `hc`.
fn error_of(v: &Value) -> Option<String> {
    let e = &v["error"];
    e.as_str()
        .or_else(|| e["message"].as_str())
        .map(String::from)
}

fn json_of(program: &str, args: &[&str]) -> Result<Value, String> {
    let what = format!("{program} {}", args.first().unwrap_or(&""));
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    if let Some(message) = error_of(&v) {
        return Err(message);
    }
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{what}: {}", err.trim()));
    }
    Ok(v)
}

fn hc(args: &[&str]) -> Result<Value, String> {
    json_of("hc", args)
}

/// `herdr-app ls` and `status` talk to the open window and never open one.
fn window(args: &[&str]) -> Result<Value, String> {
    json_of("herdr-app", args)
}

fn act(command: &str, args: &[&str]) -> Result<Value, String> {
    let mut all = vec!["do", command];
    all.extend_from_slice(args);
    window(&all)
}

/// What answers: the window, or only the core, which runs terminals unseen.
#[derive(Debug, PartialEq)]
pub enum Reach {
    Window,
    CoreOnly,
}

/// Whether the window is open; this never opens it or starts a core.
pub fn window_open() -> bool {
    window(&["status"]).is_ok()
}

pub fn check() -> Result<Reach, String> {
    if window(&["status"]).is_ok() {
        return Ok(Reach::Window);
    }
    hc(&["server", "status"])
        .map(|_| Reach::CoreOnly)
        .map_err(|e| format!("no herdr-core: {e}"))
}

/// Where the open window shows `address`: its pane, if it has one.
fn pane_of(listed: &Value, address: &str) -> Option<String> {
    listed["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["tabs"].as_array().into_iter().flatten())
        .flat_map(|t| t["terminals"].as_array().into_iter().flatten())
        .find(|t| t["terminal"] == address)
        .and_then(|t| t["pane"].as_str().map(String::from))
}

fn space_named(listed: &Value, name: &str) -> Option<String> {
    listed["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"] == name)
        .and_then(|s| s["id"].as_str().map(String::from))
}

/// How a new terminal is shown.
#[derive(Clone, Copy, PartialEq)]
pub enum Show {
    /// In front: a person asked for it.
    Front,
    /// In a tab, leaving the person's view as it was: the app shows a new tab.
    Back,
    /// As `Back`, or with the window closed, unseen in the core.
    Anywhere,
}

/// Starts `argv` (a shell when empty) in a new tab of `project`'s space, made
/// if need be, and returns its pane.
pub fn start(
    project: &str,
    title: &str,
    cwd: &Path,
    argv: &[String],
    show: Show,
) -> Result<String, String> {
    let title: String = title.chars().take(40).collect();
    let cwd = cwd.to_string_lossy();
    let shown = match window(&["status"]) {
        Ok(shown) => shown,
        Err(_) if show == Show::Anywhere && !argv.is_empty() => {
            // No parent: from inside a terminal, `hc` would type notes about
            // the run into the one that started `pma`.
            let label = format!("pma={title}");
            let mut args = vec![
                "run",
                "--bg",
                "--no-parent",
                "--no-notify",
                "--cwd",
                &cwd,
                "--label",
                &label,
                "--",
            ];
            args.extend(argv.iter().map(String::as_str));
            let ran = hc(&args)?;
            return ran["address"]
                .as_str()
                .map(|a| format!("{PREFIX}{a}"))
                .ok_or_else(|| "hc ran it and named no terminal".to_string());
        }
        Err(e) => return Err(format!("Herdr.app's window is not open: {e}")),
    };
    let argv_json = serde_json::to_string(argv).map_err(|e| e.to_string())?;
    let space_name = format!("pma: {project}");
    let mut args: Vec<&str> = vec!["--name"];
    let made = match space_named(&window(&["ls"])?, &space_name) {
        Some(space) => {
            args.extend([title.as_str(), "--space", &space, "--folder", &cwd]);
            if !argv.is_empty() {
                args.extend(["--argv", &argv_json]);
            }
            act("tab.new", &args)?
        }
        None => {
            args.extend([space_name.as_str(), "--tab", &title, "--folder", &cwd]);
            if !argv.is_empty() {
                args.extend(["--argv", &argv_json]);
            }
            act("space.new", &args)?
        }
    };
    if show != Show::Front
        && let Some(tab) = shown["tab"].as_str()
    {
        let _ = act("tab.select", &["--id", tab]);
    }
    made["terminal"]
        .as_str()
        .map(|t| format!("{PREFIX}{t}"))
        .ok_or_else(|| "Herdr.app made a tab and named no terminal".to_string())
}

/// Closes a pane and ends its terminal. It may be gone already.
pub fn close(pane: &str) {
    let address = address(pane);
    let shown = window(&["ls"])
        .ok()
        .and_then(|listed| pane_of(&listed, address));
    let closed = shown.is_some_and(|p| act("pane.close", &["--pane", &p]).is_ok());
    if !closed {
        let _ = hc(&["kill", address]);
    }
}

/// Brings a terminal to the front, giving it a tab if it ran unseen.
pub fn focus(pane: &str) -> Result<(), String> {
    let address = address(pane);
    if pane_of(&window(&["ls"])?, address).is_none() {
        act("terminal.keep", &["--terminal", address, "--place", "tab"])?;
    }
    act("pane.focus", &["--terminal", address]).map(|_| ())
}

/// What one line of `hc events` says about a terminal.
#[derive(Debug, PartialEq)]
enum Heard {
    Closed,
    Status(String),
    Nothing,
}

/// The first line is a snapshot of matching terminals; after it, `event`
/// notifications. A subscriber that fell behind gets `missed`, then a fresh
/// snapshot, which this reads as it reads the first.
fn hear(line: &str, address: &str) -> Heard {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
        return Heard::Nothing;
    };
    let snapshot = v["terminals"]
        .as_array()
        .or_else(|| v["params"]["terminals"].as_array());
    if let Some(terminals) = snapshot {
        return match terminals.iter().find(|t| t["address"] == address) {
            None => Heard::Closed,
            Some(t) if t["state"] != "live" => Heard::Closed,
            Some(t) => t["status"]
                .as_str()
                .map_or(Heard::Nothing, |s| Heard::Status(s.to_string())),
        };
    }
    let p = &v["params"];
    if p["terminal"] != address {
        return Heard::Nothing;
    }
    match p["type"].as_str() {
        Some("finished" | "removed") => Heard::Closed,
        Some("status") => {
            let d = &p["data"];
            d["status"]
                .as_str()
                .or_else(|| d["state"].as_str())
                .map_or(Heard::Nothing, |s| Heard::Status(s.to_string()))
        }
        _ => Heard::Nothing,
    }
}

struct Events(Child);

impl Drop for Events {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Follows `pane` until `stop`, setting `closed` once its terminal has ended
/// and passing each change of state to `on_state`.
pub fn follow(pane: &str, closed: &AtomicBool, stop: &AtomicBool, on_state: &dyn Fn(&str)) {
    let address = address(pane);
    let mut failures = 0;
    let mut last = String::new();
    while !stop.load(Ordering::Relaxed) {
        match subscribe_once(address, closed, stop, on_state, &mut last) {
            Ok(()) => failures = 0,
            Err(_) => failures += 1,
        }
        if closed.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
            return;
        }
        let gone = match hc(&["get", address]) {
            Ok(t) => t["state"] != "live",
            Err(e) => e.contains("no terminal") || failures >= SERVER_GONE,
        };
        if gone {
            closed.store(true, Ordering::Relaxed);
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn subscribe_once(
    address: &str,
    closed: &AtomicBool,
    stop: &AtomicBool,
    on_state: &dyn Fn(&str),
    last: &mut String,
) -> Result<(), String> {
    let mut child = Command::new("hc")
        .args(["events", "--terminal", address])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("hc events: {e}"))?;
    let stdout = child.stdout.take().ok_or("hc events: no output")?;
    let _events = Events(child);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.map(|l| tx.send(l)).is_err() {
                return;
            }
        }
    });
    while !stop.load(Ordering::Relaxed) {
        let line = match rx.recv_timeout(READ_TICK) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Err("hc events ended".into()),
        };
        match hear(&line, address) {
            Heard::Closed => {
                closed.store(true, Ordering::Relaxed);
                return Ok(());
            }
            Heard::Status(state) if state != *last => {
                on_state(&state);
                *last = state;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As captured from herdr-core 0.1.20.
    #[test]
    fn events_are_read_as_hc_sends_them() {
        let finished = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"exit_code":1,"finished_ms":1,"reason":"killed","signal":null},"seq":816,"terminal":"t1f","type":"finished"}}"#;
        assert_eq!(hear(finished, "t1f"), Heard::Closed);
        assert_eq!(hear(finished, "t1d"), Heard::Nothing, "another terminal");
        let labels = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"labels":{}},"seq":812,"terminal":"t1f","type":"labels"}}"#;
        assert_eq!(hear(labels, "t1f"), Heard::Nothing);
        let status = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"status":"blocked"},"seq":9,"terminal":"t1f","type":"status"}}"#;
        assert_eq!(hear(status, "t1f"), Heard::Status("blocked".into()));
        let record = r#"{"jsonrpc":"2.0","method":"event","params":{"data":{"state":"working"},"seq":9,"terminal":"t1f","type":"status"}}"#;
        assert_eq!(hear(record, "t1f"), Heard::Status("working".into()));
    }

    /// A snapshot opens each subscription, and a fresh one follows `missed`.
    #[test]
    fn a_snapshot_says_whether_the_terminal_still_runs() {
        let live = r#"{"channel":null,"seq":811,"terminals":[{"address":"t1f","state":"live","status":"working"}]}"#;
        assert_eq!(hear(live, "t1f"), Heard::Status("working".into()));
        let ended =
            r#"{"seq":811,"terminals":[{"address":"t1f","state":"finished","status":null}]}"#;
        assert_eq!(hear(ended, "t1f"), Heard::Closed);
        let absent = r#"{"seq":811,"terminals":[]}"#;
        assert_eq!(hear(absent, "t1f"), Heard::Closed);
    }

    #[test]
    fn a_pane_names_its_backend() {
        assert!(is_app("hc:t1c"));
        assert!(!is_app("w1:p2"));
        assert_eq!(address("hc:rohan/t7"), "rohan/t7");
    }

    #[test]
    fn the_window_lists_where_a_terminal_is() {
        let listed: Value = serde_json::from_str(
            r#"{"spaces":[{"id":"sp1","name":"pma: alpha","tabs":[{"id":"tb1","terminals":[
                {"terminal":"t6","pane":"p3"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(pane_of(&listed, "t6").as_deref(), Some("p3"));
        assert_eq!(pane_of(&listed, "t7"), None);
        assert_eq!(space_named(&listed, "pma: alpha").as_deref(), Some("sp1"));
        assert_eq!(space_named(&listed, "pma: beta"), None);
    }

    #[test]
    fn both_error_shapes_are_read() {
        let app: Value = serde_json::from_str(r#"{"error":"no such pane"}"#).unwrap();
        assert_eq!(error_of(&app).as_deref(), Some("no such pane"));
        let core: Value =
            serde_json::from_str(r#"{"error":{"kind":"not_found","message":"no terminal \"x\""}}"#)
                .unwrap();
        assert_eq!(error_of(&core).as_deref(), Some("no terminal \"x\""));
        assert_eq!(error_of(&serde_json::json!({"ok": true})), None);
    }

    /// Against the open window: `cargo test -p pma-core -- --ignored herdr_app`.
    #[test]
    #[ignore]
    fn herdr_app_live_round_trip() {
        let before = window(&["status"]).expect("open the window")["tab"].clone();
        let argv = ["sh".into(), "-c".into(), "exit 0".into()];
        let pane = start("live-probe", "probe", Path::new("/tmp"), &argv, Show::Back).unwrap();
        assert!(is_app(&pane), "{pane}");
        assert_eq!(window(&["status"]).unwrap()["tab"], before, "view kept");
        focus(&pane).unwrap();
        assert_ne!(
            window(&["status"]).unwrap()["tab"],
            before,
            "focus shows it"
        );
        let _ = act("tab.select", &["--id", before.as_str().unwrap()]);
        let (closed, stop) = (AtomicBool::new(false), AtomicBool::new(false));
        std::thread::scope(|s| {
            s.spawn(|| follow(&pane, &closed, &stop, &|_| {}));
            std::thread::sleep(Duration::from_secs(3));
            assert!(
                !closed.load(Ordering::Relaxed),
                "a shell stays after the program"
            );
            close(&pane);
            for _ in 0..20 {
                if closed.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            stop.store(true, Ordering::Relaxed);
        });
        assert!(closed.load(Ordering::Relaxed), "closing was heard");
        let listed = window(&["ls"]).unwrap();
        if let Some(space) = space_named(&listed, "pma: live-probe") {
            let _ = act("space.close", &["--id", &space, "--confirmed", "true"]);
        }
        let _ = hc(&["rm", address(&pane)]);
    }
}
