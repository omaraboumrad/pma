//! A `View` as an interactive front end shows it: tables of rows to select,
//! or a detail of fields and sections; and how a key on a selected row
//! becomes an `Action`. The TUI and the web both draw a `Screen` and send what
//! `action` builds, so neither decides for itself what a row is, what a key
//! does to it, or what a state means. Each cell carries a `Tone`, so both
//! colour a failed run or a critical task alike.

use crate::api::{self, Action, Selection, View, WorkerChoice};
use crate::keys::{Act, Tab};
use crate::store::{Job, Preset, Run, RunState};
use crate::worker::Worker;
use crate::{accept, dates, history, next, publish, rank, report, report_runs, scan, watch};

/// What a row is about.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Nothing,
    /// A task, by its dispatch target; `retry` once its attempts are used.
    Task {
        project: String,
        target: String,
        retry: bool,
    },
    /// A task no agent may take.
    Manual {
        project: String,
    },
    Run {
        id: i64,
        /// Ready, with no reason to read it: a batch approval may take it.
        clean: bool,
        /// The state the row showed. Approving or rejecting is refused once
        /// the run has left it, so no one acts on a run they did not see.
        state: String,
        project: String,
    },
    Job {
        id: i64,
        awaiting: bool,
    },
    Instance {
        id: i64,
        workflow: String,
        open: bool,
    },
    Note {
        id: i64,
        text: String,
    },
    Campaign {
        name: String,
    },
    /// A project, as a branch of the hub.
    Project {
        name: String,
    },
    Worker {
        name: String,
    },
    /// A preset, whole, so editing it starts from what it says.
    Preset {
        name: String,
        agent: String,
        /// Empty for the worker's own default.
        model: String,
        args: Vec<String>,
    },
}

/// What a value means, for a front end to colour: its palette, one meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    /// Context: ages, ids, counts of nothing.
    Muted,
    /// Under way.
    Accent,
    /// Done and accepted.
    Good,
    /// Waits on you.
    Warn,
    /// Failed, or critical.
    Bad,
    /// A total.
    Strong,
}

impl Tone {
    pub fn name(self) -> &'static str {
        match self {
            Tone::Plain => "plain",
            Tone::Muted => "muted",
            Tone::Accent => "accent",
            Tone::Good => "good",
            Tone::Warn => "warn",
            Tone::Bad => "bad",
            Tone::Strong => "strong",
        }
    }
}

/// How a column lays out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Short text, as wide as its longest value.
    Text,
    /// The column that takes the remaining width; truncated, never wrapped.
    Wide,
    /// Right-aligned.
    Number,
    /// A state or a priority: a short word shown as a badge.
    Badge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub title: &'static str,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    pub tone: Tone,
    /// A page on GitHub the cell names, for a front end that can open one.
    pub link: Option<String>,
}

/// Whether `act` can apply to a row about `pick`. An act that needs no row
/// fits any. A front end offers only the acts that fit the selected row.
pub fn fits(act: Act, pick: &Pick) -> bool {
    match act {
        Act::Dispatch => matches!(pick, Pick::Task { .. }),
        Act::Verify => matches!(
            pick,
            Pick::Task { .. } | Pick::Manual { .. } | Pick::Project { .. }
        ),
        Act::ApproveRun
        | Act::RejectRun
        | Act::ReworkRun
        | Act::AddReviewTime
        | Act::Stage
        | Act::Commit
        | Act::Pr
        | Act::Push => matches!(pick, Pick::Run { .. }),
        Act::ApproveJob => matches!(pick, Pick::Job { awaiting: true, .. }),
        Act::StopJob => matches!(pick, Pick::Job { .. }),
        Act::StopWorkflow => matches!(pick, Pick::Instance { open: true, .. }),
        Act::ResumeWorkflow => matches!(pick, Pick::Instance { open: true, .. }),
        Act::RunCampaign => matches!(pick, Pick::Campaign { .. }),
        Act::EditNote | Act::RemoveNote => matches!(pick, Pick::Note { .. }),
        Act::SetWorkerField => matches!(pick, Pick::Worker { .. }),
        Act::EditPreset => matches!(pick, Pick::Preset { .. }),
        Act::MakeDefault | Act::RemoveConfig => {
            matches!(pick, Pick::Worker { .. } | Pick::Preset { .. })
        }
        _ => !act.on_row(),
    }
}

impl Pick {
    /// The project a row is about, when it names one.
    pub fn project(&self) -> Option<&str> {
        match self {
            Pick::Task { project, .. }
            | Pick::Manual { project }
            | Pick::Run { project, .. }
            | Pick::Project { name: project } => Some(project),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub cells: Vec<Cell>,
    pub pick: Pick,
}

impl Item {
    /// The text of each cell, for a test or a plain listing.
    pub fn texts(&self) -> Vec<&str> {
        self.cells.iter().map(|c| c.text.as_str()).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub title: String,
    pub columns: Vec<Column>,
    pub items: Vec<Item>,
    /// What an empty pane says instead of nothing.
    pub empty: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Text to read, wrapped.
    Prose,
    /// A unified diff: lines starting `+`, `-` and `@@` mean something.
    Diff,
    /// Output kept as written, in a fixed-width font.
    Log,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub title: String,
    /// A badge beside the title, such as the run's state.
    pub badge: Option<Cell>,
    /// A warning above everything else.
    pub notice: Option<String>,
    pub fields: Vec<(String, String)>,
    pub sections: Vec<(String, Section, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Panes(Vec<Pane>),
    Detail(Detail),
    /// Branches to open and close, with the selected one's facts beside.
    Tree(Tree),
}

/// A tree whose rows share columns after the name: each node's `cells`
/// line up under `columns`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    pub columns: Vec<Column>,
    pub nodes: Vec<Node>,
}

/// One row of a tree: a name with a few words after it in a lighter weight,
/// its cells under the tree's columns, and what opening it in full shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique in its tree and stable across loads, so a front end keeps
    /// what is open and what is selected when the tree is read again. It is
    /// the path from the root: the same run under two branches is two nodes.
    pub id: String,
    /// One character naming what the row is: a group, a project, a task, a
    /// run or a job.
    pub icon: Cell,
    pub label: Cell,
    /// Short words after the name, such as a project's tier.
    pub facts: Vec<Cell>,
    /// One per column of the tree; an empty cell leaves its column blank.
    pub cells: Vec<Cell>,
    pub pick: Pick,
    /// Open unless the viewer closed it: a branch holding something that
    /// waits on you starts open, the rest closed.
    pub open: bool,
    /// The selected row's facts in full, label and value.
    pub fields: Vec<(String, Cell)>,
    /// Text under the fields: a task's words, a run's summary.
    pub notes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

/// A row of a tree as a front end draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Visible<'a> {
    pub node: &'a Node,
    pub depth: usize,
    pub open: bool,
    /// The index of its parent among the visible rows.
    pub parent: Option<usize>,
}

/// The rows of `nodes` a viewer sees, in order: a branch's children follow
/// it while it is open. `chosen` holds what the viewer opened or closed,
/// by id, over each node's own default.
pub fn visible<'a>(
    nodes: &'a [Node],
    chosen: &std::collections::HashMap<String, bool>,
) -> Vec<Visible<'a>> {
    fn walk<'a>(
        nodes: &'a [Node],
        depth: usize,
        parent: Option<usize>,
        chosen: &std::collections::HashMap<String, bool>,
        out: &mut Vec<Visible<'a>>,
    ) {
        for node in nodes {
            let open = !node.children.is_empty() && *chosen.get(&node.id).unwrap_or(&node.open);
            out.push(Visible {
                node,
                depth,
                open,
                parent,
            });
            if open {
                let at = out.len() - 1;
                walk(&node.children, depth + 1, Some(at), chosen, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, None, chosen, &mut out);
    out
}

/// Every node of a tree as an item, open or not, for an act over many rows
/// such as approving every clean run. A run under two branches counts once.
pub fn tree_items(nodes: &[Node]) -> Vec<Item> {
    fn walk(nodes: &[Node], out: &mut Vec<Item>) {
        for n in nodes {
            if !out.iter().any(|i| i.pick == n.pick) {
                out.push(Item {
                    cells: vec![n.label.clone()],
                    pick: n.pick.clone(),
                });
            }
            walk(&n.children, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

/// The binding `key` has on `tab` for the selected row. A tab may bind one
/// key to acts on two kinds of row, as the hub binds `a` to approve a run or
/// a job's plan; the act that fits the row wins.
pub fn lookup(key: &str, tab: Tab, pick: &Pick) -> Option<&'static crate::keys::Binding> {
    crate::keys::BINDINGS
        .iter()
        .filter(|b| b.key == key && b.tabs.contains(&tab))
        .find(|b| matches!(b.command, crate::keys::Command::Act(a) if fits(a, pick)))
        .or_else(|| crate::keys::lookup(key, tab))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// The tab it belongs to; `None` for a detail opened from one.
    pub tab: Option<Tab>,
    /// A line above the body, such as when the last scan ran.
    pub header: Option<String>,
    /// Facts above the body, label and value: a project's tier, state, CI.
    pub summary: Vec<(String, Cell)>,
    pub body: Body,
}

fn cell(text: impl Into<String>, tone: Tone) -> Cell {
    Cell {
        text: text.into(),
        tone,
        link: None,
    }
}

/// `c` linking to `url`, when the URL is a GitHub page. Run URLs come from
/// `gh`'s output; a page must not receive any other scheme.
fn linked(c: Cell, url: Option<&str>) -> Cell {
    Cell {
        link: url
            .filter(|u| u.starts_with("https://github.com/"))
            .map(String::from),
        ..c
    }
}

fn plain(text: impl Into<String>) -> Cell {
    cell(text, Tone::Plain)
}

fn muted(text: impl Into<String>) -> Cell {
    cell(text, Tone::Muted)
}

const fn col(title: &'static str, kind: Kind) -> Column {
    Column { title, kind }
}

/// A run's state, by what it asks of you.
pub fn run_tone(state: &str) -> Tone {
    match state {
        "queued" | "running" | "pr-open" => Tone::Accent,
        "ready" => Tone::Warn,
        "approved" | "pushed" | "merged" | "applied" | "done" => Tone::Good,
        "failed" | "rejected" => Tone::Bad,
        _ => Tone::Muted,
    }
}

pub fn job_tone(state: &str) -> Tone {
    match state {
        "queued" | "running" => Tone::Accent,
        "awaiting-approval" => Tone::Warn,
        "done" | "approved" => Tone::Good,
        "failed" => Tone::Bad,
        _ => Tone::Muted,
    }
}

pub fn priority_tone(priority: &str) -> Tone {
    match priority {
        "critical" => Tone::Bad,
        "high" => Tone::Warn,
        "low" => Tone::Muted,
        _ => Tone::Plain,
    }
}

/// A project's worst state, as `pma status` names it.
fn state_tone(state: &str) -> Tone {
    match state.split(" +").next().unwrap_or(state) {
        "failing CI"
        | "git settings changed"
        | "git unreadable"
        | "TODO unreadable"
        | "TODO lint"
        | "critical" => Tone::Bad,
        "CI unknown" | "unpublished" | "unpublished unknown" | "scan partial" | "stale deps" => {
            Tone::Warn
        }
        "idle" => Tone::Muted,
        "clear" => Tone::Good,
        _ => Tone::Plain,
    }
}

/// How soon a task is due, by how soon.
fn when_tone(when: &str) -> Tone {
    match when {
        w if w.starts_with("overdue") || w == "due today" => Tone::Bad,
        w if w.starts_with("due") || w == "#urgent" || w == "signal" => Tone::Warn,
        _ => Tone::Muted,
    }
}

fn task_pick(p: &rank::Placed) -> Pick {
    let t = &p.task;
    match (&t.key, t.line) {
        (Some(_), Some(line)) => Pick::Task {
            project: t.project.clone(),
            target: format!("{}:{line}", t.project),
            retry: false,
        },
        (Some(key), None) => Pick::Task {
            project: t.project.clone(),
            target: format!("{}:{key}", t.project),
            retry: false,
        },
        (None, _) => Pick::Manual {
            project: t.project.clone(),
        },
    }
}

fn next_pick(s: &next::Subject) -> Pick {
    match s {
        next::Subject::Task {
            project,
            target,
            retry,
        } => Pick::Task {
            project: project.clone(),
            target: target.clone(),
            retry: *retry,
        },
        next::Subject::Manual { project } => Pick::Manual {
            project: project.clone(),
        },
        next::Subject::Run { id, state, project } => Pick::Run {
            id: *id,
            clean: false,
            state: state.clone(),
            project: project.clone(),
        },
    }
}

/// A task's cells as `pma next` builds them: place, tier, priority, when,
/// an optional reason, and its text.
fn task_cells(cells: &[String]) -> Vec<Cell> {
    let get = |i: usize| cells.get(i).cloned().unwrap_or_default();
    let (why, text) = match cells.len() {
        n if n >= 6 => (get(4), get(5)),
        _ => (String::new(), get(4)),
    };
    vec![
        plain(get(0)),
        muted(get(1)),
        cell(get(2), priority_tone(&get(2))),
        cell(get(3), when_tone(&get(3))),
        cell(why, Tone::Warn),
        plain(text),
    ]
}

const TASK_COLUMNS: [Column; 6] = [
    col("task", Kind::Text),
    col("tier", Kind::Text),
    col("priority", Kind::Badge),
    col("when", Kind::Text),
    col("why", Kind::Text),
    col("title", Kind::Wide),
];

fn run_cells(r: &Run, now: i64) -> Vec<Cell> {
    vec![
        muted(format!("#{}", r.id)),
        cell(r.state.name(), run_tone(r.state.name())),
        plain(r.project.clone()),
        plain(r.text.clone()),
        cell(
            r.cost_usd.map_or("-".into(), |c| format!("${c:.2}")),
            Tone::Muted,
        ),
        muted(r.dispatched_at.map_or("-".into(), |t| report::ago(now - t))),
    ]
}

const RUN_COLUMNS: [Column; 6] = [
    col("run", Kind::Text),
    col("state", Kind::Badge),
    col("project", Kind::Text),
    col("task", Kind::Wide),
    col("cost", Kind::Number),
    col("started", Kind::Text),
];

fn run_detail(
    run: &Run,
    attempts: &[crate::store::Attempt],
    changelog: Option<String>,
    diff: String,
    moved: bool,
) -> Detail {
    let fields = report::run_fields(run)
        .into_iter()
        .filter(|r| r[0] != "task")
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    let mut sections = Vec::new();
    if let Some(s) = run.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        sections.push(("Summary".into(), Section::Prose, s.to_string()));
    }
    match changelog.as_deref().map(str::trim) {
        Some("") => sections.push((
            "Changelog".into(),
            Section::Prose,
            "No entry drafted.".into(),
        )),
        Some(entries) => sections.push(("Changelog".into(), Section::Prose, entries.into())),
        None => {}
    }
    let reasons = accept::review_reasons(run);
    sections.push((
        "Why read it".into(),
        Section::Prose,
        match reasons.is_empty() {
            true => "Every recorded gate is clean.".into(),
            false => reasons
                .iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n"),
        },
    ));
    if attempts.len() > 1 {
        sections.push((
            "Attempts".into(),
            Section::Log,
            report::table(&report::attempt_rows(attempts), ""),
        ));
    }
    sections.push(("Diff".into(), Section::Diff, diff));
    Detail {
        title: format!("#{} {}", run.id, run.text),
        badge: Some(cell(run.state.name(), run_tone(run.state.name()))),
        notice: moved
            .then(|| "The run changed while it was shown. Read it again before approving.".into()),
        fields,
        sections,
    }
}

/// A status row's cells, toned: `pma status`'s project, tier, state, open,
/// idle, ci, deps, local and todo.
/// A change's tone: bad news, good news, or work arriving.
pub fn change_tone(c: &history::Change) -> Tone {
    match c {
        c if c.bad() => Tone::Bad,
        c if c.good() => Tone::Good,
        _ => Tone::Accent,
    }
}

fn changes_pane(
    changes: Vec<history::ProjectChange>,
    projects: &[(rank::Project, crate::store::ProjectRow)],
) -> Pane {
    let repo = |name: &str| {
        projects
            .iter()
            .find(|(_, r)| r.name == name)
            .and_then(|(_, r)| r.slug.as_deref())
            .and_then(scan::github_url)
    };
    Pane {
        title: "Changed at the last scan".into(),
        columns: vec![
            col("project", Kind::Text),
            col("change", Kind::Badge),
            col("detail", Kind::Wide),
        ],
        items: changes
            .into_iter()
            .map(|c| {
                let url = repo(&c.project);
                let detail = match &c.change {
                    history::Change::CiFailing(_) => url.as_ref().map(|u| format!("{u}/actions")),
                    _ => None,
                };
                Item {
                    cells: vec![
                        linked(plain(c.project.clone()), url.as_deref()),
                        cell(c.change.name(), change_tone(&c.change)),
                        linked(plain(c.change.detail()), detail.as_deref()),
                    ],
                    pick: Pick::Manual { project: c.project },
                }
            })
            .collect(),
        empty: "The last scan changed nothing since the one before.",
    }
}

fn scans_pane(scans: Vec<history::Scan>, now: i64) -> Pane {
    let count = |n: i64, tone: Tone| match n {
        0 => muted("0"),
        n => cell(n.to_string(), tone),
    };
    // A fault no scan found leaves its cells empty, so its column is hidden.
    let fault = |n: i64, f: scan::Fault| match (n, f) {
        (0, _) => plain(""),
        (n, scan::Fault::GitSettingsChanged) => cell(n.to_string(), Tone::Bad),
        (n, _) => cell(n.to_string(), Tone::Warn),
    };
    let mut columns = vec![
        col("scan", Kind::Text),
        col("when", Kind::Text),
        col("kind", Kind::Text),
        col("projects", Kind::Number),
        col("failing", Kind::Number),
    ];
    columns.extend(scan::Fault::ALL.map(|f| col(f.name(), Kind::Number)));
    columns.extend([
        col("open", Kind::Number),
        col("opened", Kind::Number),
        col("closed", Kind::Number),
        col("other", Kind::Number),
    ]);
    Pane {
        title: "Recent scans".into(),
        columns,
        items: scans
            .into_iter()
            .map(|s| Item {
                cells: [
                    muted(format!("#{}", s.id)),
                    plain(report::ago(now - s.at)),
                    muted(if s.full { "full" } else { "partial" }),
                    plain(s.projects.to_string()),
                    count(s.failing, Tone::Bad),
                ]
                .into_iter()
                .chain(
                    scan::Fault::ALL
                        .iter()
                        .zip(s.faults)
                        .map(|(f, n)| fault(n, *f)),
                )
                .chain([
                    plain(s.open.to_string()),
                    count(s.opened, Tone::Accent),
                    count(s.closed, Tone::Good),
                    count(s.other, Tone::Plain),
                ])
                .collect(),
                pick: Pick::Nothing,
            })
            .collect(),
        empty: "No scan recorded yet.",
    }
}

fn status_cells(c: &[String]) -> Vec<Cell> {
    let get = |i: usize| c.get(i).cloned().unwrap_or_default();
    let ci = get(5);
    let local = get(7);
    let todo = get(8);
    vec![
        plain(get(0)),
        muted(get(1)),
        cell(get(2), state_tone(&get(2))),
        plain(get(3)),
        muted(get(4)),
        cell(
            ci.clone(),
            match ci.as_str() {
                "failing" => Tone::Bad,
                "passing" => Tone::Good,
                _ => Tone::Muted,
            },
        ),
        cell(
            get(6),
            match get(6).as_str() {
                "-" | "0" => Tone::Muted,
                _ => Tone::Warn,
            },
        ),
        cell(
            local.clone(),
            match local.as_str() {
                "clean" => Tone::Muted,
                _ => Tone::Warn,
            },
        ),
        cell(
            todo.clone(),
            match todo.as_str() {
                "ok" => Tone::Muted,
                "missing" => Tone::Warn,
                _ => Tone::Bad,
            },
        ),
    ]
}

/// The hub's columns after the name.
const HUB_COLUMNS: [Column; 6] = [
    col("status", Kind::Badge),
    col("q", Kind::Text),
    col("priority", Kind::Text),
    col("when", Kind::Text),
    col("by", Kind::Text),
    col("cost", Kind::Number),
];

/// An age in a column's width: `now`, `12m ago`, `5h ago`, `34d ago`.
fn short_ago(seconds: i64) -> String {
    match seconds {
        s if s < 90 => "now".into(),
        s if s < 90 * 60 => format!("{}m ago", s / 60),
        s if s < 36 * 3600 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// A hub row's cells, in `HUB_COLUMNS` order.
fn hub_cells(status: Cell, q: Cell, priority: Cell, when: Cell, by: Cell, cost: Cell) -> Vec<Cell> {
    vec![status, q, priority, when, by, cost]
}

/// Whether a run waits on you: to read, to publish or to merge, as
/// `pma next` lists it.
fn run_needs_you(r: &Run) -> bool {
    matches!(
        r.state,
        RunState::Ready | RunState::Failed | RunState::Approved | RunState::PrOpen
    )
}

fn run_pick(r: &Run) -> Pick {
    Pick::Run {
        id: r.id,
        clean: r.state == RunState::Ready && accept::review_reasons(r).is_empty(),
        state: r.state.name().into(),
        project: r.project.clone(),
    }
}

/// A task's dispatch target, as `pma next` and `task_pick` write it.
fn target_of(t: &rank::Task) -> String {
    match t.line {
        Some(line) => format!("{}:{line}", t.project),
        None => format!("{}:{}", t.project, t.key.as_deref().unwrap_or("")),
    }
}

/// Whether `r` worked on `t`, by its key or its text, as dispatch matches a
/// run to its task.
fn run_of(r: &Run, t: &rank::Task) -> bool {
    r.project == t.project
        && (t.key.as_deref() == Some(r.task_key.as_str())
            || crate::todo::normal_text(&r.text) == crate::todo::normal_text(&t.text))
}

/// What a job works on, read from its command line: a dispatch names its
/// tasks, a publish or a review its run, a scan, a verify or a workflow its
/// projects. Words that name none of these, such as a flag's value, match
/// nothing in the tree and are left out there.
#[derive(Debug, Clone, PartialEq)]
enum Works {
    Task(String),
    Run(i64),
    Project(String),
}

fn works_on(j: &Job) -> Vec<Works> {
    let words = || j.args.iter().skip(1).filter(|w| !w.starts_with('-'));
    match j.args.first().map(String::as_str) {
        Some("dispatch") => words()
            .filter(|w| w.contains(':'))
            .map(|w| Works::Task(w.clone()))
            .collect(),
        Some("review" | "stage" | "commit" | "pr" | "push") => words()
            .filter_map(|w| w.parse().ok())
            .map(Works::Run)
            .collect(),
        Some("scan" | "verify") => words().map(|w| Works::Project(w.clone())).collect(),
        // `workflow run <name> <projects...>`
        Some("workflow") => words().skip(2).map(|w| Works::Project(w.clone())).collect(),
        _ => Vec::new(),
    }
}

fn job_live(j: &Job) -> bool {
    matches!(j.state.as_str(), "queued" | "running" | "awaiting-approval")
}

/// Whether a job shows under the work it touches: while it is live, and for
/// a day after it fails, since a failed job may leave no run to say so. A
/// job that finished is told by the run it made.
fn job_shown(j: &Job, now: i64) -> bool {
    job_live(j) || (j.state == "failed" && now - j.finished_at.unwrap_or(j.created_at) < 86_400)
}

fn run_node(parent: &str, r: &Run, jobs: &[&Job], now: i64) -> Node {
    let state = r.state.name();
    let cells = hub_cells(
        cell(state, run_tone(state)),
        plain(""),
        plain(""),
        muted(
            r.dispatched_at
                .map_or(String::new(), |t| short_ago(now - t)),
        ),
        muted(match &r.model {
            Some(m) => format!("{} {m}", r.agent),
            None => r.agent.clone(),
        }),
        muted(r.cost_usd.map_or(String::new(), |c| format!("${c:.2}"))),
    );
    let mut fields = vec![
        ("state".to_string(), cell(state, run_tone(state))),
        ("project".to_string(), plain(r.project.clone())),
    ];
    fields.extend(
        report::run_fields(r)
            .into_iter()
            .filter(|f| f.len() == 2)
            .map(|f| (f[0].clone(), plain(f[1].clone()))),
    );
    let reasons = accept::review_reasons(r);
    if r.state == RunState::Ready {
        fields.push((
            "review".into(),
            match reasons.is_empty() {
                true => cell("clean", Tone::Good),
                false => cell(reasons.join("; "), Tone::Warn),
            },
        ));
    }
    let mut notes = Vec::new();
    if let Some(s) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        notes.push(("Summary".to_string(), s.to_string()));
    }
    if let Some(e) = r.error.as_deref().filter(|e| !e.trim().is_empty()) {
        notes.push(("Error".to_string(), e.to_string()));
    }
    let id = format!("{parent}/r{}", r.id);
    Node {
        icon: cell("●", run_tone(state)),
        label: cell(format!("run #{}", r.id), Tone::Strong),
        facts: Vec::new(),
        cells,
        pick: run_pick(r),
        open: jobs.iter().any(|j| job_live(j)),
        fields,
        notes,
        children: jobs.iter().map(|j| job_node(&id, j, now)).collect(),
        id,
    }
}

fn job_node(parent: &str, j: &Job, now: i64) -> Node {
    let command = format!("pma {}", j.args.join(" "));
    let mut fields = vec![
        (
            "state".to_string(),
            cell(j.state.clone(), job_tone(&j.state)),
        ),
        ("command".to_string(), plain(command.clone())),
        ("in".to_string(), plain(j.cwd.display().to_string())),
        ("queued".to_string(), plain(report::ago(now - j.created_at))),
    ];
    if let Some(c) = j.exit_code {
        fields.push(("exit".into(), plain(c.to_string())));
    }
    let mut notes = Vec::new();
    if j.state == "awaiting-approval" {
        notes.push((
            "Waiting".to_string(),
            "Enter shows the plan; `a` runs it.".to_string(),
        ));
    }
    Node {
        id: format!("{parent}/j{}", j.id),
        icon: cell("≡", job_tone(&j.state)),
        label: cell(format!("job #{}", j.id), Tone::Strong),
        facts: vec![muted(report::truncate(&command, 48))],
        cells: hub_cells(
            cell(j.state.clone(), job_tone(&j.state)),
            plain(""),
            plain(""),
            muted(short_ago(now - j.created_at)),
            plain(""),
            plain(""),
        ),
        pick: Pick::Job {
            id: j.id,
            awaiting: j.state == "awaiting-approval",
        },
        open: false,
        fields,
        notes,
        children: Vec::new(),
    }
}

/// What is happening to a task now, for its status: a job at
/// work on it, else its latest run's state, else nothing.
fn activity(runs: &[&Run], jobs: &[&Job]) -> Option<Cell> {
    if let Some(j) = jobs.iter().find(|j| job_live(j)) {
        let what = match j.state.as_str() {
            "awaiting-approval" => "plan waiting".to_string(),
            s => s.to_string(),
        };
        return Some(cell(format!("job #{} {what}", j.id), job_tone(&j.state)));
    }
    let r = runs.first()?;
    let state = r.state.name();
    let what = match r.state {
        RunState::Ready => "ready to review",
        RunState::Approved => "approved, to publish",
        RunState::PrOpen => "pull request open",
        _ => state,
    };
    Some(cell(format!("run #{} {what}", r.id), run_tone(state)))
}

/// A task, and under it what worked or works on it: the jobs dispatching
/// it, then its runs, newest first, each with the jobs publishing it.
#[allow(clippy::too_many_arguments)]
fn task_node<'a>(
    parent: &str,
    p: &rank::Placed,
    today: i64,
    now: i64,
    retry: bool,
    runs: &[&Run],
    jobs: &[&Job],
    run_jobs: &dyn Fn(i64) -> Vec<&'a Job>,
) -> Node {
    let t = &p.task;
    let target = target_of(t);
    let when = report::when(p, today);
    let priority = t.priority.name();
    let cells = hub_cells(
        activity(runs, jobs).unwrap_or_else(|| plain("")),
        muted(format!("{:?}", p.quadrant)),
        cell(priority, priority_tone(priority)),
        cell(when.clone(), when_tone(&when)),
        match t.eligible {
            true => cell("agent", Tone::Accent),
            false => muted("you"),
        },
        plain(""),
    );
    let mut fields = vec![
        ("project".to_string(), plain(t.project.clone())),
        ("target".to_string(), plain(target.clone())),
        (
            "quadrant".to_string(),
            plain(format!("{:?} {}", p.quadrant, p.quadrant.title())),
        ),
        (
            "priority".to_string(),
            cell(priority, priority_tone(priority)),
        ),
        ("when".to_string(), cell(when.clone(), when_tone(&when))),
        (
            "for".to_string(),
            match (t.eligible, retry) {
                (true, true) => cell("agent, its attempts used", Tone::Warn),
                (true, false) => cell("agent", Tone::Accent),
                (false, _) => muted("you"),
            },
        ),
        ("tier".to_string(), muted(format!("T{}", t.tier))),
    ];
    if let Some(g) = &t.group {
        fields.push(("heading".into(), plain(g.clone())));
    }
    if !runs.is_empty() {
        fields.push(("runs".into(), plain(runs.len().to_string())));
    }
    let pick = match task_pick(p) {
        Pick::Task {
            project, target, ..
        } => Pick::Task {
            project,
            target,
            retry,
        },
        other => other,
    };
    let id = format!("{parent}/t{target}");
    let mut children: Vec<Node> = jobs.iter().map(|j| job_node(&id, j, now)).collect();
    children.extend(runs.iter().map(|r| run_node(&id, r, &run_jobs(r.id), now)));
    Node {
        open: jobs.iter().any(|j| job_live(j))
            || runs
                .iter()
                .any(|r| run_needs_you(r) || r.state == RunState::Running),
        icon: match t.eligible {
            true => cell("○", Tone::Accent),
            false => muted("○"),
        },
        // The columns must fit beside it; the whole text is in the notes.
        label: plain(report::truncate(&t.text, 70)),
        facts: Vec::new(),
        cells,
        pick,
        fields,
        notes: vec![("Task".to_string(), t.text.clone())],
        children,
        id,
    }
}

/// A project: its open tasks in matrix order, each with what works on it;
/// then its live runs whose task is gone, the jobs on the project as a
/// whole, and its finished runs.
#[allow(clippy::too_many_arguments)]
fn project_node<'a>(
    row: &crate::store::ProjectRow,
    tier: u8,
    tasks: &[&rank::Placed],
    runs: &[Run],
    jobs: &[(&'a Job, Vec<Works>)],
    today: i64,
    now: i64,
    retries: &[String],
) -> Node {
    let id = format!("p{}", row.name);
    let run_jobs = |run: i64| -> Vec<&'a Job> {
        jobs.iter()
            .filter(|(_, w)| w.contains(&Works::Run(run)))
            .map(|(j, _)| *j)
            .collect()
    };
    let mut attached: Vec<i64> = Vec::new();
    let mut placed_jobs: Vec<i64> = Vec::new();
    let mut children: Vec<Node> = tasks
        .iter()
        .map(|p| {
            let target = target_of(&p.task);
            let own: Vec<&Run> = runs.iter().filter(|r| run_of(r, &p.task)).collect();
            attached.extend(own.iter().map(|r| r.id));
            let on: Vec<&Job> = jobs
                .iter()
                .filter(|(_, w)| w.contains(&Works::Task(target.clone())))
                .map(|(j, _)| *j)
                .collect();
            placed_jobs.extend(on.iter().map(|j| j.id));
            let retry = retries.contains(&target);
            task_node(&id, p, today, now, retry, &own, &on, &run_jobs)
        })
        .collect();
    let (live, done): (Vec<&Run>, Vec<&Run>) = runs
        .iter()
        .filter(|r| r.project == row.name && !attached.contains(&r.id))
        .partition(|r| !r.state.is_final());
    children.extend(live.iter().map(|r| run_node(&id, r, &run_jobs(r.id), now)));
    // A job on the project as a whole, or on a task no longer open here.
    let prefix = format!("{}:", row.name);
    children.extend(
        jobs.iter()
            .filter(|(j, w)| {
                !placed_jobs.contains(&j.id)
                    && w.iter().any(|w| match w {
                        Works::Project(p) => *p == row.name,
                        Works::Task(t) => t.starts_with(&prefix),
                        Works::Run(_) => false,
                    })
            })
            .map(|(j, _)| job_node(&id, j, now)),
    );
    if !done.is_empty() {
        let shown = done.len().min(10);
        let history = format!("{id}/done");
        children.push(Node {
            id: history.clone(),
            icon: muted("▤"),
            label: cell("finished runs", Tone::Muted),
            facts: vec![muted(match done.len() > shown {
                true => format!("latest {shown} of {}", done.len()),
                false => done.len().to_string(),
            })],
            cells: hub_cells(
                plain(""),
                plain(""),
                plain(""),
                plain(""),
                plain(""),
                plain(""),
            ),
            pick: Pick::Nothing,
            open: false,
            fields: vec![("runs".to_string(), plain(done.len().to_string()))],
            notes: vec![(
                "Finished runs".to_string(),
                "Runs of this project whose task is no longer open: shipped, \
                 rejected, or from a campaign or a workflow."
                    .to_string(),
            )],
            children: done[..shown]
                .iter()
                .map(|r| run_node(&history, r, &run_jobs(r.id), now))
                .collect(),
        });
    }
    let own: Vec<&Run> = runs.iter().filter(|r| r.project == row.name).collect();
    let waiting = own.iter().filter(|r| run_needs_you(r)).count();
    let running = own.iter().filter(|r| r.state == RunState::Running).count();
    let mut held = Vec::new();
    jobs_under(&children, &mut held);
    let busy = jobs
        .iter()
        .filter(|(j, _)| job_live(j) && held.contains(&j.id))
        .count();
    let stale = row.scanned_at.is_none_or(|t| now - t > 86_400);
    let scanned = row.scanned_at.map_or("never scanned".to_string(), |t| {
        format!("scanned {}", short_ago(now - t))
    });
    let facts = vec![
        muted(format!("T{tier}")),
        muted(match tasks.len() {
            1 => "1 task".to_string(),
            n => format!("{n} tasks"),
        }),
    ];
    // Its status, worst first, in the tone of the worst.
    let mut status: Vec<(String, Tone)> = Vec::new();
    if let crate::scan::Ci::Failing(_) = row.ci {
        status.push(("CI failing".into(), Tone::Bad));
    }
    if waiting > 0 {
        status.push((format!("{waiting} need you"), Tone::Warn));
    }
    if running > 0 {
        status.push((format!("{running} running"), Tone::Accent));
    }
    if busy > 0 {
        status.push((
            match busy {
                1 => "1 job".to_string(),
                n => format!("{n} jobs"),
            },
            Tone::Accent,
        ));
    }
    let cells = hub_cells(
        match status.first() {
            None => plain(""),
            Some((_, tone)) => cell(
                status
                    .iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                *tone,
            ),
        },
        plain(""),
        plain(""),
        // A day's drift is enough for CI or a branch to move.
        cell(
            scanned.clone(),
            match stale {
                true => Tone::Warn,
                false => Tone::Muted,
            },
        ),
        plain(""),
        plain(""),
    );
    let (ci, why) = row.ci.to_columns();
    let mut fields = vec![
        ("path".to_string(), plain(row.path.display().to_string())),
        (
            "tier".to_string(),
            match row.tier {
                Some(t) => plain(format!("T{t}")),
                None => muted(format!("untiered, ranked as T{tier}")),
            },
        ),
        (
            "ci".to_string(),
            cell(
                match why.is_empty() {
                    true => ci.to_string(),
                    false => format!("{ci}: {why}"),
                },
                match row.ci {
                    crate::scan::Ci::Failing(_) => Tone::Bad,
                    crate::scan::Ci::Passing => Tone::Good,
                    _ => Tone::Muted,
                },
            ),
        ),
        (
            "deps".to_string(),
            match row.deps {
                None => muted("not measured"),
                Some(0) => muted("up to date"),
                Some(n) => cell(format!("{n} outdated"), Tone::Warn),
            },
        ),
        (
            "dirty".to_string(),
            match row.dirty {
                0 => muted("clean"),
                n => cell(format!("{n} files"), Tone::Warn),
            },
        ),
        (
            "scanned".to_string(),
            cell(
                scanned,
                match stale {
                    true => Tone::Warn,
                    false => Tone::Muted,
                },
            ),
        ),
    ];
    for (fault, why) in &row.faults {
        fields.push((fault.name().to_string(), cell(why.clone(), Tone::Warn)));
    }
    Node {
        id,
        icon: cell(
            "◆",
            match (&row.ci, waiting) {
                (crate::scan::Ci::Failing(_), _) => Tone::Bad,
                (_, 1..) => Tone::Warn,
                _ => Tone::Accent,
            },
        ),
        label: cell(row.name.clone(), Tone::Strong),
        facts,
        cells,
        pick: Pick::Project {
            name: row.name.clone(),
        },
        open: waiting > 0 || running > 0 || busy > 0,
        fields,
        notes: Vec::new(),
        children,
    }
}

/// The jobs held anywhere under `nodes`, each once.
fn jobs_under(nodes: &[Node], out: &mut Vec<i64>) {
    for n in nodes {
        if let Pick::Job { id, .. } = n.pick
            && !out.contains(&id)
        {
            out.push(id);
        }
        jobs_under(&n.children, out);
    }
}

/// The hub: every project, its tasks under it, and under each task what
/// works on it. The second value is the live jobs no project holds, such as
/// a scan of every project, for the header to name.
fn hub_tree<'a>(
    portfolio: &api::Portfolio,
    placed: &[rank::Placed],
    lists: &next::Next,
    runs: &[Run],
    jobs: &'a [Job],
    today: i64,
    now: i64,
) -> (Vec<Node>, Vec<&'a Job>) {
    let retries: Vec<String> = lists
        .tasks
        .iter()
        .filter_map(|r| match &r.subject {
            next::Subject::Task {
                target,
                retry: true,
                ..
            } => Some(target.clone()),
            _ => None,
        })
        .collect();
    let shown: Vec<(&'a Job, Vec<Works>)> = jobs
        .iter()
        .filter(|j| job_shown(j, now))
        .map(|j| (j, works_on(j)))
        .collect();

    // Projects in the order of their first task in the matrix, then those
    // with runs alone, newest run first.
    let mut order: Vec<&str> = Vec::new();
    for p in placed {
        if !order.contains(&p.task.project.as_str()) {
            order.push(&p.task.project);
        }
    }
    for r in runs {
        if !order.contains(&r.project.as_str()) {
            order.push(&r.project);
        }
    }
    let projects: Vec<Node> = order
        .iter()
        .filter_map(|name| {
            let (project, row) = portfolio.projects.iter().find(|(_, r)| r.name == *name)?;
            let tasks: Vec<&rank::Placed> =
                placed.iter().filter(|p| p.task.project == *name).collect();
            Some(project_node(
                row,
                project.tier,
                &tasks,
                runs,
                &shown,
                today,
                now,
                &retries,
            ))
        })
        .collect();
    let mut held = Vec::new();
    jobs_under(&projects, &mut held);
    let loose = shown
        .iter()
        .map(|(j, _)| *j)
        .filter(|j| job_live(j) && !held.contains(&j.id))
        .collect();
    (projects, loose)
}

/// The screen a view makes. Every view has one, so a query a front end can
/// send is a screen it can draw.
pub fn screen(view: View) -> Screen {
    let panes = |tab: Tab, header: Option<String>, panes: Vec<Pane>| Screen {
        tab: Some(tab),
        header,
        summary: Vec::new(),
        body: Body::Panes(panes),
    };
    let detail = |tab: Option<Tab>, header: Option<String>, d: Detail| Screen {
        tab,
        header,
        summary: Vec::new(),
        body: Body::Detail(d),
    };
    let now = dates::now();
    match view {
        View::Hub {
            header,
            portfolio,
            placed,
            lists,
            runs,
            jobs,
            today,
        } => {
            let (tree, loose) = hub_tree(&portfolio, &placed, &lists, &runs, &jobs, today, now);
            // A live job on no one project, such as a scan of them all.
            let loose: Vec<String> = loose
                .iter()
                .map(|j| format!("job #{} {}: pma {}", j.id, j.state, j.args.join(" ")))
                .collect();
            Screen {
                tab: Some(Tab::Hub),
                header: Some(match loose.is_empty() {
                    true => header,
                    false => format!("{header}; {}", loose.join("; ")),
                }),
                summary: Vec::new(),
                body: Body::Tree(Tree {
                    columns: HUB_COLUMNS.to_vec(),
                    nodes: tree,
                }),
            }
        }
        View::Next { header, lists, .. } => {
            let rows = |rows: Vec<next::Row>, cells: fn(&[String]) -> Vec<Cell>| -> Vec<Item> {
                rows.into_iter()
                    .map(|r| Item {
                        pick: next_pick(&r.subject),
                        cells: cells(&r.cells),
                    })
                    .collect()
            };
            let run_row = |c: &[String]| -> Vec<Cell> {
                let get = |i: usize| c.get(i).cloned().unwrap_or_default();
                vec![
                    muted(get(0)),
                    cell(get(1), run_tone(&get(1))),
                    cell(get(2), Tone::Warn),
                    plain(get(3)),
                    plain(get(4)),
                ]
            };
            panes(
                Tab::Next,
                Some(header),
                vec![
                    Pane {
                        title: "Ready for agents".into(),
                        columns: TASK_COLUMNS.to_vec(),
                        items: rows(lists.agents, task_cells),
                        empty: "Nothing an agent may take. Tag a TODO.md item #agent.",
                    },
                    Pane {
                        title: "Runs waiting on you".into(),
                        columns: vec![
                            col("run", Kind::Text),
                            col("state", Kind::Badge),
                            col("next", Kind::Text),
                            col("project", Kind::Text),
                            col("task", Kind::Wide),
                        ],
                        items: rows(lists.runs, run_row),
                        empty: "No run waits on you.",
                    },
                    Pane {
                        title: "Tasks for you".into(),
                        columns: TASK_COLUMNS.to_vec(),
                        items: rows(lists.tasks, task_cells),
                        empty: "No critical or urgent task needs you.",
                    },
                ],
            )
        }
        View::Matrix {
            header,
            placed,
            today,
            ..
        } => {
            let mut quadrants: Vec<Pane> = rank::Quadrant::ALL
                .iter()
                .map(|q| Pane {
                    title: format!("{q:?} {}", q.title()),
                    columns: vec![
                        col("task", Kind::Text),
                        col("priority", Kind::Badge),
                        col("when", Kind::Text),
                        col("title", Kind::Wide),
                    ],
                    items: Vec::new(),
                    empty: "Nothing here.",
                })
                .collect();
            for p in &placed {
                let place = match p.task.line {
                    Some(line) => format!("{}:{line}", p.task.project),
                    None => p.task.project.clone(),
                };
                let when = report::when(p, today);
                let priority = p.task.priority.name();
                quadrants[p.quadrant as usize].items.push(Item {
                    cells: vec![
                        plain(place),
                        cell(priority, priority_tone(priority)),
                        cell(when.clone(), when_tone(&when)),
                        plain(p.task.text.clone()),
                    ],
                    pick: task_pick(p),
                });
            }
            panes(Tab::Matrix, Some(header), quadrants)
        }
        View::Status {
            header,
            portfolio: p,
            scans,
            changes,
            since,
            ..
        } => {
            let rows: Vec<report::StatusRow> = p
                .projects
                .iter()
                .map(|(project, row)| report::StatusRow {
                    project,
                    tiered: row.tier.is_some(),
                    has_todo: row.has_todo,
                    lint_errors: row.lint_errors,
                    faults: &row.faults,
                })
                .collect();
            let row_of = |name: &str| p.projects.iter().map(|(_, r)| r).find(|r| r.name == name);
            let repo = |name: &str| {
                row_of(name)
                    .and_then(|r| r.slug.as_deref())
                    .and_then(scan::github_url)
            };
            let items: Vec<Item> = report::status_rows(&p.cfg, &rows)
                .into_iter()
                .map(|(_, c)| {
                    let name = c.first().cloned().unwrap_or_default();
                    let url = repo(&name);
                    let mut cells = status_cells(&c);
                    cells[0] = linked(cells[0].clone(), url.as_deref());
                    cells[5] = linked(
                        cells[5].clone(),
                        url.map(|u| format!("{u}/actions")).as_deref(),
                    );
                    Item {
                        pick: Pick::Manual { project: name },
                        cells,
                    }
                })
                .collect();
            let latest = scans.first().map(|s| s.at);
            let mut failing = Vec::new();
            for item in &items {
                let Pick::Manual { project } = &item.pick else {
                    continue;
                };
                let Some(row) = row_of(project) else {
                    continue;
                };
                let scan::Ci::Failing(workflows) = &row.ci else {
                    continue;
                };
                let actions = repo(project).map(|u| format!("{u}/actions"));
                for w in workflows {
                    let run = row
                        .ci_runs
                        .iter()
                        .find(|(name, _)| name == w)
                        .map(|(_, u)| u.clone())
                        .or_else(|| actions.clone());
                    failing.push(Item {
                        cells: vec![
                            linked(plain(project.clone()), repo(project).as_deref()),
                            linked(cell(w.clone(), Tone::Bad), run.as_deref()),
                            plain(history::since_text(since.get(project), latest, now)),
                            linked(muted(run.clone().unwrap_or_default()), run.as_deref()),
                        ],
                        pick: Pick::Manual {
                            project: project.clone(),
                        },
                    });
                }
            }
            panes(
                Tab::Status,
                Some(header),
                vec![
                    Pane {
                        title: "Projects, worst first".into(),
                        columns: vec![
                            col("project", Kind::Text),
                            col("tier", Kind::Text),
                            col("state", Kind::Badge),
                            col("open", Kind::Number),
                            col("idle", Kind::Number),
                            col("ci", Kind::Badge),
                            col("deps", Kind::Number),
                            col("local", Kind::Text),
                            col("todo", Kind::Text),
                        ],
                        items,
                        empty: "No tiered project. `pma project tier <1-5> <project>` ranks one.",
                    },
                    Pane {
                        title: "Failing CI".into(),
                        columns: vec![
                            col("project", Kind::Text),
                            col("workflow", Kind::Badge),
                            col("failing", Kind::Text),
                            col("run", Kind::Wide),
                        ],
                        items: failing,
                        empty: "No workflow fails on a default branch.",
                    },
                    changes_pane(changes, &p.projects),
                    scans_pane(scans, now),
                ],
            )
        }
        View::Stale {
            header,
            placed,
            limit,
        } => detail(
            None,
            Some(header),
            Detail {
                title: "Stale tasks".into(),
                badge: None,
                notice: None,
                fields: Vec::new(),
                sections: vec![(
                    "Oldest first".into(),
                    Section::Log,
                    report::stale(&placed, limit),
                )],
            },
        ),
        View::Report { runs, attempts, by } => {
            let by_name = report_runs::By::ALL
                .iter()
                .find(|(_, b)| *b == by)
                .map_or("?", |(n, _)| n);
            let (summary, items) = match report_runs::table_of(&runs, &attempts, by) {
                None => ("No finished runs yet.".to_string(), Vec::new()),
                Some((summary, rows)) => {
                    let last = rows.len().saturating_sub(1);
                    let items = rows
                        .into_iter()
                        .enumerate()
                        .skip(1)
                        .map(|(i, r)| Item {
                            cells: r
                                .into_iter()
                                .map(|t| match i == last {
                                    true => cell(t, Tone::Strong),
                                    false => plain(t),
                                })
                                .collect(),
                            pick: Pick::Nothing,
                        })
                        .collect();
                    (summary, items)
                }
            };
            let mut columns = vec![col(by_name, Kind::Wide)];
            columns.extend(report_runs::HEADINGS.iter().map(|h| col(h, Kind::Number)));
            panes(
                Tab::Report,
                Some(summary),
                vec![Pane {
                    title: format!("Runs by {by_name}"),
                    columns,
                    items,
                    empty: "Dispatch some tasks first.",
                }],
            )
        }
        View::Runs(runs) => {
            let items = runs
                .iter()
                .map(|r| Item {
                    cells: run_cells(r, now),
                    pick: Pick::Run {
                        id: r.id,
                        clean: r.state == RunState::Ready && accept::review_reasons(r).is_empty(),
                        state: r.state.name().into(),
                        project: r.project.clone(),
                    },
                })
                .collect::<Vec<_>>();
            panes(
                Tab::Runs,
                None,
                vec![Pane {
                    title: "Runs not yet final".into(),
                    columns: RUN_COLUMNS.to_vec(),
                    items,
                    empty: "No run waits on a decision.",
                }],
            )
        }
        View::Run {
            run,
            attempts,
            changelog,
            diff,
            moved,
        } => detail(
            None,
            Some(run.project.clone()),
            run_detail(&run, &attempts, changelog, diff, moved),
        ),
        View::Agents {
            state,
            jobs,
            runs,
            now,
            workers,
            presets,
            agent,
            preset,
        } => {
            let agents = watch::agents(&state, &runs, now)
                .into_iter()
                .map(|a| Item {
                    cells: vec![
                        muted(format!("#{}", a.run.id)),
                        plain(a.run.project.clone()),
                        plain(a.run.node.clone().unwrap_or_else(|| "dispatch".into())),
                        muted(a.run.agent.clone()),
                        cell(a.running, Tone::Accent),
                        muted(a.quiet),
                        plain(a.progress.tool_calls.to_string()),
                        plain(
                            a.progress
                                .last_tool
                                .or(a.progress.last_text)
                                .unwrap_or_default(),
                        ),
                    ],
                    pick: Pick::Run {
                        id: a.run.id,
                        clean: false,
                        state: a.run.state.name().into(),
                        project: a.run.project.clone(),
                    },
                })
                .collect();
            let open = watch::open_jobs(&jobs)
                .into_iter()
                .map(|j| Item {
                    cells: vec![
                        muted(format!("#{}", j.id)),
                        cell(j.state.clone(), job_tone(&j.state)),
                        plain(format!("pma {}", j.args.join(" "))),
                    ],
                    pick: Pick::Job {
                        id: j.id,
                        awaiting: j.state == "awaiting-approval",
                    },
                })
                .collect();
            panes(
                Tab::Agents,
                None,
                vec![
                    Pane {
                        title: "Running agents".into(),
                        columns: vec![
                            col("run", Kind::Text),
                            col("project", Kind::Text),
                            col("node", Kind::Text),
                            col("agent", Kind::Text),
                            col("running", Kind::Text),
                            col("quiet", Kind::Text),
                            col("tools", Kind::Number),
                            col("doing", Kind::Wide),
                        ],
                        items: agents,
                        empty: "No agent is running.",
                    },
                    Pane {
                        title: "Open jobs".into(),
                        columns: vec![
                            col("job", Kind::Text),
                            col("state", Kind::Badge),
                            col("command", Kind::Wide),
                        ],
                        items: open,
                        empty: "No job is queued, running or waiting.",
                    },
                    worker_pane(&workers, &presets, &agent),
                    preset_pane(&presets, &workers, preset.as_deref()),
                ],
            )
        }
        View::Worker {
            worker,
            default,
            presets,
        } => detail(
            None,
            None,
            Detail {
                title: format!("Worker {}", worker.name),
                badge: default.then(|| cell("default", Tone::Accent)),
                notice: None,
                fields: worker
                    .fields()
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                sections: vec![
                    (
                        "Presets".into(),
                        Section::Log,
                        match presets.is_empty() {
                            true => format!(
                                "None. `+` on the agents tab adds one: <name> {} [model]",
                                worker.name
                            ),
                            false => presets
                                .iter()
                                .map(|p| {
                                    format!(
                                        "{}  {}  {}",
                                        p.name,
                                        p.model.as_deref().unwrap_or("its own default"),
                                        p.args.join(" ")
                                    )
                                    .trim_end()
                                    .to_string()
                                })
                                .collect::<Vec<_>>()
                                .join("\n"),
                        },
                    ),
                    (
                        "Fields".into(),
                        Section::Log,
                        crate::worker::FIELDS
                            .iter()
                            .map(|(n, what)| format!("{n:<16} {what}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ],
            },
        ),
        View::Jobs(jobs) => {
            let items = jobs
                .iter()
                .map(|j| Item {
                    cells: vec![
                        muted(format!("#{}", j.id)),
                        cell(j.state.clone(), job_tone(&j.state)),
                        muted(report::ago(now - j.created_at)),
                        plain(format!("pma {}", j.args.join(" "))),
                    ],
                    pick: Pick::Job {
                        id: j.id,
                        awaiting: j.state == "awaiting-approval",
                    },
                })
                .collect::<Vec<_>>();
            panes(
                Tab::Jobs,
                None,
                vec![Pane {
                    title: "Jobs".into(),
                    columns: vec![
                        col("job", Kind::Text),
                        col("state", Kind::Badge),
                        col("queued", Kind::Text),
                        col("command", Kind::Wide),
                    ],
                    items,
                    empty: "No jobs. `--detach` on dispatch or workflow run queues one.",
                }],
            )
        }
        View::JobLog { id, text: log } => detail(
            None,
            None,
            Detail {
                title: format!("Job #{id}"),
                badge: None,
                notice: None,
                fields: Vec::new(),
                sections: vec![(
                    "Output".into(),
                    Section::Log,
                    log.unwrap_or_else(|| "No output yet.".into()),
                )],
            },
        ),
        View::Notes(notes) => {
            let items = notes
                .into_iter()
                .map(|n| Item {
                    cells: vec![
                        muted(format!("#{}", n.id)),
                        muted(dates::format(dates::day(n.updated_at))),
                        plain(n.text.clone()),
                    ],
                    pick: Pick::Note {
                        id: n.id,
                        text: n.text,
                    },
                })
                .collect::<Vec<_>>();
            panes(
                Tab::Notes,
                None,
                vec![Pane {
                    title: "Notes".into(),
                    columns: vec![
                        col("note", Kind::Text),
                        col("updated", Kind::Text),
                        col("text", Kind::Wide),
                    ],
                    items,
                    empty: "No notes. Press n to add one.",
                }],
            )
        }
        View::Workflows {
            revisions,
            instances,
        } => {
            let open: Vec<Item> = instances
                .iter()
                .map(|i| {
                    let outcome = i.outcome.clone().unwrap_or_else(|| "open".into());
                    Item {
                        cells: vec![
                            muted(i.id.to_string()),
                            plain(i.workflow.clone()),
                            plain(i.target.clone()),
                            cell(
                                outcome.clone(),
                                match outcome.as_str() {
                                    "open" => Tone::Accent,
                                    "finished" => Tone::Good,
                                    "failed" => Tone::Bad,
                                    _ => Tone::Muted,
                                },
                            ),
                            muted(report::ago(now - i.started_at)),
                        ],
                        pick: Pick::Instance {
                            id: i.id,
                            workflow: i.workflow.clone(),
                            open: i.finished_at.is_none(),
                        },
                    }
                })
                .collect();
            let revs: Vec<Item> = revisions
                .iter()
                .map(|r| Item {
                    cells: vec![
                        muted(r.revision.to_string()),
                        match r.activated_at {
                            None => muted("draft"),
                            Some(_) => cell("active", Tone::Good),
                        },
                        plain(
                            r.document()
                                .map(|d| {
                                    d.workflows
                                        .iter()
                                        .map(|w| w.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                })
                                .unwrap_or_default(),
                        ),
                        muted(match r.worst_case_cost {
                            Some(c) => format!("${c:.2} per unit"),
                            None => String::new(),
                        }),
                    ],
                    pick: Pick::Nothing,
                })
                .collect();
            panes(
                Tab::Workflows,
                None,
                vec![
                    Pane {
                        title: "Instances".into(),
                        columns: vec![
                            col("instance", Kind::Text),
                            col("workflow", Kind::Text),
                            col("target", Kind::Wide),
                            col("status", Kind::Badge),
                            col("started", Kind::Text),
                        ],
                        items: open,
                        empty: "No instance. Press f to start one.",
                    },
                    Pane {
                        title: "Revisions".into(),
                        columns: vec![
                            col("revision", Kind::Text),
                            col("status", Kind::Badge),
                            col("workflows", Kind::Wide),
                            col("worst case", Kind::Number),
                        ],
                        items: revs,
                        empty: "No revision. `pma workflow propose <file>` stores one.",
                    },
                ],
            )
        }
        View::Campaigns(all) => {
            let items = all
                .iter()
                .map(|c| {
                    let taken = c.members.iter().filter(|(_, r)| r.is_some()).count();
                    Item {
                        cells: vec![
                            plain(c.name.clone()),
                            muted(format!("class {}", c.class.name())),
                            cell(
                                format!("{taken}/{}", c.members.len()),
                                match taken == c.members.len() {
                                    true => Tone::Good,
                                    false => Tone::Accent,
                                },
                            ),
                            plain(c.text.clone()),
                        ],
                        pick: Pick::Campaign {
                            name: c.name.clone(),
                        },
                    }
                })
                .collect::<Vec<_>>();
            panes(
                Tab::Campaigns,
                None,
                vec![Pane {
                    title: "Campaigns".into(),
                    columns: vec![
                        col("campaign", Kind::Text),
                        col("class", Kind::Text),
                        col("dispatched", Kind::Number),
                        col("task", Kind::Wide),
                    ],
                    items,
                    empty: "No campaigns. `pma campaign add` defines one.",
                }],
            )
        }
        View::Project {
            name,
            path,
            scanned_at,
            tags,
            verify,
            status,
            placed,
            today,
            runs,
        } => {
            let mut summary: Vec<(String, Cell)> = Vec::new();
            match &status {
                None => summary.push((
                    "state".into(),
                    cell("not found by the last scan", Tone::Warn),
                )),
                Some((states, c)) => {
                    let toned = status_cells(c);
                    for (i, label) in report::STATUS_HEADINGS.iter().enumerate().skip(1) {
                        let mut v = toned[i].clone();
                        // Every state by name; each one's reason follows.
                        if i == 2 {
                            v.text = states
                                .iter()
                                .map(|(st, _)| st.name())
                                .collect::<Vec<_>>()
                                .join(", ");
                        }
                        if !v.text.is_empty() && v.text != "-" {
                            summary.push((label.to_string(), v));
                        }
                    }
                    for (st, why) in states.iter().filter(|(_, why)| !why.is_empty()) {
                        summary.push((
                            st.name().to_string(),
                            cell(why.clone(), state_tone(st.name())),
                        ));
                    }
                }
            }
            summary.push((
                "scanned".into(),
                match scanned_at {
                    None => cell("never", Tone::Warn),
                    Some(t) => cell(
                        report::ago(now - t),
                        // A day's drift is enough for CI or a branch to move.
                        match now - t > 86_400 {
                            true => Tone::Warn,
                            false => Tone::Muted,
                        },
                    ),
                },
            ));
            summary.push((
                "verify".into(),
                match verify {
                    Some(v) => plain(v),
                    None => muted("detected at dispatch"),
                },
            ));
            if !tags.is_empty() {
                summary.push(("tags".into(), plain(tags.join(", "))));
            }
            let tasks = placed
                .iter()
                .map(|p| {
                    let when = report::when(p, today);
                    let priority = p.task.priority.name();
                    Item {
                        cells: vec![
                            plain(match p.task.line {
                                Some(line) => format!("{}:{line}", p.task.project),
                                None => p.task.project.clone(),
                            }),
                            cell(priority, priority_tone(priority)),
                            cell(when.clone(), when_tone(&when)),
                            match p.task.eligible {
                                true => cell("agent", Tone::Accent),
                                false => muted("you"),
                            },
                            plain(p.task.text.clone()),
                        ],
                        pick: task_pick(p),
                    }
                })
                .collect();
            let runs = runs
                .iter()
                .map(|r| Item {
                    cells: vec![
                        muted(format!("#{}", r.id)),
                        cell(r.state.name(), run_tone(r.state.name())),
                        plain(r.text.clone()),
                        muted(r.cost_usd.map_or("-".into(), |c| format!("${c:.2}"))),
                        muted(r.dispatched_at.map_or("-".into(), |t| report::ago(now - t))),
                    ],
                    pick: Pick::Run {
                        id: r.id,
                        clean: r.state == RunState::Ready && accept::review_reasons(r).is_empty(),
                        state: r.state.name().into(),
                        project: r.project.clone(),
                    },
                })
                .collect();
            Screen {
                tab: Some(Tab::Project),
                header: Some(format!("{name}  {}", path.display())),
                summary,
                body: Body::Panes(vec![
                    Pane {
                        title: "Open tasks".into(),
                        columns: vec![
                            col("task", Kind::Text),
                            col("priority", Kind::Badge),
                            col("when", Kind::Text),
                            col("for", Kind::Badge),
                            col("title", Kind::Wide),
                        ],
                        items: tasks,
                        empty: "No open task in its TODO.md.",
                    },
                    Pane {
                        title: "Runs".into(),
                        columns: vec![
                            col("run", Kind::Text),
                            col("state", Kind::Badge),
                            col("task", Kind::Wide),
                            col("cost", Kind::Number),
                            col("started", Kind::Text),
                        ],
                        items: runs,
                        empty: "No agent has worked on it yet.",
                    },
                ]),
            }
        }
        View::Campaign { campaign, runs } => {
            let fields = campaign
                .members
                .iter()
                .map(|(project, run)| {
                    let state = match run.and_then(|id| runs.iter().find(|r| r.id == id)) {
                        None => "not dispatched".to_string(),
                        Some(r) => format!("#{} {}", r.id, r.state.name()),
                    };
                    (project.clone(), state)
                })
                .collect();
            detail(
                None,
                None,
                Detail {
                    title: campaign.name.clone(),
                    badge: None,
                    notice: None,
                    fields,
                    sections: vec![("Task".into(), Section::Prose, campaign.text.clone())],
                },
            )
        }
    }
}

/// Text a prompt starts with: a note being edited starts as its text.
/// The workers, the default one marked: how each program is run.
fn worker_pane(workers: &[Worker], presets: &[Preset], default: &str) -> Pane {
    let items = workers
        .iter()
        .map(|w| {
            let flags = w.flags();
            let used = presets.iter().filter(|p| p.agent == w.name).count();
            Item {
                cells: vec![
                    match w.name == default {
                        true => cell(format!("* {}", w.name), Tone::Accent),
                        false => plain(format!("  {}", w.name)),
                    },
                    plain(w.command.clone()),
                    muted(w.parse.name()),
                    muted(match flags.is_empty() {
                        true => "-".to_string(),
                        false => flags.join(","),
                    }),
                    muted(used.to_string()),
                ],
                pick: Pick::Worker {
                    name: w.name.clone(),
                },
            }
        })
        .collect();
    Pane {
        title: "Workers (* default)".into(),
        columns: vec![
            col("worker", Kind::Text),
            col("command", Kind::Text),
            col("parse", Kind::Text),
            col("flags", Kind::Text),
            col("presets", Kind::Number),
        ],
        items,
        empty: "No worker. `pma agent set <name> command <program>` adds one.",
    }
}

/// The presets, the default one marked: which model each worker runs at.
fn preset_pane(presets: &[Preset], workers: &[Worker], default: Option<&str>) -> Pane {
    let items = presets
        .iter()
        .map(|p| {
            let missing = !workers.iter().any(|w| w.name == p.agent);
            Item {
                cells: vec![
                    match default == Some(p.name.as_str()) {
                        true => cell(format!("* {}", p.name), Tone::Accent),
                        false => plain(format!("  {}", p.name)),
                    },
                    match missing {
                        true => cell(format!("{} (no such worker)", p.agent), Tone::Bad),
                        false => plain(p.agent.clone()),
                    },
                    plain(p.model.clone().unwrap_or_else(|| "its own default".into())),
                    muted(p.args.join(" ")),
                ],
                pick: Pick::Preset {
                    name: p.name.clone(),
                    agent: p.agent.clone(),
                    model: p.model.clone().unwrap_or_default(),
                    args: p.args.clone(),
                },
            }
        })
        .collect();
    Pane {
        title: "Presets (* default)".into(),
        columns: vec![
            col("preset", Kind::Text),
            col("worker", Kind::Text),
            col("model", Kind::Text),
            col("args", Kind::Wide),
        ],
        items,
        empty: "No preset. `+` adds one: <name> <worker> [model] [args...]",
    }
}

pub fn initial_input(act: Act, pick: &Pick) -> String {
    match (act, pick) {
        (Act::EditNote, Pick::Note { text, .. }) => text.clone(),
        (
            Act::EditPreset,
            Pick::Preset {
                agent, model, args, ..
            },
        ) => std::iter::once(agent.as_str())
            .chain((!model.is_empty()).then_some(model.as_str()))
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// The question a confirmation asks.
pub fn question(act: Act, pick: &Pick) -> String {
    let what = match pick {
        Pick::Task { target, .. } => target.clone(),
        Pick::Manual { project } => project.clone(),
        Pick::Run { id, .. } | Pick::Job { id, .. } | Pick::Note { id, .. } => format!("#{id}"),
        Pick::Instance { id, .. } => format!("instance {id}"),
        Pick::Campaign { name } | Pick::Project { name } => name.clone(),
        Pick::Worker { name } => format!("worker {name}"),
        Pick::Preset { name, .. } => format!("preset {name}"),
        Pick::Nothing => String::new(),
    };
    let verb = match act {
        Act::Dispatch => "dispatch",
        Act::RejectRun => "reject",
        Act::Stage => "stage in its clone",
        Act::Commit => "commit in its clone",
        Act::Pr => "open a pull request for",
        Act::Push => "push",
        Act::StopJob | Act::StopWorkflow => "stop",
        Act::RemoveNote => "remove note",
        Act::RemoveConfig => "remove",
        Act::RunCampaign => "run campaign",
        Act::ApproveRuns => "approve every clean run",
        Act::Sync => "sync critical items to GitHub issues",
        _ => "do this to",
    };
    format!("{verb} {what}?").replace(" ?", "?")
}

/// The action `act` takes on `pick`. `pane` is the list the pick is in, for
/// an act over many rows; `input` is what a prompt read.
pub fn action(act: Act, pick: &Pick, pane: &[Item], input: &str) -> Result<Action, String> {
    action_in(act, pick, pane, input, "")
}

/// `action`, on a view of `project`: an act on the project itself, such as
/// rescanning it, needs no row.
pub fn action_in(
    act: Act,
    pick: &Pick,
    pane: &[Item],
    input: &str,
    project: &str,
) -> Result<Action, String> {
    let input = input.trim();
    let worker = WorkerChoice::default();
    let need = |what: &str| Err(format!("select {what} first"));
    Ok(match (act, pick) {
        (Act::Scan, _) => Action::Scan {
            of: Selection::default(),
            offline: false,
            deps: false,
        },
        (Act::ScanProject, _) => {
            if project.is_empty() {
                return Err("open a project first".into());
            }
            Action::Scan {
                of: Selection {
                    projects: vec![project.to_string()],
                    tags: Vec::new(),
                },
                offline: false,
                deps: false,
            }
        }
        (Act::Sync, _) => Action::Sync {
            of: Selection::default(),
            apply: true,
        },
        (Act::AddNote, _) => Action::AddNote { text: input.into() },
        (Act::SettlePrs, _) => Action::SettlePrs,
        (Act::StartWorkflow, _) => {
            let mut words = input.split_whitespace();
            let name = words
                .next()
                .ok_or("type a workflow name, then its target")?
                .to_string();
            let (mut projects, mut set) = (Vec::new(), Vec::new());
            let (mut local, mut unattended, mut model) = (false, false, None);
            for w in words {
                match w {
                    "--local" => local = true,
                    "--unattended" => unattended = true,
                    _ if w.starts_with("--model=") => model = Some(w[8..].to_string()),
                    _ if w.starts_with("--set=") => set.push(w[6..].to_string()),
                    _ if w.starts_with('-') => return Err(format!("unknown flag `{w}`")),
                    _ => projects.push(w.to_string()),
                }
            }
            Action::WorkflowRun {
                name,
                projects,
                tag: None,
                instance: None,
                set,
                approve: None,
                unattended,
                local,
                cap: Vec::new(),
                worker: WorkerChoice {
                    model,
                    ..WorkerChoice::default()
                },
            }
        }
        (Act::ApproveRuns, _) => {
            let ids: Vec<i64> = pane
                .iter()
                .filter_map(|i| match i.pick {
                    Pick::Run {
                        id, clean: true, ..
                    } => Some(id),
                    _ => None,
                })
                .collect();
            if ids.is_empty() {
                return Err("no clean run to approve".into());
            }
            Action::ApproveRuns { ids }
        }
        (Act::Dispatch, Pick::Task { target, retry, .. }) => Action::Dispatch {
            targets: vec![target.clone()],
            tags: Vec::new(),
            auto: false,
            count: None,
            retry: *retry,
            worker,
        },
        (Act::Dispatch, Pick::Manual { .. }) => return Err("not a task for agents".into()),
        (
            Act::Verify,
            Pick::Task { project, .. } | Pick::Manual { project } | Pick::Project { name: project },
        ) => Action::Verify {
            of: Selection {
                projects: vec![project.clone()],
                tags: Vec::new(),
            },
        },
        (Act::Dispatch | Act::Verify, _) => return need("a task"),
        (Act::ApproveRun, Pick::Run { id, state, .. }) => Action::ApproveRun {
            id: *id,
            seen: Some(state.clone()),
        },
        (Act::RejectRun, Pick::Run { id, state, .. }) => Action::RejectRun {
            id: *id,
            keep_git_changes: false,
            infra: false,
            seen: Some(state.clone()),
        },
        (Act::ReworkRun, Pick::Run { id, .. }) => {
            if input.is_empty() {
                return Err("rework needs feedback".into());
            }
            Action::ReworkRun {
                id: *id,
                feedback: input.into(),
            }
        }
        (Act::AddReviewTime, Pick::Run { id, .. }) => Action::AddReviewTime {
            id: *id,
            minutes: input
                .parse()
                .map_err(|_| "type the minutes as a whole number".to_string())?,
        },
        (Act::Stage | Act::Commit | Act::Pr | Act::Push, Pick::Run { id, .. }) => Action::Publish {
            ids: vec![*id],
            all_approved: false,
            target: match act {
                Act::Stage => publish::Target::Stage,
                Act::Commit => publish::Target::Commit,
                Act::Pr => publish::Target::Pr,
                _ => publish::Target::Push,
            },
        },
        (
            Act::ApproveRun
            | Act::RejectRun
            | Act::ReworkRun
            | Act::AddReviewTime
            | Act::Stage
            | Act::Commit
            | Act::Pr
            | Act::Push,
            _,
        ) => return need("a run"),
        (Act::ApproveJob, Pick::Job { id, awaiting: true }) => Action::ApproveJob { id: *id },
        (Act::ApproveJob, Pick::Job { id, .. }) => {
            return Err(format!("job #{id} is not waiting for approval"));
        }
        (Act::StopJob, Pick::Job { id, .. }) => Action::StopJob { id: *id },
        (Act::ApproveJob | Act::StopJob, _) => return need("a job"),
        (Act::StopWorkflow, Pick::Instance { id, .. }) => Action::StopWorkflow { instance: *id },
        (
            Act::ResumeWorkflow,
            Pick::Instance {
                id,
                workflow,
                open: true,
            },
        ) => Action::WorkflowRun {
            name: workflow.clone(),
            projects: Vec::new(),
            tag: None,
            instance: Some(*id),
            set: Vec::new(),
            approve: None,
            unattended: false,
            local: false,
            cap: Vec::new(),
            worker,
        },
        (Act::ResumeWorkflow, Pick::Instance { id, .. }) => {
            return Err(format!("instance {id} is finished"));
        }
        (Act::StopWorkflow | Act::ResumeWorkflow, _) => return need("an instance"),
        (Act::RunCampaign, Pick::Campaign { name }) => Action::RunCampaign {
            name: name.clone(),
            count: None,
        },
        (Act::RunCampaign, _) => return need("a campaign"),
        (Act::EditNote, Pick::Note { id, .. }) => Action::EditNote {
            id: *id,
            text: input.into(),
        },
        (Act::RemoveNote, Pick::Note { id, .. }) => Action::RemoveNote { id: *id },
        (Act::EditNote | Act::RemoveNote, _) => return need("a note"),
        (Act::SetWorkerField, Pick::Worker { name }) => {
            let (field, value) = input
                .split_once(char::is_whitespace)
                .map_or((input, ""), |(f, v)| (f, v.trim()));
            if field.is_empty() {
                return Err(format!(
                    "type a field and its value; fields: {}",
                    crate::worker::FIELDS.map(|(n, _)| n).join(", ")
                ));
            }
            Action::SetWorker {
                name: name.clone(),
                field: field.into(),
                value: value.into(),
            }
        }
        (Act::SetWorkerField, _) => return need("a worker"),
        (Act::EditPreset, Pick::Preset { name, .. }) => {
            preset(std::iter::once(name.as_str()).chain(input.split_whitespace()))?
        }
        (Act::EditPreset, _) => return need("a preset"),
        (Act::AddPreset, _) => preset(input.split_whitespace())?,
        (Act::AddWorker, _) => {
            let mut lines = input.lines().map(str::trim);
            let name = lines.next().unwrap_or_default();
            let command = lines.next().unwrap_or_default();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err("the worker name is one word, such as `codex`".into());
            }
            if command.is_empty() {
                return Err(format!("`{name}` needs a program to run, such as `codex`"));
            }
            Action::AddWorker {
                name: name.into(),
                command: command.into(),
            }
        }
        (Act::MakeDefault, Pick::Worker { name }) => Action::UseWorker { name: name.clone() },
        (Act::MakeDefault, Pick::Preset { name, .. }) => Action::UsePreset { name: name.clone() },
        (Act::RemoveConfig, Pick::Worker { name }) => Action::RemoveWorker { name: name.clone() },
        (Act::RemoveConfig, Pick::Preset { name, .. }) => {
            Action::RemovePreset { name: name.clone() }
        }
        (Act::MakeDefault | Act::RemoveConfig, _) => return need("a worker or a preset"),
    })
}

/// `name worker [model] [args...]`, as `pma preset set` reads it. A model
/// that starts with `-` is an argument instead: the worker's own default.
fn preset<'a>(mut words: impl Iterator<Item = &'a str>) -> Result<Action, String> {
    let usage = "type a name, a worker, then a model and arguments if any";
    let name = words.next().ok_or(usage)?.to_string();
    let agent = words.next().ok_or(usage)?.to_string();
    let mut rest: Vec<String> = words.map(String::from).collect();
    let model = match rest.first() {
        Some(m) if !m.starts_with('-') => Some(rest.remove(0)),
        _ => None,
    };
    Ok(Action::SetPreset(crate::store::Preset {
        name,
        agent,
        model,
        args: rest,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: i64, clean: bool) -> Item {
        Item {
            cells: vec![plain(format!("#{id}"))],
            pick: Pick::Run {
                id,
                clean,
                state: "ready".into(),
                project: "p".into(),
            },
        }
    }

    #[test]
    fn keys_on_rows_build_actions() {
        let task = Pick::Task {
            project: "a".into(),
            target: "a:5".into(),
            retry: true,
        };
        assert_eq!(
            action(Act::Dispatch, &task, &[], ""),
            Ok(Action::Dispatch {
                targets: vec!["a:5".into()],
                tags: vec![],
                auto: false,
                count: None,
                retry: true,
                worker: WorkerChoice::default(),
            })
        );
        let pane = [run(1, true), run(2, false), run(3, true)];
        assert_eq!(
            action(Act::ApproveRuns, &Pick::Nothing, &pane, ""),
            Ok(Action::ApproveRuns { ids: vec![1, 3] })
        );
        assert!(
            action(Act::ApproveRun, &task, &[], "").is_err(),
            "not a run"
        );
        assert!(
            action(
                Act::ApproveJob,
                &Pick::Job {
                    id: 4,
                    awaiting: false
                },
                &[],
                ""
            )
            .unwrap_err()
            .contains("not waiting")
        );
        assert_eq!(
            action(
                Act::AddReviewTime,
                &Pick::Run {
                    id: 2,
                    clean: false,
                    state: "ready".into(),
                    project: "p".into()
                },
                &[],
                " 5 "
            ),
            Ok(Action::AddReviewTime { id: 2, minutes: 5 })
        );
        assert!(
            action(
                Act::ReworkRun,
                &Pick::Run {
                    id: 2,
                    clean: false,
                    state: "ready".into(),
                    project: "p".into()
                },
                &[],
                ""
            )
            .is_err()
        );
    }

    /// A row offers the acts that can apply to it: a task is dispatched,
    /// not approved; a run is approved, not dispatched.
    #[test]
    fn a_row_offers_only_the_acts_that_fit_it() {
        let task = Pick::Task {
            project: "a".into(),
            target: "a:5".into(),
            retry: false,
        };
        let run = Pick::Run {
            id: 1,
            clean: true,
            state: "ready".into(),
            project: "a".into(),
        };
        assert!(fits(Act::Dispatch, &task) && !fits(Act::ApproveRun, &task));
        assert!(fits(Act::ApproveRun, &run) && !fits(Act::Dispatch, &run));
        assert!(
            fits(Act::Scan, &Pick::Nothing),
            "a whole-view act fits anything"
        );
        assert!(!fits(
            Act::ApproveJob,
            &Pick::Job {
                id: 1,
                awaiting: false
            }
        ));
        for act in Act::ALL.into_iter().filter(|a| a.on_row()) {
            assert!(!fits(act, &Pick::Nothing), "{act:?} needs a row");
        }
        assert_eq!(run.project(), Some("a"));
    }

    /// Rescanning a project scans that project alone, and needs one open.
    #[test]
    fn a_project_rescan_scans_only_it() {
        assert_eq!(
            action_in(Act::ScanProject, &Pick::Nothing, &[], "", "margo"),
            Ok(Action::Scan {
                of: Selection {
                    projects: vec!["margo".into()],
                    tags: vec![],
                },
                offline: false,
                deps: false,
            })
        );
        assert!(action(Act::ScanProject, &Pick::Nothing, &[], "").is_err());
    }

    #[test]
    fn a_typed_workflow_start_reads_its_flags() {
        let Ok(Action::WorkflowRun {
            name,
            projects,
            local,
            unattended,
            worker,
            set,
            ..
        }) = action(
            Act::StartWorkflow,
            &Pick::Nothing,
            &[],
            "review-fix argdec --local --unattended --model=haiku --set=breadth=5",
        )
        else {
            panic!("a workflow run");
        };
        assert_eq!(
            (name.as_str(), projects),
            ("review-fix", vec!["argdec".to_string()])
        );
        assert!(local && unattended);
        assert_eq!(worker.model.as_deref(), Some("haiku"));
        assert_eq!(set, ["breadth=5"]);
        assert!(action(Act::StartWorkflow, &Pick::Nothing, &[], "w a --nope").is_err());
    }

    #[test]
    fn prompts_and_questions_name_their_subject() {
        let note = Pick::Note {
            id: 3,
            text: "buy milk".into(),
        };
        assert_eq!(initial_input(Act::EditNote, &note), "buy milk");
        assert_eq!(question(Act::RemoveNote, &note), "remove note #3?");
        assert_eq!(
            question(Act::ApproveRuns, &Pick::Nothing),
            "approve every clean run?"
        );
    }

    fn job(id: i64, args: &[&str], state: &str, now: i64) -> Job {
        Job {
            id,
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: "/".into(),
            state: state.into(),
            created_at: now,
            started_at: None,
            finished_at: None,
            pid: None,
            exit_code: None,
            awaiting: None,
        }
    }

    /// A hub with one project: a task being dispatched again by a job while
    /// its last run waits on you and a push job publishes it; a finished run
    /// of a closed task; and a scan of every project.
    fn hub_fixture() -> (Vec<Node>, Vec<i64>) {
        use crate::rank::{Placed, Quadrant, Task};
        use crate::scan::Ci;
        use crate::store::ProjectRow;
        use crate::todo::Priority;
        let now = dates::now();
        let row = ProjectRow {
            name: "argdec".into(),
            path: "/src/argdec".into(),
            slug: None,
            absent_since: None,
            tier: Some(2),
            scanned_at: Some(now - 3 * 86_400),
            has_todo: true,
            lint_errors: 0,
            dirty: 0,
            leftover: 0,
            ahead: None,
            last_activity: None,
            ci: Ci::Failing(vec!["test".into()]),
            ci_runs: Vec::new(),
            faults: Vec::new(),
            deps: None,
            deps_detail: String::new(),
            deps_at: None,
            held_back: None,
            lock_at: None,
        };
        let project = rank::Project {
            name: "argdec".into(),
            tier: 2,
            open: vec![Priority::High],
            idle_days: None,
            ci: row.ci.clone(),
            dirty: 0,
            leftover: 0,
            ahead: None,
            deps: None,
        };
        let task = |line: i64, text: &str| Placed {
            task: Task {
                project: "argdec".into(),
                tier: 2,
                priority: Priority::High,
                text: text.into(),
                line: Some(line),
                key: Some(text.into()),
                group: None,
                due: None,
                tagged_urgent: false,
                signal_urgent: false,
                age_days: 3,
                eligible: true,
            },
            importance: 0.4,
            urgency: None,
            quadrant: Quadrant::Q2,
        };
        let placed = vec![task(7, "split the parser"), task(9, "quiet task")];
        let runs = vec![
            Run {
                id: 5,
                project: "argdec".into(),
                task_key: "split the parser".into(),
                text: "split the parser".into(),
                state: RunState::Approved,
                ..Run::default()
            },
            Run {
                id: 2,
                project: "argdec".into(),
                task_key: "an old item".into(),
                text: "an old item".into(),
                state: RunState::Merged,
                ..Run::default()
            },
        ];
        let jobs = vec![
            job(12, &["scan"], "running", now),
            job(11, &["push", "5"], "queued", now),
            job(
                10,
                &["dispatch", "argdec:7", "-a", "claude"],
                "running",
                now,
            ),
            job(8, &["dispatch", "argdec:9"], "done", now),
        ];
        let lists = next::build(&placed, &runs, 0, |_| next::Taken::Held);
        let portfolio = api::Portfolio {
            cfg: crate::config::Config::default(),
            today: 0,
            projects: vec![(project, row)],
            tasks: placed.iter().map(|p| p.task.clone()).collect(),
            untiered: 0,
            ranked_as: None,
            scanned_ago: 0,
        };
        let (tree, loose) = hub_tree(&portfolio, &placed, &lists, &runs, &jobs, 0, now);
        (tree, loose.iter().map(|j| j.id).collect())
    }

    /// Projects are the roots and tasks their branches; under a task is what
    /// works on it: the job dispatching it, and its run with the job
    /// publishing that. A finished job is told by its run, not shown, and a
    /// job on every project is left for the header.
    #[test]
    fn the_hub_nests_work_under_its_task() {
        let (tree, loose) = hub_fixture();
        let rows = visible(&tree, &Default::default());
        let lines: Vec<String> = rows
            .iter()
            .map(|v| format!("{}{}", "  ".repeat(v.depth), v.node.label.text))
            .collect();
        assert_eq!(
            lines,
            [
                "argdec",
                "  split the parser",
                "    job #10",
                "    run #5",
                "      job #11",
                "  quiet task",
                "  finished runs",
            ],
            "{lines:#?}"
        );
        assert_eq!(loose, [12]);
        let cells = |i: usize| -> Vec<String> {
            rows[i].node.cells.iter().map(|c| c.text.clone()).collect()
        };
        assert_eq!(cells(0)[0], "CI failing, 1 need you, 2 jobs", "worst first");
        assert_eq!(rows[0].node.cells[0].tone, Tone::Bad);
        assert!(
            cells(0)[3].starts_with("scanned ") && rows[0].node.cells[3].tone == Tone::Warn,
            "a three-day-old scan says so: {:?}",
            cells(0)
        );
        let facts: Vec<&str> = rows[0].node.facts.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(facts, ["T2", "2 tasks"]);
        assert_eq!(
            cells(1)[..5],
            ["job #10 running", "Q2", "high", "open 3d", "agent"],
            "what is happening comes first"
        );
        assert_eq!(cells(5)[0], "", "nothing happens to the quiet task");
        for v in &rows {
            assert_eq!(
                v.node.cells.len(),
                HUB_COLUMNS.len(),
                "{}",
                v.node.label.text
            );
        }
        assert_eq!(
            rows[1].node.pick,
            Pick::Task {
                project: "argdec".into(),
                target: "argdec:7".into(),
                retry: false,
            }
        );
        let mut ids: Vec<&str> = rows.iter().map(|v| v.node.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), rows.len(), "every id is unique");
    }

    /// What the viewer opens or closes holds over the node's default; a
    /// leaf is never open.
    #[test]
    fn a_viewer_opens_and_closes_branches() {
        let (tree, _) = hub_fixture();
        let mut chosen = std::collections::HashMap::new();
        chosen.insert("pargdec/targdec:7".to_string(), false);
        chosen.insert("pargdec/done".to_string(), true);
        chosen.insert("pargdec/targdec:9".to_string(), true);
        let rows = visible(&tree, &chosen);
        let labels: Vec<&str> = rows.iter().map(|v| v.node.label.text.as_str()).collect();
        assert_eq!(
            labels,
            [
                "argdec",
                "split the parser",
                "quiet task",
                "finished runs",
                "run #2"
            ]
        );
        assert!(
            !rows[2].open,
            "a task with nothing under it has nothing to open"
        );
        assert_eq!(rows[4].parent, Some(3));
        chosen.insert("pargdec".to_string(), false);
        assert_eq!(visible(&tree, &chosen).len(), 1);
    }

    /// A job's command line says what it works on.
    #[test]
    fn a_job_names_what_it_works_on() {
        let j = |args: &[&str]| works_on(&job(1, args, "running", 0));
        assert_eq!(
            j(&["dispatch", "a:7", "b:ci", "-a", "claude"]),
            [Works::Task("a:7".into()), Works::Task("b:ci".into())]
        );
        assert_eq!(j(&["push", "5"]), [Works::Run(5)]);
        assert_eq!(j(&["scan", "portal"]), [Works::Project("portal".into())]);
        assert!(j(&["scan"]).is_empty());
        assert_eq!(
            j(&["workflow", "run", "review", "portal"]),
            [Works::Project("portal".into())]
        );
        assert!(j(&["note", "add", "x"]).is_empty());
    }

    /// On the hub `a` approves a run or a job's plan, and `s` stages a run or
    /// stops a job: the selected row decides. No row fits both acts of one
    /// key, so the choice is never a guess.
    #[test]
    fn a_key_with_two_acts_follows_the_row() {
        let run = Pick::Run {
            id: 1,
            clean: false,
            state: "ready".into(),
            project: "p".into(),
        };
        let job = Pick::Job {
            id: 2,
            awaiting: true,
        };
        let act = |key: &str, pick: &Pick| match lookup(key, Tab::Hub, pick).map(|b| b.command) {
            Some(crate::keys::Command::Act(a)) => Some(a),
            _ => None,
        };
        assert_eq!(act("a", &run), Some(Act::ApproveRun));
        assert_eq!(act("a", &job), Some(Act::ApproveJob));
        assert_eq!(act("s", &run), Some(Act::Stage));
        assert_eq!(act("s", &job), Some(Act::StopJob));
        assert_eq!(
            act("c", &job),
            Some(Act::Scan),
            "keys bound everywhere still work"
        );

        let picks = [
            Pick::Nothing,
            Pick::Task {
                project: "p".into(),
                target: "p:1".into(),
                retry: false,
            },
            Pick::Manual {
                project: "p".into(),
            },
            run,
            job,
            Pick::Instance {
                id: 1,
                workflow: "w".into(),
                open: true,
            },
            Pick::Note {
                id: 1,
                text: "n".into(),
            },
            Pick::Campaign { name: "c".into() },
            Pick::Project { name: "p".into() },
        ];
        for tab in Tab::ALL {
            let on = crate::keys::on_tab(tab);
            for (i, a) in on.iter().enumerate() {
                for b in on.iter().skip(i + 1).filter(|b| b.key == a.key) {
                    let (crate::keys::Command::Act(x), crate::keys::Command::Act(y)) =
                        (a.command, b.command)
                    else {
                        panic!("`{}` means two things on {tab:?}", a.key);
                    };
                    for p in &picks {
                        assert!(
                            !(fits(x, p) && fits(y, p)),
                            "`{}` on {tab:?} fits {p:?} as {x:?} and {y:?}",
                            a.key
                        );
                    }
                }
            }
        }
    }
}
