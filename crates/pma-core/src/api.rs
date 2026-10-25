//! What a front end may read and do: `Query` returns a `View` of plain data,
//! and `Action` returns an `Outcome`. The CLI, the TUI and the web each render
//! these, so a front end that matches every variant cannot fall behind
//! another. Design: `docs/dev/frontends.md`.
//!
//! An action that runs agents or waits on the network is queued as a job
//! running the same CLI command, so no front end blocks on it.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::store::{
    Attempt, Campaign, Job, Note, Run, RunState, Session, Store, WorkflowInstance, WorkflowRevision,
};
use crate::{dates, dispatch, jobs, next, publish, rank, report_runs, store, todo};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Projects named outright and those carrying any of `tags`; empty is all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selection {
    pub projects: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    Next {
        of: Selection,
        all: bool,
    },
    Matrix {
        of: Selection,
        quadrant: Option<rank::Quadrant>,
        all: bool,
    },
    /// `all` ranks untiered projects as tier 5.
    Status {
        of: Selection,
        explain: bool,
        all: bool,
    },
    Stale {
        of: Selection,
        count: Option<usize>,
    },
    Report {
        by: report_runs::By,
    },
    /// Runs that are not final: what waits on a reviewer.
    Runs,
    /// One run, with its diff. Showing it records the tree shown, which an
    /// approval then requires.
    Run {
        id: i64,
    },
    /// Jobs and running agents, as `pma watch` reads them.
    Agents,
    Jobs,
    JobLog {
        id: i64,
    },
    Notes,
    Workflows,
    Campaigns,
    Campaign {
        name: String,
    },
    /// One project: its state, its open tasks and its runs.
    Project {
        name: String,
    },
}

pub enum View {
    Next {
        header: String,
        lists: next::Next,
        limit: Option<usize>,
    },
    Matrix {
        header: String,
        placed: Vec<rank::Placed>,
        limit: Option<usize>,
        quadrant: Option<rank::Quadrant>,
        today: i64,
    },
    Status {
        header: String,
        portfolio: Box<Portfolio>,
        explain: bool,
        /// The latest scans, newest first.
        scans: Vec<crate::history::Scan>,
        /// What the latest scan changed, in the selected projects.
        changes: Vec<crate::history::ProjectChange>,
        /// When each failing project's CI began failing.
        since: std::collections::HashMap<String, crate::history::Since>,
    },
    Stale {
        header: String,
        placed: Vec<rank::Placed>,
        limit: usize,
    },
    Report {
        runs: Vec<Run>,
        attempts: Vec<Attempt>,
        by: report_runs::By,
    },
    Runs(Vec<Run>),
    Run {
        run: Box<Run>,
        attempts: Vec<Attempt>,
        /// `None` when the project keeps no `CHANGELOG.md`; empty when the
        /// agent drafted no entry.
        changelog: Option<String>,
        diff: String,
        /// Set when the tree moved while it was read, so no approval may
        /// rest on what was shown.
        moved: bool,
    },
    Agents {
        state: PathBuf,
        jobs: Vec<Job>,
        runs: Vec<Run>,
        now: i64,
    },
    Jobs(Vec<Job>),
    /// `None` until the job writes output.
    JobLog {
        id: i64,
        text: Option<String>,
    },
    Notes(Vec<Note>),
    Workflows {
        revisions: Vec<WorkflowRevision>,
        instances: Vec<WorkflowInstance>,
    },
    Campaigns(Vec<Campaign>),
    Campaign {
        campaign: Campaign,
        runs: Vec<Run>,
    },
    Project {
        name: String,
        path: PathBuf,
        /// When a scan last read it; what the view shows is that old.
        scanned_at: Option<i64>,
        tags: Vec<String>,
        /// The verify command set for it; `None` detects one at dispatch.
        verify: Option<String>,
        /// Its states and status cells, as `pma status` shows them; `None`
        /// for a project no scan has found lately.
        status: Option<crate::report::StatusLine>,
        /// Its open tasks, in matrix order.
        placed: Vec<rank::Placed>,
        today: i64,
        /// Every run it has had, newest first.
        runs: Vec<Run>,
    },
}

/// The worker a queued action names, as `-a`, `-m` and `-p` do.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkerChoice {
    pub agent: Option<String>,
    pub model: Option<String>,
    pub preset: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Approves one run, read or not; the reviewer has read it. `seen` is
    /// the state a front end showed it in: the run must still be in it.
    ApproveRun {
        id: i64,
        seen: Option<String>,
    },
    /// Approves several runs at once, each of which must be clean.
    ApproveRuns {
        ids: Vec<i64>,
    },
    RejectRun {
        id: i64,
        keep_git_changes: bool,
        infra: bool,
        seen: Option<String>,
    },
    AddReviewTime {
        id: i64,
        minutes: u32,
    },
    /// Reads each open pull request's state from GitHub.
    SettlePrs,
    StopJob {
        id: i64,
    },
    ApproveJob {
        id: i64,
    },
    StopWorkflow {
        instance: i64,
    },
    AddNote {
        text: String,
    },
    EditNote {
        id: i64,
        text: String,
    },
    RemoveNote {
        id: i64,
    },

    // Queued as jobs.
    Scan {
        of: Selection,
        offline: bool,
        deps: bool,
    },
    Dispatch {
        targets: Vec<String>,
        tags: Vec<String>,
        auto: bool,
        count: Option<usize>,
        retry: bool,
        worker: WorkerChoice,
    },
    /// Starts or resumes an instance; with `approve`, runs that plan.
    WorkflowRun {
        name: String,
        projects: Vec<String>,
        tag: Option<String>,
        instance: Option<i64>,
        set: Vec<String>,
        approve: Option<String>,
        unattended: bool,
        local: bool,
        cap: Vec<String>,
        worker: WorkerChoice,
    },
    ReworkRun {
        id: i64,
        feedback: String,
    },
    Publish {
        ids: Vec<i64>,
        all_approved: bool,
        target: publish::Target,
    },
    RunCampaign {
        name: String,
        count: Option<usize>,
    },
    Sync {
        of: Selection,
        apply: bool,
    },
    Verify {
        of: Selection,
    },
}

/// What an action did. `said` goes to standard output, `warnings` to
/// standard error.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub said: Vec<String>,
    pub warnings: Vec<String>,
}

impl Outcome {
    fn said(line: impl Into<String>) -> Outcome {
        Outcome {
            said: vec![line.into()],
            warnings: Vec::new(),
        }
    }
}

/// Scans the status view lists.
pub const SCANS_SHOWN: usize = 10;

/// What the last scan recorded, for tiered projects.
pub struct Portfolio {
    pub cfg: Config,
    pub today: i64,
    pub projects: Vec<(rank::Project, store::ProjectRow)>,
    pub tasks: Vec<rank::Task>,
    /// Projects with no tier of their own.
    pub untiered: usize,
    /// The tier the untiered projects are ranked at, from `default_tier` or
    /// `--all`; `None` leaves them out of `projects`.
    pub ranked_as: Option<u8>,
    pub scanned_ago: i64,
}

impl Portfolio {
    pub fn header(&self) -> String {
        let ago = crate::report::ago(self.scanned_ago);
        let n = self.projects.len();
        match (self.untiered, self.ranked_as) {
            (0, _) => format!("last scan {ago}; {n} projects"),
            (u, Some(t)) => {
                format!("last scan {ago}; {n} projects, {u} untiered ranked as tier {t}")
            }
            (u, None) => format!(
                "last scan {ago}; {n} tiered projects, {u} untiered \
                 (pma project tier <1-5> <project>..., or pma config default_tier <1-5>)"
            ),
        }
    }
}

pub fn config(store: &Store) -> Result<Config> {
    let rows = store.config_rows()?;
    Ok(Config::with_overrides(
        rows.iter().map(|(k, v)| (k.as_str(), v.as_str())),
    )?)
}

pub fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// The projects named outright together with every project carrying one of
/// `tags`. Empty means all of them, so a `--tag` that matches nothing is an
/// error rather than a silent selection of the whole portfolio.
pub fn select(store: &Store, names: &[String], tags: &[String]) -> Result<Vec<String>> {
    if tags.is_empty() {
        return Ok(names.to_vec());
    }
    let mut out = names.to_vec();
    for project in store.projects_tagged(tags)? {
        if !out.contains(&project) {
            out.push(project);
        }
    }
    if out.is_empty() {
        return Err(format!("no project carries `{}`", tags.join("` or `")).into());
    }
    Ok(out)
}

pub fn portfolio(store: &Store, of: &Selection, all: bool) -> Result<Portfolio> {
    let cfg = config(store)?;
    let now = dates::now();
    let today = dates::day(now);
    let last = store
        .last_scan()?
        .ok_or("nothing scanned yet; run `pma scan`")?;

    let names = select(store, &of.projects, &of.tags)?;
    let rows = store.projects()?;
    for n in &names {
        if !rows.iter().any(|r| &r.name == n) {
            return Err(format!("unknown project `{n}`").into());
        }
    }
    // A project no scan can find has nothing to dispatch against, so it does
    // not compete for attention. `pma project` still lists it.
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| names.is_empty() || names.contains(&r.name))
        .filter(|r| r.absent_since.is_none())
        .collect();
    let ranked_as = cfg.tier_or_default(None).or(all.then_some(5));
    let untiered = rows.iter().filter(|r| r.tier.is_none()).count();
    let task_rows = store.tasks()?;

    let mut projects = Vec::new();
    let mut tasks = Vec::new();
    for row in rows {
        let Some(tier) = row.tier.or(ranked_as) else {
            continue;
        };
        let own: Vec<&store::TaskRow> =
            task_rows.iter().filter(|t| t.project == row.name).collect();
        let project = rank::Project {
            name: row.name.clone(),
            tier,
            open: own.iter().map(|t| t.priority).collect(),
            idle_days: row.last_activity.map(|t| (today - dates::day(t)).max(0)),
            ci: row.ci.clone(),
            dirty: row.dirty,
            leftover: row.leftover,
            ahead: row.ahead,
            deps: row.deps.map(|moves| {
                let days = |t: i64| (today - dates::day(t)).max(0);
                rank::Deps {
                    moves,
                    measured: row.deps_at.map_or(0, days),
                    lock: row.lock_at.map(days),
                    held_back: row.held_back,
                }
            }),
        };
        tasks.extend(own.iter().map(|t| rank::Task {
            project: row.name.clone(),
            tier,
            priority: t.priority,
            text: t.text.clone(),
            line: Some(t.line),
            key: Some(t.key.clone()),
            group: t.group.clone(),
            due: t.due.as_deref().and_then(dates::parse),
            tagged_urgent: t.tags.iter().any(|g| g == "urgent"),
            signal_urgent: false,
            eligible: t.tags.iter().any(|g| g == "agent") && !t.tags.iter().any(|g| g == "manual"),
            age_days:
                (today - dates::day(t.added_at.unwrap_or(t.first_seen).min(t.first_seen))).max(0),
        }));
        let mut signals = rank::signal_tasks(&cfg, &project);
        // Ranked at a default tier, a project's signals are shown, but an
        // agent takes them only when named: no one chose it for `--auto`.
        if row.tier.is_none() {
            signals.iter_mut().for_each(|s| s.eligible = false);
        }
        tasks.extend(signals);
        projects.push((project, row));
    }
    Ok(Portfolio {
        cfg,
        today,
        projects,
        tasks,
        untiered,
        ranked_as,
        scanned_ago: now - last,
    })
}

/// Whether a run that is not final holds this task, by key or by its text.
pub fn has_run(runs: &[Run], project: &str, key: &str, text: &str) -> bool {
    runs.iter().any(|r| {
        !r.state.is_final()
            && r.project == project
            && (r.task_key == key || todo::normal_text(&r.text) == todo::normal_text(text))
    })
}

pub fn next_lists(store: &Store, p: &Portfolio) -> Result<next::Next> {
    let active = store.runs()?;
    let absent: Vec<String> = store
        .projects()?
        .into_iter()
        .filter(|r| r.absent_since.is_some())
        .map(|r| r.name)
        .collect();
    let placed = rank::place(&p.cfg, p.tasks.clone(), p.today);
    let mut spent = std::collections::HashMap::new();
    for x in placed.iter().filter(|x| x.task.eligible) {
        let revision = dispatch::revision(&x.task.text);
        let n = store.consumed_attempts(&x.task.project, &revision)?;
        spent.insert((x.task.project.clone(), revision), n);
    }
    // What `--auto` checks before it takes a task, in the order it checks.
    let taken = |x: &rank::Placed| {
        let t = &x.task;
        if absent.contains(&t.project) {
            return next::Taken::Absent;
        }
        if let Some(key) = &t.key
            && has_run(&active, &t.project, key, &t.text)
        {
            return next::Taken::Held;
        }
        let used = spent
            .get(&(t.project.clone(), dispatch::revision(&t.text)))
            .copied()
            .unwrap_or(0);
        match used >= dispatch::ATTEMPT_LIMIT {
            true => next::Taken::Exhausted(used),
            false => next::Taken::Free,
        }
    };
    Ok(next::build(&placed, &active, p.today, taken))
}

/// A run's drafted changelog entries: `None` when its project keeps no
/// `CHANGELOG.md` and the agent drafted none, empty when it should have.
pub fn changelog(home: &Path, run: &Run) -> Option<String> {
    match std::fs::read_to_string(dispatch::changelog_draft(home, run.id)) {
        Ok(text) => Some(text),
        Err(_) => dispatch::wants_changelog(run).then(String::new),
    }
}

/// Whether a job or an agent run is under way, so a front end refreshes
/// often rather than now and then.
pub fn busy(store: &Store) -> Result<bool> {
    Ok(store
        .jobs(50)?
        .iter()
        .any(|j| matches!(j.state.as_str(), "queued" | "running"))
        || store.runs()?.iter().any(|r| r.state == RunState::Running))
}

/// Marks runs whose session died `failed`, when no session holds the lock.
pub fn settle_interrupted(store: &Store) -> Result<()> {
    if let Some(s) = Session::try_acquire(&store::home()?)? {
        store.fail_interrupted_runs(&s)?;
    }
    Ok(())
}

pub fn query(store: &Store, q: Query) -> Result<View> {
    Ok(match q {
        Query::Next { of, all } => {
            let p = portfolio(store, &of, false)?;
            View::Next {
                header: p.header(),
                lists: next_lists(store, &p)?,
                limit: (!all).then_some(p.cfg.quadrant_limit as usize),
            }
        }
        Query::Matrix { of, quadrant, all } => {
            let p = portfolio(store, &of, false)?;
            View::Matrix {
                header: p.header(),
                limit: (!all).then_some(p.cfg.quadrant_limit as usize),
                today: p.today,
                placed: rank::place(&p.cfg, p.tasks, p.today),
                quadrant,
            }
        }
        Query::Status { of, explain, all } => {
            let p = portfolio(store, &of, all)?;
            let scans = store.scans(SCANS_SHOWN)?;
            let names = select(store, &of.projects, &of.tags)?;
            let changes = match scans.first() {
                Some(latest) => store
                    .scan_changes(latest.id)?
                    .into_iter()
                    .filter(|c| names.is_empty() || names.contains(&c.project))
                    .collect(),
                None => Vec::new(),
            };
            View::Status {
                header: p.header(),
                portfolio: Box::new(p),
                explain,
                scans,
                changes,
                since: store.failing_since()?,
            }
        }
        Query::Stale { of, count } => {
            let p = portfolio(store, &of, false)?;
            View::Stale {
                header: p.header(),
                limit: count.unwrap_or(p.cfg.quadrant_limit as usize),
                placed: rank::place(&p.cfg, p.tasks, p.today),
            }
        }
        Query::Report { by } => View::Report {
            runs: store.runs()?,
            attempts: store.attempts(None)?,
            by,
        },
        Query::Runs => {
            settle_interrupted(store)?;
            View::Runs(
                store
                    .runs()?
                    .into_iter()
                    .filter(|r| !r.state.is_final())
                    .collect(),
            )
        }
        Query::Run { id } => {
            let run = store.run(id)?;
            let home = store::state_home()?;
            // The tree on both sides of the diff, so an approval can require
            // what was shown. A tree that moved meanwhile is not recorded.
            let approvable = matches!(run.state, RunState::Ready | RunState::Approved);
            let before = approvable.then(|| dispatch::tree(&run).ok()).flatten();
            // A workflow node that is not an `edit` has no branch, and its tree
            // is gone once its output is read.
            let diff = if run.branch.is_empty() {
                "(none: this node changes no files)".to_string()
            } else {
                dispatch::diff(&run).unwrap_or_else(|e| format!("(no diff: {e})"))
            };
            let after = approvable.then(|| dispatch::tree(&run).ok()).flatten();
            let moved = match (before, after) {
                (Some(a), Some(b)) if a == b => {
                    store.set_reviewed_tree(run.id, &a)?;
                    false
                }
                (Some(_), Some(_)) => true,
                _ => false,
            };
            View::Run {
                attempts: store.attempts(Some(run.id))?,
                changelog: changelog(&home, &run),
                run: Box::new(run),
                diff,
                moved,
            }
        }
        Query::Agents => View::Agents {
            state: store::state_home()?,
            jobs: store.jobs(50)?,
            runs: store.runs()?,
            now: dates::now(),
        },
        Query::Jobs => View::Jobs(store.jobs(20)?),
        Query::JobLog { id } => {
            store.job(id)?;
            View::JobLog {
                id,
                text: crate::agent::read_log(&jobs::log_path(id)?),
            }
        }
        Query::Notes => View::Notes(store.notes()?),
        Query::Workflows => View::Workflows {
            revisions: store.workflow_revisions()?,
            instances: store.workflow_instances()?,
        },
        Query::Campaigns => View::Campaigns(store.campaigns()?),
        Query::Campaign { name } => View::Campaign {
            campaign: store.campaign(&name)?,
            runs: store.runs()?,
        },
        Query::Project { name } => {
            let row = store
                .project(&name)?
                .ok_or_else(|| format!("unknown project `{name}`"))?;
            let p = portfolio(
                store,
                &Selection {
                    projects: vec![name.clone()],
                    tags: Vec::new(),
                },
                true,
            )?;
            let status = p.projects.first().and_then(|(project, row)| {
                let r = crate::report::StatusRow {
                    project,
                    tiered: row.tier.is_some(),
                    has_todo: row.has_todo,
                    lint_errors: row.lint_errors,
                    faults: &row.faults,
                };
                crate::report::status_rows(&p.cfg, &[r]).into_iter().next()
            });
            let mut runs: Vec<Run> = store
                .runs()?
                .into_iter()
                .filter(|r| r.project == name)
                .collect();
            runs.sort_by_key(|r| std::cmp::Reverse(r.id));
            View::Project {
                scanned_at: row.scanned_at,
                path: row.path,
                tags: store
                    .project_tags()?
                    .into_iter()
                    .filter(|(p, _)| *p == name)
                    .map(|(_, t)| t)
                    .collect(),
                verify: p.cfg.project(&name).verify,
                status,
                placed: rank::place(&p.cfg, p.tasks, p.today),
                today: p.today,
                runs,
                name,
            }
        }
    })
}

/// Refuses a run that left the state a front end showed it in.
fn unmoved(run: &Run, seen: Option<&str>) -> Result<()> {
    match seen {
        Some(s) if s != run.state.name() => Err(format!(
            "run #{} is now {}, not {s}; read it again",
            run.id,
            run.state.name()
        )
        .into()),
        _ => Ok(()),
    }
}

pub fn act(store: &Store, a: Action) -> Result<Outcome> {
    let note_text = |t: String| -> Result<String> {
        let t = t.trim().to_string();
        if t.is_empty() {
            return Err("a note needs text".into());
        }
        Ok(t)
    };
    Ok(match a {
        Action::ApproveRun { id, seen } => {
            let mut run = store.run(id)?;
            unmoved(&run, seen.as_deref())?;
            dispatch::approve(store, &mut run, &whoami())?;
            Outcome::default()
        }
        Action::ApproveRuns { ids } => {
            let mut runs = Vec::new();
            for id in &ids {
                let run = store.run(*id)?;
                let reasons = crate::accept::review_reasons(&run);
                if !reasons.is_empty() {
                    return Err(format!(
                        "#{id} is not clean, so it is not a batch approval: {}. \
                         Approve it on its own after reading it.",
                        reasons.join("; ")
                    )
                    .into());
                }
                runs.push(run);
            }
            let by = whoami();
            let mut out = Outcome::default();
            for mut run in runs {
                dispatch::approve(store, &mut run, &by)?;
                out.said
                    .push(format!("#{} {}: approved", run.id, run.project));
            }
            out
        }
        Action::RejectRun {
            id,
            keep_git_changes,
            infra,
            seen,
        } => {
            // Rejecting changes the worktree, so it holds the session.
            let session = Session::acquire(&store::home()?)?;
            store.fail_interrupted_runs(&session)?;
            let mut run = store.run(id)?;
            unmoved(&run, seen.as_deref())?;
            dispatch::reject(store, &mut run, keep_git_changes, infra)?;
            Outcome::default()
        }
        Action::AddReviewTime { id, minutes } => {
            store.run(id)?;
            store.add_review_seconds(id, i64::from(minutes) * 60)?;
            Outcome::default()
        }
        Action::SettlePrs => {
            let mut out = Outcome::default();
            publish::settle(store, |run, outcome| match outcome {
                Ok(o) => out.said.push(format!("#{} {}: {o}", run.id, run.project)),
                Err(e) => out.warnings.push(format!(
                    "#{} {}: pull request state unknown: {e}",
                    run.id, run.project
                )),
            })?;
            out
        }
        Action::StopJob { id } => Outcome::said(jobs::stop(store, id)?),
        Action::ApproveJob { id } => Outcome::said(jobs::approve(store, id)?),
        Action::StopWorkflow { instance } => {
            let _session = Session::acquire(&store::home()?)?;
            let found = store
                .workflow_instance(instance)?
                .ok_or_else(|| format!("no workflow instance {instance}"))?;
            if found.finished_at.is_some() {
                return Err(format!(
                    "instance {instance} is already {}",
                    found.outcome.as_deref().unwrap_or("finished")
                )
                .into());
            }
            store.finish_instance(instance, "stopped")?;
            let _ = std::fs::remove_dir_all(
                store::state_home()?
                    .join("workflow-trees")
                    .join(instance.to_string()),
            );
            Outcome::said(format!(
                "instance {instance} stopped; runs it made stand in `pma review`"
            ))
        }
        Action::AddNote { text } => {
            let id = store.add_note(&note_text(text)?, dates::now())?;
            Outcome::said(format!("#{id}"))
        }
        Action::EditNote { id, text } => {
            if !store.edit_note(id, &note_text(text)?, dates::now())? {
                return Err(format!("no note #{id}").into());
            }
            Outcome::default()
        }
        Action::RemoveNote { id } => {
            if !store.remove_note(id)? {
                return Err(format!("no note #{id}").into());
            }
            Outcome::default()
        }
        Action::Scan {
            offline: true,
            deps: true,
            ..
        } => return Err("a scan is offline or reads dependencies, not both".into()),
        queued => {
            let args = command_line(&queued).expect("every other action is queued");
            let id = jobs::queue(store, &args, &std::env::current_dir()?)?;
            Outcome::said(format!(
                "job {id} queued; `pma job` lists jobs and `pma job log {id}` shows its output"
            ))
        }
    })
}

/// The CLI command a queued action runs; `None` for one that runs in place.
pub fn command_line(a: &Action) -> Option<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let mut push = |s: &str| v.push(s.to_string());
    let flag = |v: &mut Vec<String>, on: bool, name: &str| {
        if on {
            v.push(name.to_string());
        }
    };
    // `--name=value`, so a value that starts with `-` is not read as a flag.
    let opt = |v: &mut Vec<String>, name: &str, value: &Option<String>| {
        if let Some(x) = value {
            v.push(format!("{name}={x}"));
        }
    };
    let worker = |v: &mut Vec<String>, w: &WorkerChoice| {
        opt(v, "--agent", &w.agent);
        opt(v, "--model", &w.model);
        opt(v, "--preset", &w.preset);
    };
    let tags = |v: &mut Vec<String>, tags: &[String]| {
        for t in tags {
            v.push(format!("--tag={t}"));
        }
    };
    match a {
        Action::Scan { of, offline, deps } => {
            push("scan");
            v.extend(of.projects.iter().cloned());
            tags(&mut v, &of.tags);
            flag(&mut v, *offline, "--offline");
            flag(&mut v, *deps, "--deps");
        }
        Action::Dispatch {
            targets,
            tags: t,
            auto,
            count,
            retry,
            worker: w,
        } => {
            push("dispatch");
            v.extend(targets.iter().cloned());
            tags(&mut v, t);
            flag(&mut v, *auto, "--auto");
            opt(&mut v, "--count", &count.map(|n| n.to_string()));
            flag(&mut v, *retry, "--retry");
            worker(&mut v, w);
        }
        Action::WorkflowRun {
            name,
            projects,
            tag,
            instance,
            set,
            approve,
            unattended,
            local,
            cap,
            worker: w,
        } => {
            v.extend(["workflow".to_string(), "run".into(), name.clone()]);
            v.extend(projects.iter().cloned());
            opt(&mut v, "--tag", tag);
            opt(&mut v, "--instance", &instance.map(|n| n.to_string()));
            for s in set {
                v.push(format!("--set={s}"));
            }
            opt(&mut v, "--approve", approve);
            flag(&mut v, *unattended, "--unattended");
            flag(&mut v, *local, "--local");
            for c in cap {
                v.push(format!("--cap={c}"));
            }
            worker(&mut v, w);
        }
        Action::ReworkRun { id, feedback } => {
            v.extend([
                "review".to_string(),
                id.to_string(),
                format!("--rework={feedback}"),
            ]);
        }
        Action::Publish {
            ids,
            all_approved,
            target,
        } => {
            push(target.name());
            v.extend(ids.iter().map(i64::to_string));
            flag(&mut v, *all_approved, "--all-approved");
        }
        Action::RunCampaign { name, count } => {
            v.extend(["campaign".to_string(), "run".into(), name.clone()]);
            opt(&mut v, "--count", &count.map(|n| n.to_string()));
        }
        Action::Sync { of, apply } => {
            push("sync");
            v.extend(of.projects.iter().cloned());
            tags(&mut v, &of.tags);
            flag(&mut v, *apply, "--apply");
        }
        Action::Verify { of } => {
            push("verify");
            v.extend(of.projects.iter().cloned());
            tags(&mut v, &of.tags);
        }
        Action::ApproveRun { .. }
        | Action::ApproveRuns { .. }
        | Action::RejectRun { .. }
        | Action::AddReviewTime { .. }
        | Action::SettlePrs
        | Action::StopJob { .. }
        | Action::ApproveJob { .. }
        | Action::StopWorkflow { .. }
        | Action::AddNote { .. }
        | Action::EditNote { .. }
        | Action::RemoveNote { .. } => return None,
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A queued action's command line is one the CLI parses, with each flag
    /// spelled as the CLI spells it.
    #[test]
    fn queued_actions_name_their_cli_command() {
        let line = |a: Action| command_line(&a).expect("queued");
        let w = WorkerChoice {
            agent: None,
            model: Some("haiku".into()),
            preset: None,
        };
        assert_eq!(
            line(Action::WorkflowRun {
                name: "review-fix".into(),
                projects: vec!["argdec".into()],
                tag: None,
                instance: Some(3),
                set: vec!["breadth=5".into()],
                approve: Some("abc".into()),
                unattended: false,
                local: true,
                cap: vec![],
                worker: w.clone(),
            }),
            [
                "workflow",
                "run",
                "review-fix",
                "argdec",
                "--instance=3",
                "--set=breadth=5",
                "--approve=abc",
                "--local",
                "--model=haiku"
            ]
        );
        assert_eq!(
            line(Action::Dispatch {
                targets: vec![],
                tags: vec!["py".into()],
                auto: true,
                count: Some(2),
                retry: false,
                worker: w,
            }),
            [
                "dispatch",
                "--tag=py",
                "--auto",
                "--count=2",
                "--model=haiku"
            ]
        );
        assert_eq!(
            line(Action::Publish {
                ids: vec![4],
                all_approved: false,
                target: publish::Target::Pr
            }),
            ["pr", "4"]
        );
    }
}
