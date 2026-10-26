//! The one key table every front end reads: which key opens which tab, moves
//! the selection, or does which action. Keys are DOM `KeyboardEvent.key`
//! names (`j`, `Enter`, `Escape`, `ArrowDown`), so the web uses the table as
//! it is and the TUI maps its key codes to the same names. Plain keys only:
//! a browser or screen reader reserves most combinations with a modifier.

use crate::api::{Action, Query, Selection};
use crate::report_runs::By;
use crate::screen::{self, Pick};

/// A screen: one query, rendered as a list or as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    /// What waits on you, what is under way, and what agents can take.
    Inbox,
    /// Every project, worst first.
    Projects,
    /// Jobs, running agents and workflow instances, newest first.
    Activity,
    Matrix,
    /// Workflow revisions and campaigns: what can be run.
    Library,
    Report,
    Notes,
    /// One project, opened from a row that names it. It has no key of its
    /// own: which project is the front end's to remember.
    Project,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Inbox,
        Tab::Projects,
        Tab::Activity,
        Tab::Matrix,
        Tab::Library,
        Tab::Report,
        Tab::Notes,
        Tab::Project,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Inbox => "inbox",
            Tab::Projects => "projects",
            Tab::Activity => "activity",
            Tab::Matrix => "matrix",
            Tab::Library => "library",
            Tab::Report => "report",
            Tab::Notes => "notes",
            Tab::Project => "project",
        }
    }

    /// What the tab shows. Lists are not cut short: a front end scrolls.
    /// `project` names the project the project tab shows.
    pub fn query(self, by: By, project: &str) -> Query {
        let of = Selection::default();
        match self {
            Tab::Inbox => Query::Inbox { of },
            // Every project, untiered ones too: it is the list a project is
            // opened from.
            Tab::Projects => Query::Status {
                of,
                explain: false,
                all: true,
            },
            Tab::Activity => Query::Activity,
            Tab::Matrix => Query::Matrix {
                of,
                quadrant: None,
                all: true,
            },
            Tab::Library => Query::Library,
            Tab::Report => Query::Report { by },
            Tab::Notes => Query::Notes,
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
    /// `pma run <name>` over the project in view or the selected row's,
    /// from a typed `<name> [--review] [--worktree] [--push]`.
    Run,
}

impl Act {
    pub const ALL: [Act; 25] = [
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
        Act::Run,
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
                | Act::Run
        )
    }

    /// How much an act weighs: `primary` moves work forward, `danger`
    /// discards or publishes something that cannot be called back.
    pub fn weight(self) -> &'static str {
        match self {
            Act::ApproveRun | Act::ApproveJob | Act::Dispatch => "primary",
            Act::RejectRun | Act::StopJob | Act::StopWorkflow | Act::RemoveNote | Act::Push => {
                "danger"
            }
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
            Action::Run { .. } => &[Act::Run],
            Action::Publish { .. } => &[Act::Stage, Act::Commit, Act::Pr, Act::Push],
            Action::RunCampaign { .. } => &[Act::RunCampaign],
            Action::Sync { .. } => &[Act::Sync],
            Action::Verify { .. } => &[Act::Verify],
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
    /// The selected item in full: a run's diff, a job's log; a task's
    /// project.
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
    /// One of a list `api::choices` gives, plus its flags, with this label.
    Choose(&'static str),
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

const TASKS: &[Tab] = &[Tab::Inbox, Tab::Matrix, Tab::Project];
const PANES: &[Tab] = &[
    Tab::Inbox,
    Tab::Activity,
    Tab::Matrix,
    Tab::Library,
    Tab::Project,
];
const RUNS: &[Tab] = &[Tab::Inbox, Tab::Project];
const JOBS: &[Tab] = &[Tab::Inbox, Tab::Activity, Tab::Project];

pub const BINDINGS: &[Binding] = &[
    bind("1", Command::Tab(Tab::Inbox), "inbox"),
    bind("2", Command::Tab(Tab::Projects), "projects"),
    bind("3", Command::Tab(Tab::Activity), "activity"),
    bind("4", Command::Tab(Tab::Matrix), "matrix"),
    bind("5", Command::Tab(Tab::Library), "library"),
    bind("6", Command::Tab(Tab::Report), "report"),
    bind("7", Command::Tab(Tab::Notes), "notes"),
    bind("j", Command::Down, "down"),
    bind("ArrowDown", Command::Down, "down"),
    bind("k", Command::Up, "up"),
    bind("ArrowUp", Command::Up, "up"),
    bind("g", Command::First, "first"),
    bind("Home", Command::First, "first"),
    bind("G", Command::Last, "last"),
    bind("End", Command::Last, "last"),
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
    // A run or a job in full; a task's project.
    Binding {
        tabs: &[
            Tab::Inbox,
            Tab::Projects,
            Tab::Activity,
            Tab::Matrix,
            Tab::Project,
        ],
        ..bind("Enter", Command::Open, "open")
    },
    Binding {
        tabs: &[Tab::Inbox, Tab::Projects, Tab::Activity, Tab::Matrix],
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
    // A project: the one in view, or the selected row's.
    on(
        "z",
        &[Tab::Project, Tab::Projects, Tab::Inbox, Tab::Matrix],
        Act::Run,
        Ask::Choose("run"),
        "run",
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
    // A job. `a` and `s` are a run's keys too: a key means one thing for
    // the row it is pressed on, and `lookup` tells them apart by the row.
    on("a", JOBS, Act::ApproveJob, Ask::Nothing, "approve plan"),
    on("s", JOBS, Act::StopJob, Ask::Confirm, "stop"),
    // A workflow instance.
    on(
        "r",
        &[Tab::Activity],
        Act::ResumeWorkflow,
        Ask::Nothing,
        "resume",
    ),
    on(
        "f",
        &[Tab::Activity, Tab::Library],
        Act::StartWorkflow,
        Ask::Text("workflow target [--local] [--unattended]"),
        "start workflow",
    ),
    on(
        "s",
        &[Tab::Activity],
        Act::StopWorkflow,
        Ask::Confirm,
        "stop",
    ),
    // A campaign.
    on(
        "r",
        &[Tab::Library],
        Act::RunCampaign,
        Ask::Confirm,
        "run campaign",
    ),
    // A note.
    on("e", &[Tab::Notes], Act::EditNote, Ask::Text("note"), "edit"),
    on("x", &[Tab::Notes], Act::RemoveNote, Ask::Confirm, "remove"),
];

/// The binding `key` has on `tab` for a row about `pick`. One applying to
/// that tab alone wins over one applying everywhere; among those, an act
/// that fits the row wins, then a command that is not an act, then the
/// first, so a key for the wrong row says what it needs.
pub fn lookup(key: &str, tab: Tab, pick: &Pick) -> Option<&'static Binding> {
    let fits = |b: &&Binding| b.key == key;
    let on_tab: Vec<&Binding> = BINDINGS
        .iter()
        .filter(fits)
        .filter(|b| b.tabs.contains(&tab))
        .collect();
    let candidates = match on_tab.is_empty() {
        false => on_tab,
        true => BINDINGS
            .iter()
            .filter(fits)
            .filter(|b| b.tabs.is_empty())
            .collect(),
    };
    candidates
        .iter()
        .find(|b| matches!(b.command, Command::Act(a) if screen::fits(a, pick)))
        .or_else(|| {
            candidates
                .iter()
                .find(|b| !matches!(b.command, Command::Act(_)))
        })
        .or_else(|| candidates.first())
        .copied()
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

    /// One key means one thing for a row: two bindings for the same key on
    /// one tab must be acts on rows, and no row can fit both. A tab binding
    /// hiding an everywhere binding would make it ambiguous too.
    #[test]
    fn no_key_means_two_things_for_one_row() {
        let samples = [
            Pick::Nothing,
            Pick::Task {
                project: "p".into(),
                target: "p:1".into(),
                retry: false,
            },
            Pick::Manual {
                project: "p".into(),
            },
            Pick::Run {
                id: 1,
                clean: true,
                state: "ready".into(),
                project: "p".into(),
            },
            Pick::Job {
                id: 1,
                awaiting: true,
            },
            Pick::Instance {
                id: 1,
                workflow: "w".into(),
                open: true,
            },
            Pick::Note {
                id: 1,
                text: "t".into(),
            },
            Pick::Campaign { name: "c".into() },
        ];
        for tab in Tab::ALL {
            let on = on_tab(tab);
            let keys: Vec<&str> = on.iter().map(|b| b.key).collect();
            for k in &keys {
                let same: Vec<&&Binding> = on.iter().filter(|b| b.key == *k).collect();
                if same.len() == 1 {
                    continue;
                }
                assert!(
                    same.iter().all(|b| b.tabs.contains(&tab)),
                    "`{k}` on {tab:?} is bound for the tab and for everywhere"
                );
                for pick in &samples {
                    let fitting = same
                        .iter()
                        .filter(|b| {
                            matches!(b.command, Command::Act(a) if a.on_row() && screen::fits(a, pick))
                        })
                        .count();
                    assert!(
                        fitting <= 1,
                        "`{k}` on {tab:?} means two things for {pick:?}"
                    );
                }
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
    fn lookup_prefers_the_tab_then_the_row() {
        let job = Pick::Job {
            id: 1,
            awaiting: true,
        };
        let run = Pick::Run {
            id: 1,
            clean: true,
            state: "ready".into(),
            project: "p".into(),
        };
        assert_eq!(
            lookup("a", Tab::Activity, &Pick::Nothing).map(|b| b.command),
            Some(Command::Act(Act::ApproveJob))
        );
        assert_eq!(
            lookup("a", Tab::Inbox, &run).map(|b| b.command),
            Some(Command::Act(Act::ApproveRun))
        );
        assert_eq!(
            lookup("a", Tab::Inbox, &job).map(|b| b.command),
            Some(Command::Act(Act::ApproveJob))
        );
        assert_eq!(
            lookup("a", Tab::Inbox, &Pick::Nothing).map(|b| b.command),
            Some(Command::Act(Act::ApproveRun)),
            "the first, so the error names a run"
        );
        assert!(lookup("a", Tab::Notes, &run).is_none());
        assert_eq!(
            lookup("c", Tab::Notes, &Pick::Nothing).map(|b| b.command),
            Some(Command::Act(Act::Scan))
        );
    }
}
