//! The record of past scans: each project's state at each scan, and what
//! changed since the scan before. `Store::save_scan` writes both; this module
//! decides what counts as a change.

use crate::scan::Ci;

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
    /// Lint errors or a scan error, where there were none.
    Broken(String),
    Mended,
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
            Change::Broken(_) => "broken",
            Change::Mended => "mended",
            Change::Opened { .. } => "opened",
            Change::Closed { .. } => "closed",
        }
    }

    /// Whether the change is good news.
    pub fn good(&self) -> bool {
        matches!(
            self,
            Change::CiPassing | Change::Mended | Change::Closed { .. } | Change::Returned
        )
    }

    /// Whether the change asks for attention.
    pub fn bad(&self) -> bool {
        matches!(
            self,
            Change::CiFailing(_) | Change::Broken(_) | Change::Absent
        )
    }

    pub fn detail(&self) -> String {
        match self {
            Change::CiFailing(names) => names.join(", "),
            Change::Broken(why) => why.clone(),
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
            Change::Broken(why) => ("broken", None, why.clone()),
            Change::Mended => ("mended", None, String::new()),
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
            "broken" => Change::Broken(detail),
            "mended" => Change::Mended,
            "opened" => Change::Opened {
                priority,
                text: detail,
            },
            "closed" => Change::Closed {
                priority,
                text: detail,
            },
            _ => return None,
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
    pub broken: i64,
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

/// Why a project counts as broken, or `None`.
pub fn broken(lint_errors: i64, scan_error: Option<&str>) -> Option<String> {
    match (lint_errors, scan_error) {
        (_, Some(e)) => Some(e.lines().next().unwrap_or_default().to_string()),
        (0, None) => None,
        (1, None) => Some("1 lint error".into()),
        (n, None) => Some(format!("{n} lint errors")),
    }
}

pub fn broken_change(before: Option<&str>, after: Option<&str>) -> Option<Change> {
    match (before, after) {
        (None, Some(why)) => Some(Change::Broken(why.to_string())),
        (Some(_), None) => Some(Change::Mended),
        _ => None,
    }
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

    #[test]
    fn broken_names_the_first_reason() {
        assert_eq!(broken(0, None), None);
        assert_eq!(broken(1, None).as_deref(), Some("1 lint error"));
        assert_eq!(broken(3, None).as_deref(), Some("3 lint errors"));
        assert_eq!(
            broken(3, Some("git status: timed out\nmore")).as_deref(),
            Some("git status: timed out")
        );
        assert_eq!(
            broken_change(None, Some("2 lint errors")),
            Some(Change::Broken("2 lint errors".into()))
        );
        assert_eq!(broken_change(Some("x"), None), Some(Change::Mended));
        assert_eq!(broken_change(Some("x"), Some("y")), None);
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
            Change::Broken("2 lint errors".into()),
            Change::Mended,
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
