//! The record of past scans: each project's state at each scan, and what
//! changed since the scan before. `Store::save_scan` writes both; this module
//! decides what counts as a change.

use crate::scan::{Ci, Fault};

/// Scans kept. An older one is deleted with its rows.
pub const KEPT: i64 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// No earlier scan recorded the project.
    New,
    /// A full scan found it again after it was absent.
    Returned,
    /// A full scan did not find it under any root.
    Absent,
    /// CI fails where the last known state did not; the failing workflows.
    CiFailing(Vec<String>),
    /// CI no longer fails.
    CiPassing,
    /// A fault the scan before did not find, with its cause.
    Fault(Fault, String),
    /// A fault the scan before found is gone.
    Mended(Fault),
    Opened {
        priority: String,
        text: String,
    },
    Closed {
        priority: String,
        text: String,
    },
}

impl Change {
    /// What the change is, as a short phrase.
    pub fn name(&self) -> &'static str {
        match self {
            Change::New => "new",
            Change::Returned => "returned",
            Change::Absent => "absent",
            Change::CiFailing(_) => "CI failing",
            Change::CiPassing => "CI passing",
            Change::Fault(f, _) => f.name(),
            Change::Mended(_) => "mended",
            Change::Opened { .. } => "opened",
            Change::Closed { .. } => "closed",
        }
    }

    /// Whether the change is good news.
    pub fn good(&self) -> bool {
        matches!(
            self,
            Change::CiPassing | Change::Mended(_) | Change::Closed { .. } | Change::Returned
        )
    }

    /// Whether the change asks for attention.
    pub fn bad(&self) -> bool {
        matches!(
            self,
            Change::CiFailing(_) | Change::Fault(..) | Change::Absent
        )
    }

    pub fn detail(&self) -> String {
        match self {
            Change::CiFailing(names) => names.join(", "),
            Change::Fault(_, why) => why.clone(),
            Change::Mended(f) => f.name().into(),
            Change::Opened { priority, text } | Change::Closed { priority, text } => {
                format!("{priority}: {text}")
            }
            _ => String::new(),
        }
    }

    /// Kind, priority and detail, as stored.
    pub fn to_columns(&self) -> (&'static str, Option<&str>, String) {
        match self {
            Change::New => ("new", None, String::new()),
            Change::Returned => ("returned", None, String::new()),
            Change::Absent => ("absent", None, String::new()),
            Change::CiFailing(names) => ("ci_failing", None, names.join("\n")),
            Change::CiPassing => ("ci_passing", None, String::new()),
            Change::Fault(f, why) => (f.column(), None, why.clone()),
            Change::Mended(f) => ("mended", None, f.column().into()),
            Change::Opened { priority, text } => ("opened", Some(priority), text.clone()),
            Change::Closed { priority, text } => ("closed", Some(priority), text.clone()),
        }
    }

    pub fn from_columns(kind: &str, priority: Option<String>, detail: String) -> Option<Change> {
        let priority = priority.unwrap_or_default();
        Some(match kind {
            "new" => Change::New,
            "returned" => Change::Returned,
            "absent" => Change::Absent,
            "ci_failing" => Change::CiFailing(detail.lines().map(String::from).collect()),
            "ci_passing" => Change::CiPassing,
            "mended" => Change::Mended(Fault::from_column(&detail)?),
            "opened" => Change::Opened {
                priority,
                text: detail,
            },
            "closed" => Change::Closed {
                priority,
                text: detail,
            },
            kind => Change::Fault(Fault::from_column(kind)?, detail),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectChange {
    pub project: String,
    pub change: Change,
}

/// One recorded scan, with totals over the projects it read.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    pub id: i64,
    pub at: i64,
    /// Every project under the roots, rather than a selection.
    pub full: bool,
    pub projects: i64,
    pub failing: i64,
    /// Projects with each fault, in `Fault::ALL` order.
    pub faults: [i64; 7],
    /// Open items in the projects it read.
    pub open: i64,
    pub opened: i64,
    pub closed: i64,
    /// Changes other than items opened and closed.
    pub other: i64,
}

/// A CI change, judged against the last state that was known. An unknown
/// state, as an offline scan records, is not news either way.
pub fn ci_change(before: Option<&Ci>, after: &Ci) -> Option<Change> {
    let failing = |c: &Ci| matches!(c, Ci::Failing(_));
    match (before, after) {
        (_, Ci::Unknown(_)) | (None | Some(Ci::Unknown(_)), _) => None,
        (Some(b), Ci::Failing(names)) => (b != after).then(|| Change::CiFailing(names.clone())),
        (Some(b), _) if failing(b) => Some(Change::CiPassing),
        _ => None,
    }
}

/// The faults found or gone between two scans' fault lists.
pub fn fault_changes(before: &[(Fault, String)], after: &[(Fault, String)]) -> Vec<Change> {
    let has = |list: &[(Fault, String)], f: Fault| list.iter().any(|(g, _)| *g == f);
    let found = after
        .iter()
        .filter(|(f, _)| !has(before, *f))
        .map(|(f, why)| Change::Fault(*f, why.clone()));
    let gone = before
        .iter()
        .filter(|(f, _)| !has(after, *f))
        .map(|(f, _)| Change::Mended(*f));
    found.chain(gone).collect()
}

/// When a project's CI began failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Since {
    /// The earliest scan in the latest unbroken run of failing scans.
    pub at: i64,
    /// A scan before the run knew CI not to fail. Without one, the run
    /// reaches the oldest record and `at` is only a lower bound.
    pub known: bool,
}

/// `states` is newest first, each a scan's time and CI; an unknown CI
/// neither ends nor extends the run.
pub fn failing_since(states: &[(i64, Ci)]) -> Option<Since> {
    let mut since: Option<Since> = None;
    for (at, ci) in states {
        match ci {
            Ci::Unknown(_) => {}
            Ci::Failing(_) => {
                since = Some(Since {
                    at: *at,
                    known: false,
                })
            }
            _ => return since.map(|s| Since { known: true, ..s }),
        }
    }
    since
}

/// How long CI has failed, in words: `new` at the latest scan, else since
/// when, marked a lower bound when no earlier scan knew it to pass.
pub fn since_text(since: Option<&Since>, latest: Option<i64>, now: i64) -> String {
    match since {
        Some(s) if s.known && Some(s.at) == latest => "new".into(),
        Some(s) if s.known => format!("since {}", crate::report::ago(now - s.at)),
        Some(s) if Some(s.at) != latest => {
            format!("since {} or earlier", crate::report::ago(now - s.at))
        }
        _ => "no earlier record".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing(name: &str) -> Ci {
        Ci::Failing(vec![name.into()])
    }

    #[test]
    fn ci_changes_are_judged_against_the_last_known_state() {
        let off = Ci::Unknown("offline".into());
        assert_eq!(
            ci_change(Some(&Ci::Passing), &failing("ci")),
            Some(Change::CiFailing(vec!["ci".into()]))
        );
        assert_eq!(
            ci_change(Some(&failing("ci")), &Ci::Passing),
            Some(Change::CiPassing)
        );
        assert_eq!(ci_change(Some(&failing("ci")), &failing("ci")), None);
        assert_eq!(
            ci_change(Some(&failing("ci")), &failing("lint")),
            Some(Change::CiFailing(vec!["lint".into()]))
        );
        assert_eq!(ci_change(Some(&failing("ci")), &off), None);
        assert_eq!(ci_change(None, &failing("ci")), None);
        assert_eq!(ci_change(Some(&Ci::Passing), &Ci::NoRuns), None);
    }

    /// A fault is news when it appears or goes; a new cause for a fault
    /// already found is not.
    #[test]
    fn a_fault_changes_when_it_appears_or_goes() {
        let lint = |why: &str| (Fault::TodoLint, why.to_string());
        let git = [(Fault::GitUnreadable, "git status: timed out".to_string())];
        assert_eq!(fault_changes(&[], &[]), []);
        assert_eq!(fault_changes(&[lint("a")], &[lint("b")]), []);
        assert_eq!(
            fault_changes(&[lint("a")], &git),
            [
                Change::Fault(Fault::GitUnreadable, "git status: timed out".into()),
                Change::Mended(Fault::TodoLint),
            ]
        );
    }

    #[test]
    fn failing_since_skips_unknown_scans() {
        let off = Ci::Unknown("offline".into());
        let states = [
            (5, failing("ci")),
            (4, off.clone()),
            (3, failing("ci")),
            (2, Ci::Passing),
            (1, failing("ci")),
        ];
        assert_eq!(failing_since(&states), Some(Since { at: 3, known: true }));
        assert_eq!(
            failing_since(&states[..3]),
            Some(Since {
                at: 3,
                known: false
            })
        );
        assert_eq!(failing_since(&[(2, Ci::Passing)]), None);
        assert_eq!(failing_since(&[(2, off)]), None);
    }

    #[test]
    fn changes_round_trip_through_columns() {
        let all = [
            Change::New,
            Change::Returned,
            Change::Absent,
            Change::CiFailing(vec!["a".into(), "b".into()]),
            Change::CiPassing,
            Change::Fault(Fault::TodoLint, "line 2: the item has no text".into()),
            Change::Fault(Fault::ScanPartial, "git log: timed out".into()),
            Change::Mended(Fault::GitUnreadable),
            Change::Opened {
                priority: "High".into(),
                text: "x".into(),
            },
            Change::Closed {
                priority: "Low".into(),
                text: "y".into(),
            },
        ];
        for c in all {
            let (kind, priority, detail) = c.to_columns();
            assert_eq!(
                Change::from_columns(kind, priority.map(String::from), detail),
                Some(c)
            );
        }
    }
}
