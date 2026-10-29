//! Agents in a herdr session of their own. `pma herdr` opens session `pma`,
//! with `pma tui` in the workspace `pma` and a tab per agent run in the
//! workspace `agents`. While it runs and `herdr` is on, an agent starts in
//! that tab instead of headless; otherwise nothing changes.
//!
//! `pma` still owns the run. A bridge, `pma herdr bridge`, takes the agent's
//! place under `agent::run_limited`, so the timeout, the log and the exit
//! status are what they were. The tab runs `pma herdr pane`, which starts
//! the agent with the environment `pma` built, not the herdr server's, so
//! the push credentials stay stripped. Each side holds a lock the other
//! watches: the bridge killed at the timeout ends the agent, and the tab
//! closed ends the bridge.
//!
//! A tab stays until its run no longer waits on anyone: `sweep` closes it
//! once the run is approved, rejected or published.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::{self, Finished};
use crate::config::Config;
use crate::store::{self, Result, RunState, Store};

/// The herdr session `pma` uses, apart from the user's own.
pub const SESSION: &str = "pma";
/// The workspace holding `pma tui`.
const HOME: &str = "pma";
/// The workspace holding a tab per agent run.
const AGENTS: &str = "agents";
/// How long the bridge waits for the tab to start the agent.
const START: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_millis(200);

/// The run an agent works for, to name its tab.
pub struct Launch<'a> {
    pub run: i64,
    pub project: &'a str,
    pub agent: &'a str,
}

impl Launch<'_> {
    fn label(&self) -> String {
        format!("{} #{}", self.project, self.run)
    }
}

/// `herdr` aimed at the `pma` session, whatever session the caller is in.
fn herdr() -> Command {
    let mut cmd = Command::new("herdr");
    cmd.args(["--session", SESSION]);
    // Set inside a herdr pane, where `pma tui` runs; the explicit session
    // decides, not the pane the caller sits in.
    cmd.env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_TAB_ID")
        .env_remove("HERDR_WORKSPACE_ID");
    cmd
}

/// Runs one herdr API command and returns its `result`.
fn call(args: &[&str]) -> Result<Value> {
    let out = herdr()
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("herdr: {e}"))?;
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    if let Some(message) = v["error"]["message"].as_str() {
        return Err(format!("herdr {}: {message}", args.join(" ")).into());
    }
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("herdr {}: {}", args.join(" "), err.trim()).into());
    }
    Ok(v["result"].clone())
}

/// Whether the `pma` session's server is up. False when herdr is not
/// installed.
pub fn running() -> bool {
    herdr()
        .args(["workspace", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Starts the `pma` session's server in the background, unless it runs.
pub fn start() -> Result<()> {
    use std::os::unix::process::CommandExt;

    if running() {
        return Ok(());
    }
    let state = store::state_home()?;
    std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let path = state.join("herdr.log");
    let log = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    herdr()
        .arg("server")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("herdr: {e}; is it installed?"))?;
    let started = Instant::now();
    while !running() {
        if started.elapsed() > Duration::from_secs(10) {
            return Err(format!("the herdr server did not start; see {}", path.display()).into());
        }
        std::thread::sleep(TICK);
    }
    Ok(())
}

/// Stops the `pma` session's server, and with it every tab.
pub fn stop() -> Result<()> {
    let out = Command::new("herdr")
        .args(["session", "stop", SESSION])
        .output()
        .map_err(|e| format!("herdr: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = String::from_utf8_lossy(&out.stdout);
        return Err(format!("herdr session stop: {}{}", msg.trim(), err.trim()).into());
    }
    Ok(())
}

/// The id of the workspace labelled `label`, if there is one.
fn workspace(label: &str) -> Result<Option<String>> {
    let list = call(&["workspace", "list"])?;
    Ok(list["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|w| w["label"] == label)
        .and_then(|w| w["workspace_id"].as_str())
        .map(String::from))
}

/// The `agents` workspace, created when it is missing.
pub fn agents() -> Result<String> {
    if let Some(id) = workspace(AGENTS)? {
        return Ok(id);
    }
    let made = call(&["workspace", "create", "--label", AGENTS, "--no-focus"])?;
    Ok(made["workspace"]["workspace_id"]
        .as_str()
        .ok_or("herdr returned no workspace")?
        .to_string())
}

/// The `pma` workspace with `pma tui` running in it, and the TUI's tab
/// focused: focusing the workspace alone shows whichever tab was last
/// active, which may be a shell. herdr brings a stopped session's
/// workspaces back with a shell in each pane and not what ran there, so a
/// workspace that exists may still need the TUI.
pub fn home(cwd: &Path) -> Result<()> {
    let id = match workspace(HOME)? {
        Some(id) => id,
        None => {
            let cwd = cwd.to_string_lossy();
            let made = call(&["workspace", "create", "--label", HOME, "--cwd", &cwd])?;
            made["workspace"]["workspace_id"]
                .as_str()
                .ok_or("herdr returned no workspace")?
                .to_string()
        }
    };
    let panes = call(&["pane", "list", "--workspace", &id])?;
    let mut idle = None;
    for entry in panes["panes"].as_array().into_iter().flatten() {
        let (Some(pane), Some(tab)) = (entry["pane_id"].as_str(), entry["tab_id"].as_str()) else {
            continue;
        };
        let info = call(&["pane", "process-info", "--pane", pane])?;
        let info = &info["process_info"];
        let running = info["foreground_processes"]
            .as_array()
            .into_iter()
            .flatten();
        let mut running = running.map(|p| {
            p["argv"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
        });
        if running.any(|argv| is_tui(&argv)) {
            return focus(&id, tab);
        }
        if idle.is_none() && info["foreground_process_group_id"] == info["shell_pid"] {
            idle = Some((pane.to_string(), tab.to_string()));
        }
    }
    let (pane, tab) = match idle {
        Some(idle) => idle,
        // Every pane is busy: the TUI gets a tab of its own.
        None => {
            let cwd = cwd.to_string_lossy();
            let made = call(&["tab", "create", "--workspace", &id, "--cwd", &cwd])?;
            let root = &made["root_pane"];
            match (root["pane_id"].as_str(), root["tab_id"].as_str()) {
                (Some(pane), Some(tab)) => (pane.to_string(), tab.to_string()),
                _ => return Err("herdr returned no pane".into()),
            }
        }
    };
    let exe = std::env::current_exe()?;
    let line = format!("{} tui", quote(&exe.to_string_lossy()));
    call(&["pane", "run", &pane, &line])?;
    focus(&id, &tab)
}

/// Shows `tab` of workspace `id` to the client that attaches next.
fn focus(id: &str, tab: &str) -> Result<()> {
    call(&["workspace", "focus", id])?;
    call(&["tab", "focus", tab])?;
    Ok(())
}

/// Whether a process's arguments are `pma tui`, by any path to `pma`.
fn is_tui(argv: &[&str]) -> bool {
    match argv {
        [program, "tui", ..] => Path::new(program).file_name().is_some_and(|n| n == "pma"),
        _ => false,
    }
}

/// Quoted for the pane's shell.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Runs the agent `cmd` as `agent::run_limited` does: in a herdr tab when
/// `herdr` is on and the session runs, else headless.
pub fn run_limited(
    cfg: &Config,
    cmd: Command,
    log: &Path,
    timeout: Duration,
    launch: &Launch,
) -> std::io::Result<Finished> {
    if !cfg.herdr || !running() {
        return agent::run_limited(cmd, None, log, timeout);
    }
    let dir = log.with_extension("herdr");
    let _ = std::fs::remove_dir_all(&dir);
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    }
    write_spec(&dir, &cmd, log, launch)?;
    let mut bridge = Command::new(std::env::current_exe()?);
    bridge.args(["herdr", "bridge"]).arg(&dir);
    let finished = agent::run_limited(bridge, None, log, timeout);
    // The pane reads it at once; one left here was never read, and it holds
    // the agent's environment.
    let _ = std::fs::remove_dir_all(&dir);
    finished
}

/// What the tab runs: the agent's program, arguments, directory and its
/// whole environment, which the tab sets in place of its own.
fn write_spec(dir: &Path, cmd: &Command, log: &Path, launch: &Launch) -> std::io::Result<()> {
    use std::collections::BTreeMap;
    use std::os::unix::fs::OpenOptionsExt;

    let mut env: BTreeMap<String, String> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into()))
        .collect();
    for (k, v) in cmd.get_envs() {
        let k = k.to_string_lossy().into_owned();
        match v {
            Some(v) => env.insert(k, v.to_string_lossy().into()),
            None => env.remove(&k),
        };
    }
    // An agent started from `pma tui` would inherit the pane's; it is not
    // meant to drive herdr.
    env.retain(|k, _| !k.starts_with("HERDR_"));
    let cwd = cmd
        .get_current_dir()
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir()?);
    let spec = json!({
        "program": cmd.get_program().to_string_lossy(),
        "args": cmd.get_args().map(|a| a.to_string_lossy()).collect::<Vec<_>>(),
        "cwd": cwd,
        "env": env,
        "log": log,
        "run": launch.run,
        "label": launch.label(),
        "agent": launch.agent,
    });
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join("spec.json"))?;
    f.write_all(spec.to_string().as_bytes())
}

fn read_spec(dir: &Path) -> Result<Value> {
    let path = dir.join("spec.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn text<'a>(spec: &'a Value, key: &str) -> Result<&'a str> {
    spec[key]
        .as_str()
        .ok_or_else(|| format!("spec has no {key}").into())
}

/// Whether the process holding `lock` exclusively has ended. A lock that
/// is missing went with the run's directory, which `run_limited` removes
/// once the bridge has ended, at the timeout too.
fn gone(lock: &Path) -> bool {
    match File::open(lock) {
        Ok(f) => f.try_lock_shared().is_ok(),
        Err(_) => true,
    }
}

/// Appends a line to the run's log, after anything the agent wrote.
fn note(log: &Path, msg: &str) {
    if let Ok(mut f) = OpenOptions::new().append(true).open(log) {
        let _ = writeln!(f, "pma: {msg}");
    }
}

/// `pma herdr bridge <dir>`: opens the run's tab, starts the agent in it,
/// and exits with its status. It writes nothing to its own output, which is
/// the run's log, now written by the agent.
pub fn bridge(dir: &Path) -> Result<i32> {
    let spec = read_spec(dir)?;
    let log = PathBuf::from(text(&spec, "log")?);
    let lock = File::create(dir.join("bridge.lock"))?;
    lock.lock()?;
    match open_tab(dir, &spec) {
        Ok(()) => {}
        Err(e) => {
            note(&log, &format!("could not open a herdr tab: {e}"));
            return Ok(1);
        }
    }
    let started = Instant::now();
    loop {
        if let Ok(status) = std::fs::read_to_string(dir.join("status")) {
            return Ok(status.trim().parse().unwrap_or(1));
        }
        let pane_started = dir.join("started").exists();
        if pane_started && gone(&dir.join("pane.lock")) {
            // The status is written before the lock goes: read once more.
            if let Ok(status) = std::fs::read_to_string(dir.join("status")) {
                return Ok(status.trim().parse().unwrap_or(1));
            }
            note(&log, "the herdr tab closed before the agent finished");
            return Ok(1);
        }
        if !pane_started && started.elapsed() > START {
            note(&log, "the herdr tab did not start the agent");
            return Ok(1);
        }
        std::thread::sleep(TICK);
    }
}

fn open_tab(dir: &Path, spec: &Value) -> Result<()> {
    let run = spec["run"].as_i64().ok_or("spec has no run")?;
    let label = text(spec, "label")?;
    let cwd = text(spec, "cwd")?;
    let agents = agents()?;
    // A retry's tab replaces the attempt before it.
    for tab in tabs()?.into_iter().filter(|t| t.run == run) {
        tab.close();
    }
    let made = call(&[
        "tab",
        "create",
        "--workspace",
        &agents,
        "--label",
        label,
        "--cwd",
        cwd,
        "--no-focus",
    ])?;
    let pane = &made["root_pane"];
    let tab = Tab {
        run,
        pane: pane["pane_id"].as_str().unwrap_or_default().into(),
        tab: pane["tab_id"].as_str().unwrap_or_default().into(),
        terminal: pane["terminal_id"].as_str().unwrap_or_default().into(),
    };
    tab.save()?;
    let exe = std::env::current_exe()?;
    let line = format!(
        "{} herdr pane {}",
        quote(&exe.to_string_lossy()),
        quote(&dir.to_string_lossy())
    );
    call(&["pane", "run", &tab.pane, &line])?;
    Ok(())
}

/// `pma herdr pane <dir>`, in the run's tab: runs the agent under the
/// environment the spec holds, shows its log as it grows, and writes its
/// exit status for the bridge.
pub fn pane(dir: &Path) -> Result<()> {
    let lock = File::create(dir.join("pane.lock"))?;
    lock.lock()?;
    let spec = read_spec(dir)?;
    let _ = std::fs::remove_file(dir.join("spec.json"));
    std::fs::write(dir.join("started"), "")?;
    let log = PathBuf::from(text(&spec, "log")?);
    let label = text(&spec, "label")?;
    let agent = text(&spec, "agent")?;
    let here = std::env::var("HERDR_PANE_ID").ok();
    if let Ok(tab) = std::env::var("HERDR_TAB_ID") {
        // herdr names a new tab after its directory once its shell starts.
        let _ = call(&["tab", "rename", &tab, label]);
    }
    let report = |state: &str, msg: &str| {
        if let Some(pane) = &here {
            let _ = call(&[
                "pane",
                "report-agent",
                pane,
                "--source",
                "pma",
                "--agent",
                agent,
                "--state",
                state,
                "--message",
                msg,
            ]);
        }
    };

    let mut cmd = Command::new(text(&spec, "program")?);
    cmd.args(
        spec["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    )
    .current_dir(text(&spec, "cwd")?);
    let env = spec["env"].as_object().ok_or("spec has no env")?;
    // Removed one by one rather than with `env_clear`, which
    // `spawn_guarded` does not carry over when it wraps the command: the
    // herdr server's environment holds the credentials pma took away.
    for (k, _) in std::env::vars_os() {
        if !env.contains_key(k.to_string_lossy().as_ref()) {
            cmd.env_remove(k);
        }
    }
    for (k, v) in env {
        cmd.env(k, v.as_str().unwrap_or_default());
    }
    println!("pma: {label}, {agent}\n");
    report("working", label);
    // Dropping the pipe kills the agent's group, so closing the tab ends it.
    let (mut child, lifeline) = agent::spawn_guarded(cmd, None, &log)?;
    let mut shown = Tail::new(&log);
    let status = loop {
        shown.print();
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if gone(&dir.join("bridge.lock")) {
            break None;
        }
        std::thread::sleep(TICK);
    };
    let group = format!("-{}", child.id());
    let _ = Command::new("kill").args(["-KILL", "--", &group]).status();
    let _ = child.wait();
    drop(lifeline);
    shown.print();
    let code = status.and_then(|s| s.code()).unwrap_or(1);
    // Gone with the directory when the bridge ended first; nobody reads it.
    let tmp = dir.join("status.tmp");
    let _ = std::fs::write(&tmp, code.to_string())
        .and_then(|()| std::fs::rename(&tmp, dir.join("status")));
    let run = spec["run"].as_i64().unwrap_or_default();
    let end = match status {
        None => "stopped by pma".to_string(),
        Some(_) if code == 0 => format!("finished; `pma review {run}`"),
        Some(_) => format!("failed (exit {code}); `pma review {run}`"),
    };
    println!("\npma: {label} {end}. The tab closes once the run is reviewed.");
    report("idle", &end);
    Ok(())
}

/// The part of a log not yet shown, printed line by line as `pma watch`
/// reads it.
struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    fn new(path: &Path) -> Tail {
        Tail {
            path: path.to_path_buf(),
            offset: 0,
            partial: Vec::new(),
        }
    }

    fn print(&mut self) {
        let Ok(mut f) = File::open(&self.path) else {
            return;
        };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return;
        }
        self.offset += buf.len() as u64;
        self.partial.extend(buf);
        let mut out = std::io::stdout().lock();
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            for shown in crate::watch::render(&String::from_utf8_lossy(&line)) {
                let _ = writeln!(out, "{shown}");
            }
        }
        let _ = out.flush();
    }
}

/// A tab opened for a run, as recorded in the state directory: herdr reuses
/// tab and pane ids across server restarts, so the terminal id confirms it
/// is still the same tab before it is closed.
struct Tab {
    run: i64,
    pane: String,
    tab: String,
    terminal: String,
}

fn tabs_dir() -> Result<PathBuf> {
    Ok(store::state_home()?.join("herdr-tabs"))
}

impl Tab {
    fn file(&self) -> Result<PathBuf> {
        let name = format!("{}-{}", self.run, self.terminal);
        Ok(tabs_dir()?.join(name))
    }

    fn save(&self) -> Result<()> {
        let dir = tabs_dir()?;
        std::fs::create_dir_all(&dir)?;
        let line = format!(
            "{} {} {} {}\n",
            self.run, self.pane, self.tab, self.terminal
        );
        std::fs::write(self.file()?, line)?;
        Ok(())
    }

    /// Closes the tab if it is still the one recorded, and forgets it.
    fn close(&self) {
        let same = call(&["pane", "get", &self.pane])
            .is_ok_and(|v| v["pane"]["terminal_id"] == self.terminal.as_str());
        if same {
            let _ = call(&["tab", "close", &self.tab]);
        }
        if let Ok(f) = self.file() {
            let _ = std::fs::remove_file(f);
        }
    }
}

fn tabs() -> Result<Vec<Tab>> {
    let Ok(entries) = std::fs::read_dir(tabs_dir()?) else {
        return Ok(Vec::new());
    };
    let mut tabs = Vec::new();
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let parts: Vec<&str> = text.split_whitespace().collect();
        if let [run, pane, tab, terminal] = parts[..]
            && let Ok(run) = run.parse()
        {
            tabs.push(Tab {
                run,
                pane: pane.into(),
                tab: tab.into(),
                terminal: terminal.into(),
            });
        }
    }
    Ok(tabs)
}

/// Whether a run still waits on someone, so its tab stays open.
fn waits(state: RunState) -> bool {
    matches!(
        state,
        RunState::Queued | RunState::Running | RunState::Ready | RunState::Failed
    )
}

/// `sweep`, opening the store only when a tab is open.
pub fn sweep_default() -> Result<()> {
    if tabs()?.is_empty() {
        return Ok(());
    }
    sweep(&Store::open_default()?)
}

/// Closes the tab of each run that no longer waits: approved, rejected,
/// published or done. Costs nothing when no tab is open.
pub fn sweep(store: &Store) -> Result<()> {
    let open = tabs()?;
    if open.is_empty() {
        return Ok(());
    }
    let runs = store.runs()?;
    let running = running();
    for tab in open {
        let state = runs.iter().find(|r| r.id == tab.run).map(|r| r.state);
        if state.is_some_and(waits) {
            continue;
        }
        match running {
            true => tab.close(),
            // The session ended, and its tabs with it.
            false => {
                if let Ok(f) = tab.file() {
                    let _ = std::fs::remove_file(f);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_path_survives_the_shell() {
        let path = "/tmp/it's here";
        let out = Command::new("sh")
            .args(["-c", &format!("printf %s {}", quote(path))])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), path);
    }

    #[test]
    fn the_tui_is_known_by_its_arguments() {
        assert!(is_tui(&["/Users/u/.local/bin/pma", "tui"]));
        assert!(is_tui(&["pma", "tui"]));
        assert!(!is_tui(&["pma", "watch"]));
        assert!(!is_tui(&["vim", "tui"]));
        assert!(!is_tui(&["-zsh"]));
    }

    #[test]
    fn a_run_waits_until_it_is_reviewed() {
        assert!(waits(RunState::Running));
        assert!(waits(RunState::Ready));
        assert!(waits(RunState::Failed));
        assert!(!waits(RunState::Approved));
        assert!(!waits(RunState::Rejected));
        assert!(!waits(RunState::Done));
    }
}
