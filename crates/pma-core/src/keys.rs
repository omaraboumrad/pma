//! The one key table every front end reads: which key opens which tab, moves
//! the selection, or does which action. Keys are DOM `KeyboardEvent.key`
//! names (`j`, `Enter`, `Escape`, `ArrowDown`), so the web uses the table as
//! it is and the TUI maps its key codes to the same names. Plain keys only:
//! a browser or screen reader reserves most combinations with a modifier.

use crate::api::{Action, Query, Selection};
use crate::report_runs::By;

/// A screen: one query, rendered as a list or as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Next,
    Matrix,
    Runs,
    Jobs,
    Agents,
    Workflows,
    Campaigns,
    Report,
    Notes,
    Status,
    /// One project, opened from a row that names it. It has no key of its
    /// own: which project is the front end's to remember.
    Project,
}

impl Tab {
    pub const ALL: [Tab; 11] = [
        Tab::Next,
        Tab::Matrix,
        Tab::Runs,
        Tab::Jobs,
        Tab::Agents,
        Tab::Workflows,
        Tab::Campaigns,
        Tab::Report,
        Tab::Notes,
        Tab::Status,
        Tab::Project,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Next => "next",
            Tab::Matrix => "matrix",
            Tab::Runs => "runs",
            Tab::Jobs => "jobs",
            Tab::Agents => "agents",
            Tab::Workflows => "workflows",
            Tab::Campaigns => "campaigns",
            Tab::Report => "report",
            Tab::Notes => "notes",
            Tab::Status => "status",
            Tab::Project => "project",
        }
    }

    /// What the tab shows. Lists are not cut short: a front end scrolls.
    /// `project` names the project the project tab shows.
    pub fn query(self, by: By, project: &str) -> Query {
        let of = Selection::default();
        match self {
            Tab::Next => Query::Next { of, all: true },
            Tab::Matrix => Query::Matrix {
                of,
                quadrant: None,
                all: true,
            },
            Tab::Runs => Query::Runs,
            Tab::Jobs => Query::Jobs,
            Tab::Agents => Query::Agents,
            Tab::Workflows => Query::Workflows,
            Tab::Campaigns => Query::Campaigns,
            Tab::Report => Query::Report { by },
            Tab::Notes => Query::Notes,
            // Every project, untiered ones too: it is the list a project is
            // opened from.
            Tab::Status => Query::Status {
                of,
                explain: false,
                all: true,
            },
            Tab::Project => Query::Project {
                name: project.to_string(),
            },
        }
    }
}

/// An action by kind, without its arguments: what a key names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    ApproveRun,
    ApproveRuns,
    RejectRun,
    AddReviewTime,
    SettlePrs,
    StopJob,
    ApproveJob,
    StopWorkflow,
    AddNote,
    EditNote,
    RemoveNote,
    Scan,
    /// Scans the project the project view shows, and nothing else.
    ScanProject,
    Dispatch,
    /// Resumes the selected instance.
    ResumeWorkflow,
    /// Starts an instance from a typed `<workflow> <target> [flags]`.
    StartWorkflow,
    ReworkRun,
    Stage,
    Commit,
    Pr,
    Push,
    RunCampaign,
    Sync,
    Verify,
    /// Sets one field of the selected worker from a typed `field value`.
    SetWorkerField,
    /// Replaces the selected preset from a typed `worker [model] [args...]`.
    EditPreset,
    /// Names a preset from a typed `name worker [model] [args...]`.
    AddPreset,
    /// Creates a worker from a typed `name program`.
    AddWorker,
    /// Makes the selected worker or preset the default.
    MakeDefault,
    /// Removes the selected worker or preset.
    RemoveConfig,
}

impl Act {
    pub const ALL: [Act; 30] = [
        Act::ApproveRun,
        Act::ApproveRuns,
        Act::RejectRun,
        Act::AddReviewTime,
        Act::SettlePrs,
        Act::StopJob,
        Act::ApproveJob,
        Act::StopWorkflow,
        Act::AddNote,
        Act::EditNote,
        Act::RemoveNote,
        Act::Scan,
        Act::ScanProject,
        Act::Dispatch,
        Act::ResumeWorkflow,
        Act::StartWorkflow,
        Act::ReworkRun,
        Act::Stage,
        Act::Commit,
        Act::Pr,
        Act::Push,
        Act::RunCampaign,
        Act::Sync,
        Act::Verify,
        Act::SetWorkerField,
        Act::EditPreset,
        Act::AddPreset,
        Act::AddWorker,
        Act::MakeDefault,
        Act::RemoveConfig,
    ];

    pub fn name(self) -> String {
        format!("{self:?}")
    }

    /// Whether the act needs a selected row; one that does not acts on the
    /// whole tab or on nothing in particular.
    pub fn on_row(self) -> bool {
        !matches!(
            self,
            Act::Scan
                | Act::ScanProject
                | Act::Sync
                | Act::AddNote
                | Act::SettlePrs
                | Act::ApproveRuns
                | Act::StartWorkflow
                | Act::AddPreset
                | Act::AddWorker
        )
    }

    /// How much an act weighs: `primary` moves work forward, `danger`
    /// discards or publishes something that cannot be called back.
    pub fn weight(self) -> &'static str {
        match self {
            Act::ApproveRun | Act::ApproveJob | Act::Dispatch => "primary",
            Act::RejectRun
            | Act::StopJob
            | Act::StopWorkflow
            | Act::RemoveNote
            | Act::RemoveConfig
            | Act::Push => "danger",
            _ => "default",
        }
    }

    /// The kinds an action can be. A new `Action` variant fails to compile
    /// here until it has a kind, and the tests then require it a key.
    pub fn of(a: &Action) -> &'static [Act] {
        match a {
            Action::ApproveRun { .. } => &[Act::ApproveRun],
            Action::ApproveRuns { .. } => &[Act::ApproveRuns],
            Action::RejectRun { .. } => &[Act::RejectRun],
            Action::AddReviewTime { .. } => &[Act::AddReviewTime],
            Action::SettlePrs => &[Act::SettlePrs],
            Action::StopJob { .. } => &[Act::StopJob],
            Action::ApproveJob { .. } => &[Act::ApproveJob],
            Action::StopWorkflow { .. } => &[Act::StopWorkflow],
            Action::AddNote { .. } => &[Act::AddNote],
            Action::EditNote { .. } => &[Act::EditNote],
            Action::RemoveNote { .. } => &[Act::RemoveNote],
            Action::Scan { .. } => &[Act::Scan, Act::ScanProject],
            Action::Dispatch { .. } => &[Act::Dispatch],
            Action::WorkflowRun { .. } => &[Act::ResumeWorkflow, Act::StartWorkflow],
            Action::ReworkRun { .. } => &[Act::ReworkRun],
            Action::Publish { .. } => &[Act::Stage, Act::Commit, Act::Pr, Act::Push],
            Action::RunCampaign { .. } => &[Act::RunCampaign],
            Action::Sync { .. } => &[Act::Sync],
            Action::Verify { .. } => &[Act::Verify],
            Action::SetWorker { .. } => &[Act::SetWorkerField],
            Action::AddWorker { .. } => &[Act::AddWorker],
            Action::SetPreset(_) => &[Act::EditPreset, Act::AddPreset],
            Action::UseWorker { .. } | Action::UsePreset { .. } => &[Act::MakeDefault],
            Action::RemoveWorker { .. } | Action::RemovePreset { .. } => &[Act::RemoveConfig],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Tab(Tab),
    Act(Act),
    Down,
    Up,
    First,
    Last,
    /// The next list on a tab with several.
    NextPane,
    PrevPane,
    /// The selected item in full: a run's diff, a job's log.
    Open,
    /// Closes what `Open` or a prompt opened.
    Back,
    /// The report's next dimension.
    ReportBy,
    /// The project of the selected row.
    OpenProject,
    Help,
    /// The TUI only; a browser tab is closed by the browser.
    Quit,
}

/// What a front end asks before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Nothing,
    /// Yes or no: the action cannot be undone from here.
    Confirm,
    /// A line of text, with this label.
    Text(&'static str),
    /// A line for each label, asked in turn. The answers reach `action` one
    /// per line.
    Fields(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: &'static str,
    pub command: Command,
    /// Where the key applies; empty is every tab.
    pub tabs: &'static [Tab],
    pub ask: Ask,
    pub label: &'static str,
}

const fn bind(key: &'static str, command: Command, label: &'static str) -> Binding {
    Binding {
        key,
        command,
        tabs: &[],
        ask: Ask::Nothing,
        label,
    }
}

const fn on(
    key: &'static str,
    tabs: &'static [Tab],
    act: Act,
    ask: Ask,
    label: &'static str,
) -> Binding {
    Binding {
        key,
        command: Command::Act(act),
        tabs,
        ask,
        label,
    }
}

const TASKS: &[Tab] = &[Tab::Next, Tab::Matrix, Tab::Project];
const PANES: &[Tab] = &[
    Tab::Next,
    Tab::Matrix,
    Tab::Agents,
    Tab::Workflows,
    Tab::Project,
];
const RUNS: &[Tab] = &[Tab::Next, Tab::Runs, Tab::Project];

pub const BINDINGS: &[Binding] = &[
    bind("1", Command::Tab(Tab::Next), "next"),
    bind("2", Command::Tab(Tab::Matrix), "matrix"),
    bind("3", Command::Tab(Tab::Runs), "runs"),
    bind("4", Command::Tab(Tab::Jobs), "jobs"),
    bind("5", Command::Tab(Tab::Agents), "agents"),
    bind("6", Command::Tab(Tab::Workflows), "workflows"),
    bind("7", Command::Tab(Tab::Campaigns), "campaigns"),
    bind("8", Command::Tab(Tab::Report), "report"),
    bind("9", Command::Tab(Tab::Notes), "notes"),
    bind("0", Command::Tab(Tab::Status), "status"),
    bind("j", Command::Down, "down"),
    bind("ArrowDown", Command::Down, "down"),
    bind("k", Command::Up, "up"),
    bind("ArrowUp", Command::Up, "up"),
    bind("g", Command::First, "first"),
    bind("G", Command::Last, "last"),
    Binding {
        tabs: PANES,
        ..bind("Tab", Command::NextPane, "next list")
    },
    Binding {
        tabs: PANES,
        ..bind("l", Command::NextPane, "next list")
    },
    Binding {
        tabs: PANES,
        ..bind("h", Command::PrevPane, "previous list")
    },
    Binding {
        tabs: &[Tab::Next, Tab::Runs, Tab::Jobs, Tab::Agents, Tab::Project],
        ..bind("Enter", Command::Open, "open")
    },
    Binding {
        tabs: &[Tab::Next, Tab::Matrix, Tab::Runs, Tab::Agents, Tab::Status],
        ..bind("o", Command::OpenProject, "open project")
    },
    bind("Escape", Command::Back, "back"),
    Binding {
        tabs: &[Tab::Report],
        ..bind("b", Command::ReportBy, "by")
    },
    bind("?", Command::Help, "keys"),
    bind("q", Command::Quit, "quit"),
    // Anywhere.
    on("c", &[], Act::Scan, Ask::Nothing, "scan"),
    on("i", &[], Act::Sync, Ask::Confirm, "sync issues"),
    on("n", &[], Act::AddNote, Ask::Text("note"), "add note"),
    on(
        "r",
        &[Tab::Project],
        Act::ScanProject,
        Ask::Nothing,
        "rescan project",
    ),
    // A task.
    on("d", TASKS, Act::Dispatch, Ask::Confirm, "dispatch"),
    on("v", TASKS, Act::Verify, Ask::Nothing, "verify project"),
    // A run.
    on("a", RUNS, Act::ApproveRun, Ask::Nothing, "approve"),
    on("b", RUNS, Act::ApproveRuns, Ask::Confirm, "approve clean"),
    on("x", RUNS, Act::RejectRun, Ask::Confirm, "reject"),
    on("w", RUNS, Act::ReworkRun, Ask::Text("feedback"), "rework"),
    on(
        "t",
        RUNS,
        Act::AddReviewTime,
        Ask::Text("minutes"),
        "review time",
    ),
    on("s", RUNS, Act::Stage, Ask::Confirm, "stage"),
    on("m", RUNS, Act::Commit, Ask::Confirm, "commit"),
    on("p", RUNS, Act::Pr, Ask::Confirm, "pull request"),
    on("u", RUNS, Act::Push, Ask::Confirm, "push"),
    on("y", RUNS, Act::SettlePrs, Ask::Nothing, "check PRs"),
    // A job.
    on(
        "a",
        &[Tab::Jobs],
        Act::ApproveJob,
        Ask::Nothing,
        "approve plan",
    ),
    on("s", &[Tab::Jobs], Act::StopJob, Ask::Confirm, "stop"),
    // A workflow instance.
    on(
        "r",
        &[Tab::Workflows],
        Act::ResumeWorkflow,
        Ask::Nothing,
        "resume",
    ),
    on(
        "f",
        &[Tab::Workflows],
        Act::StartWorkflow,
        Ask::Text("workflow target [--local] [--unattended]"),
        "start",
    ),
    on(
        "s",
        &[Tab::Workflows],
        Act::StopWorkflow,
        Ask::Confirm,
        "stop",
    ),
    // A campaign.
    on(
        "r",
        &[Tab::Campaigns],
        Act::RunCampaign,
        Ask::Confirm,
        "run",
    ),
    // A worker or a preset.
    on(
        "f",
        &[Tab::Agents],
        Act::SetWorkerField,
        Ask::Text("field value"),
        "set field",
    ),
    on(
        "e",
        &[Tab::Agents],
        Act::EditPreset,
        Ask::Text("worker [model] [args...]"),
        "edit preset",
    ),
    on(
        "+",
        &[Tab::Agents],
        Act::AddPreset,
        Ask::Text("name worker [model] [args...]"),
        "add preset",
    ),
    on(
        "a",
        &[Tab::Agents],
        Act::AddWorker,
        Ask::Fields(&["worker name", "program to run"]),
        "add worker",
    ),
    on(
        "u",
        &[Tab::Agents],
        Act::MakeDefault,
        Ask::Nothing,
        "make default",
    ),
    on(
        "x",
        &[Tab::Agents],
        Act::RemoveConfig,
        Ask::Confirm,
        "remove",
    ),
    // A note.
    on("e", &[Tab::Notes], Act::EditNote, Ask::Text("note"), "edit"),
    on("x", &[Tab::Notes], Act::RemoveNote, Ask::Confirm, "remove"),
];

/// The binding `key` has on `tab`. One applying to that tab alone wins over
/// one applying everywhere.
pub fn lookup(key: &str, tab: Tab) -> Option<&'static Binding> {
    let fits = |b: &&Binding| b.key == key;
    BINDINGS
        .iter()
        .filter(fits)
        .find(|b| b.tabs.contains(&tab))
        .or_else(|| BINDINGS.iter().filter(fits).find(|b| b.tabs.is_empty()))
}

/// The bindings that apply on `tab`, for a help screen.
pub fn on_tab(tab: Tab) -> Vec<&'static Binding> {
    BINDINGS
        .iter()
        .filter(|b| b.tabs.is_empty() || b.tabs.contains(&tab))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action a front end may take has a key, so no front end can do
    /// what the other has no key for.
    #[test]
    fn every_act_has_a_key() {
        for act in Act::ALL {
            assert!(
                BINDINGS.iter().any(|b| b.command == Command::Act(act)),
                "{act:?} has no key"
            );
        }
        for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::Project) {
            assert!(
                BINDINGS.iter().any(|b| b.command == Command::Tab(tab)),
                "{tab:?} has no key"
            );
        }
        assert!(
            BINDINGS.iter().any(|b| b.command == Command::OpenProject),
            "a project opens from a row"
        );
    }

    /// One key means one thing on a tab: two bindings for the same tab, or a
    /// tab binding that hides an everywhere binding, would make it ambiguous.
    #[test]
    fn no_key_means_two_things_on_one_tab() {
        for tab in Tab::ALL {
            let keys: Vec<&str> = on_tab(tab).iter().map(|b| b.key).collect();
            for k in &keys {
                assert_eq!(
                    keys.iter().filter(|x| *x == k).count(),
                    1,
                    "`{k}` is bound twice on {tab:?}"
                );
            }
        }
    }

    #[test]
    fn keys_are_plain() {
        for b in BINDINGS {
            assert!(
                b.key.chars().count() == 1 || b.key.chars().all(|c| c.is_ascii_alphabetic()),
                "{}",
                b.key
            );
        }
    }

    #[test]
    fn lookup_prefers_the_tab() {
        assert_eq!(
            lookup("a", Tab::Jobs).map(|b| b.command),
            Some(Command::Act(Act::ApproveJob))
        );
        assert_eq!(
            lookup("a", Tab::Runs).map(|b| b.command),
            Some(Command::Act(Act::ApproveRun))
        );
        assert!(lookup("a", Tab::Next).is_none());
        assert_eq!(
            lookup("c", Tab::Notes).map(|b| b.command),
            Some(Command::Act(Act::Scan))
        );
    }
}
