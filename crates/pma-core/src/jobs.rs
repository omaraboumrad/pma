//! Jobs: a `pma` command queued to run detached. `--detach` stores the
//! command's arguments and starts a runner unless one holds `runner.lock`.
//! The runner runs each job as a child `pma`, oldest first, and exits once
//! the queue is empty. Design: `docs/dev/async_dispatch.md`.
//!
//! A job runs under the agent watchdog, so the runner exiting kills the job,
//! and the job exiting kills its agents: nothing outlives the process that
//! recorded it as running.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::store::{self, Job, Result, Session, Store};

const RUNNER_LOCK: &str = "runner.lock";

/// Set by the runner on the `pma` it starts for a job.
const JOB_VAR: &str = "PMA_JOB";

/// The job this process runs for, if a runner started it.
pub fn current() -> Option<i64> {
    std::env::var(JOB_VAR).ok()?.parse().ok()
}

/// Queues this invocation, less `--detach`, and makes sure a runner will
/// take it. `targets` are the command's project arguments; one names the
/// job's project, for that project's view.
pub fn enqueue(store: &Store, targets: &[String]) -> Result<()> {
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != "--detach")
        .collect();
    let project = match targets {
        [one] => Some(one.split(':').next().unwrap_or(one)),
        _ => None,
    };
    let id = queue(store, &args, &std::env::current_dir()?, project)?;
    println!("job {id} queued; `pma job` lists jobs and `pma job log {id}` shows its output");
    Ok(())
}

/// `project` is the one the command is about, for the project's view.
pub fn queue(
    store: &Store,
    args: &[String],
    cwd: &std::path::Path,
    project: Option<&str>,
) -> Result<i64> {
    let id = store.add_job(args, cwd, project)?;
    spawn_runner()?;
    Ok(id)
}

/// Queues the command that approves the plan a job stopped at. The plan is
/// checked when that job runs: one that changed since is refused, as it is
/// in the foreground.
pub fn approve(store: &Store, id: i64) -> Result<String> {
    let job = store.job(id)?;
    let Some(args) = job.awaiting.filter(|_| job.state == "awaiting-approval") else {
        return Err(format!("job {id} is {}, not awaiting approval", job.state).into());
    };
    if !store.approve_job(id)? {
        return Err(format!("job {id} was approved already").into());
    }
    let next = queue(store, &args, &job.cwd, job.project.as_deref())?;
    Ok(format!("job {next} queued: pma {}", args.join(" ")))
}

/// Starts `pma runner` in a process group of its own, with no terminal, so
/// closing the terminal does not end it. One that finds the lock held exits.
fn spawn_runner() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let state = store::state_home()?;
    std::fs::create_dir_all(&state).map_err(|e| format!("{}: {e}", state.display()))?;
    let path = state.join("runner.log");
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Command::new(std::env::current_exe()?)
        .arg("runner")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    Ok(())
}

/// Runs queued jobs until none is left. The lock is released before the
/// last check, so a job queued while it was held is not stranded: its
/// command's runner found the lock held and exited.
pub fn run() -> Result<()> {
    let home = store::home()?;
    loop {
        let Some(lock) = Session::try_lock(&home, RUNNER_LOCK)? else {
            return Ok(());
        };
        let store = Store::open_default()?;
        store.fail_running_jobs()?;
        while let Some(job) = store.claim_job()? {
            if let Err(e) = run_one(&store, &home, &job) {
                eprintln!("job {}: {e}", job.id);
                store.finish_job(job.id, None)?;
            }
            if let Err(e) = notify(&store, job.id) {
                eprintln!("job {}: notify: {e}", job.id);
            }
        }
        drop(lock);
        if !store.has_queued_jobs()? {
            return Ok(());
        }
    }
}

fn run_one(store: &Store, home: &std::path::Path, job: &Job) -> Result<()> {
    // A command run in the foreground may hold the session. Waiting here
    // rather than in the job keeps the job from failing on the lock.
    while Session::try_acquire(home)?.is_none() {
        if store.job(job.id)?.state == "stopped" {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let log = log_path(job.id)?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(&job.args)
        .current_dir(&job.cwd)
        .env(JOB_VAR, job.id.to_string());
    let (mut child, lifeline) = crate::agent::spawn_guarded(cmd, None, &log)?;
    store.set_job_pid(job.id, child.id())?;
    // `stop` read no pid if it ran before the line above; it kills here.
    if store.job(job.id)?.state == "stopped" {
        kill_group(child.id().into());
    }
    let status = child.wait()?;
    // The watchdog's reader is still waiting on the pipe; closing it ends
    // the group.
    drop(lifeline);
    store.finish_job(job.id, status.code())?;
    Ok(())
}

/// Runs `notify` once a job has ended. Bounded and logged like an agent, so
/// a hung command cannot hold the queue.
fn notify(store: &Store, id: i64) -> Result<()> {
    let rows = store.config_rows()?;
    let cfg =
        crate::config::Config::with_overrides(rows.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
    let Some(command) = cfg.notify.filter(|c| !c.trim().is_empty()) else {
        return Ok(());
    };
    let job = store.job(id)?;
    let mut summary = format!("pma job {id} {}: pma {}", job.state, job.args.join(" "));
    if job.state == "awaiting-approval" {
        summary.push_str(&format!("; `pma job approve {id}` runs its plan"));
    }
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &command])
        .env("PMA_JOB_ID", id.to_string())
        .env("PMA_JOB_STATE", &job.state)
        .env("PMA_JOB_SUMMARY", &summary);
    let log = log_path(id)?.with_extension("notify.log");
    crate::agent::run_limited(cmd, None, &log, Duration::from_secs(30))?;
    Ok(())
}

pub fn log_path(id: i64) -> Result<PathBuf> {
    let dir = store::state_home()?.join("jobs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir.join(format!("{id}.log")))
}

/// Stops a queued job, or kills a running one's process group. Its agents
/// die with it; its runs are marked failed when the next session starts.
pub fn stop(store: &Store, id: i64) -> Result<String> {
    let job = store.job(id)?;
    let was = store.stop_job(id)?;
    Ok(match was.as_str() {
        "queued" => format!("job {id} stopped before it ran"),
        "running" => {
            if let Some(pid) = job.pid {
                kill_group(pid);
            }
            format!("job {id} stopped; its agents were killed")
        }
        other => format!("job {id} is {other}; nothing to stop"),
    })
}

fn kill_group(pid: i64) {
    let _ = Command::new("kill")
        .args(["-TERM", "--", &format!("-{pid}")])
        .status();
}

/// One line per job: id, state, exit code, and the command.
pub fn list(jobs: &[Job]) -> String {
    if jobs.is_empty() {
        return "no jobs. `--detach` on `pma dispatch` or `pma workflow run` queues one\n".into();
    }
    let mut out = String::new();
    for j in jobs {
        let exit = j.exit_code.map_or("-".into(), |c| c.to_string());
        out.push_str(&format!(
            "#{:<4} {:<17} exit {:<3} {}  pma {}\n",
            j.id,
            j.state,
            exit,
            crate::dates::format(crate::dates::day(j.created_at)),
            j.args.join(" ")
        ));
        if j.state == "awaiting-approval" {
            out.push_str(&format!(
                "      `pma job log {}` shows the plan; `pma job approve {}` runs it\n",
                j.id, j.id
            ));
        }
    }
    out
}
