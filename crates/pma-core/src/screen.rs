//! A `View` as an interactive front end shows it: tables of rows to select,
//! or a detail of fields and sections; and how a key on a selected row
//! becomes an `Action`. The TUI and the web both draw a `Screen` and send what
//! `action` builds, so neither decides for itself what a row is, what a key
//! does to it, or what a state means. Each cell carries a `Tone`, so both
//! colour a failed run or a critical task alike.

use crate::api::{Action, Selection, View, WorkerChoice};
use crate::keys::{Act, Tab};
use crate::store::{Run, RunState};
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
        Act::Verify => matches!(pick, Pick::Task { .. } | Pick::Manual { .. }),
        Act::ApproveRun
        | Act::RejectRun
        | Act::ReworkRun
        | Act::AddReviewTime
        | Act::Pr
        | Act::Push => matches!(pick, Pick::Run { .. }),
        Act::ApproveJob => matches!(pick, Pick::Job { awaiting: true, .. }),
        Act::StopJob => matches!(pick, Pick::Job { .. }),
        Act::StopWorkflow => matches!(pick, Pick::Instance { open: true, .. }),
        Act::ResumeWorkflow => matches!(pick, Pick::Instance { open: true, .. }),
        Act::RunCampaign => matches!(pick, Pick::Campaign { .. }),
        Act::EditNote | Act::RemoveNote => matches!(pick, Pick::Note { .. }),
        _ => !act.on_row(),
    }
}

impl Pick {
    /// The project a row is about, when it names one.
    pub fn project(&self) -> Option<&str> {
        match self {
            Pick::Task { project, .. } | Pick::Manual { project } | Pick::Run { project, .. } => {
                Some(project)
            }
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
        "failing CI" | "broken" | "critical" => Tone::Bad,
        "unpublished" | "stale deps" => Tone::Warn,
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
    Pane {
        title: "Recent scans".into(),
        columns: vec![
            col("scan", Kind::Text),
            col("when", Kind::Text),
            col("kind", Kind::Text),
            col("projects", Kind::Number),
            col("failing", Kind::Number),
            col("broken", Kind::Number),
            col("open", Kind::Number),
            col("opened", Kind::Number),
            col("closed", Kind::Number),
            col("other", Kind::Number),
        ],
        items: scans
            .into_iter()
            .map(|s| Item {
                cells: vec![
                    muted(format!("#{}", s.id)),
                    plain(report::ago(now - s.at)),
                    muted(if s.full { "full" } else { "partial" }),
                    plain(s.projects.to_string()),
                    count(s.failing, Tone::Bad),
                    count(s.broken, Tone::Warn),
                    plain(s.open.to_string()),
                    count(s.opened, Tone::Accent),
                    count(s.closed, Tone::Good),
                    count(s.other, Tone::Plain),
                ],
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
                    tiered: row.tier.is_some() || p.cfg.default_tier.is_some(),
                    has_todo: row.has_todo,
                    lint_errors: row.lint_errors,
                    scan_error: row.scan_error.as_deref(),
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
                ],
            )
        }
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
pub fn initial_input(act: Act, pick: &Pick) -> String {
    match (act, pick) {
        (Act::EditNote, Pick::Note { text, .. }) => text.clone(),
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
        Pick::Campaign { name } => name.clone(),
        Pick::Nothing => String::new(),
    };
    let verb = match act {
        Act::Dispatch => "dispatch",
        Act::RejectRun => "reject",
        Act::Pr => "open a pull request for",
        Act::Push => "push",
        Act::StopJob | Act::StopWorkflow => "stop",
        Act::RemoveNote => "remove note",
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
        (Act::Verify, Pick::Task { project, .. } | Pick::Manual { project }) => Action::Verify {
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
        (Act::Pr | Act::Push, Pick::Run { id, .. }) => Action::Publish {
            ids: vec![*id],
            all_approved: false,
            target: match act {
                Act::Pr => publish::Target::Pr,
                _ => publish::Target::Push,
            },
        },
        (
            Act::ApproveRun
            | Act::RejectRun
            | Act::ReworkRun
            | Act::AddReviewTime
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
    })
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
}
