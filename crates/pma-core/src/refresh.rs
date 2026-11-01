//! Keeps the store current after the first `pma scan`. Local state is re-read
//! when a project's files change; CI, which needs the network, on a timer while
//! a front end is open. Design: docs/dev/refresh.md.

use std::path::PathBuf;
use std::time::Duration;

use crate::api;
use crate::dates;
use crate::dispatch;
use crate::scan::{self, Facts};
use crate::store::{Result, RunState, Store};

/// How often a front end looks for changed files. 112 projects take 61 ms.
const TICK: Duration = Duration::from_secs(5);

/// Reads `selected`, leaving unread a clone whose git would run a setting a
/// failed run added.
pub fn read(
    store: &Store,
    selected: Vec<(String, PathBuf)>,
    offline: bool,
    deps: bool,
    finished: &(dyn Fn(&str) + Sync),
) -> Result<Vec<Facts>> {
    let cfg = api::config(store)?;
    let runs = store.runs()?;
    let mut held = Vec::new();
    let mut free = Vec::new();
    for (name, path) in selected {
        match dispatch::clone_held(&runs, &path) {
            Ok(()) => free.push((name, path)),
            Err(e) => held.push(Facts::held(&name, &path, e.to_string())),
        }
    }
    let mut facts = scan::scan_all(&free, &cfg.activity_ignore, offline, deps, finished);
    facts.extend(held);
    Ok(facts)
}

/// An offline read that keeps each project's last CI and run links, which it
/// does not ask for and would otherwise record as unknown.
fn offline(store: &Store, projects: &[(String, PathBuf)]) -> Result<Vec<Facts>> {
    let cfg = api::config(store)?;
    let runs = store.runs()?;
    let free: Vec<(String, PathBuf)> = projects
        .iter()
        .filter(|(_, path)| dispatch::clone_held(&runs, path).is_ok())
        .cloned()
        .collect();
    let mut facts = scan::scan_all(&free, &cfg.activity_ignore, true, false, &|_| {});
    for f in &mut facts {
        if let Some(row) = store.project(&f.name)? {
            (f.ci, f.ci_runs) = (row.ci, row.ci_runs);
        }
    }
    Ok(facts)
}

/// Rereads `projects`, as after a publish ticks an item.
pub fn rescan(store: &mut Store, projects: &[(String, PathBuf)]) -> Result<()> {
    if projects.is_empty() {
        return Ok(());
    }
    let facts = offline(store, projects)?;
    store.save_scan(&facts, false, dates::now())?;
    Ok(())
}

/// Rereads each project under a root whose files changed, and any new one.
/// Nothing before the first scan, which a person starts. A clone an agent is
/// working in waits until the run ends.
pub fn changed(store: &mut Store) -> Result<usize> {
    if store.last_scan()?.is_none() {
        return Ok(0);
    }
    let (found, _) = scan::discover(&store.roots()?);
    let stamps = store.project_stamps()?;
    let runs = store.runs()?;
    let due: Vec<(String, PathBuf)> = found
        .into_iter()
        .filter(|(name, path)| {
            stamps.get(name) != Some(&scan::stamp(path))
                && !runs
                    .iter()
                    .any(|r| r.state == RunState::Running && r.worktree == *path)
        })
        .collect();
    if due.is_empty() {
        return Ok(0);
    }
    let facts = offline(store, &due)?;
    store.save_quiet_scan(&facts, false, dates::now())?;
    Ok(facts.len())
}

/// Whether CI was last read over `scan.every` minutes ago. Never before the
/// first scan, and never with `scan.every` at 0.
fn ci_due(store: &Store) -> Result<bool> {
    let every = api::config(store)?.scan_every;
    Ok(every > 0
        && store
            .last_full_scan()?
            .is_some_and(|at| dates::now() - at >= every * 60))
}

/// A full scan with CI, as `pma scan` does, that leaves no row in `pma history`
/// unless it found a change.
fn everything(store: &mut Store) -> Result<()> {
    let (found, _) = scan::discover(&store.roots()?);
    let facts = read(store, found, false, false, &|_| {})?;
    store.save_quiet_scan(&facts, true, dates::now())?;
    Ok(())
}

/// Keeps the store current while a front end is open. Failures are dropped: a
/// terminal UI has nowhere to print them, and `c` scans by hand.
pub fn start(open: impl Fn() -> Result<Store> + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(mut store) = open() else {
            return;
        };
        loop {
            std::thread::sleep(TICK);
            let _ = changed(&mut store);
            if ci_due(&store).unwrap_or(false) {
                let _ = everything(&mut store);
            }
        }
    });
}
