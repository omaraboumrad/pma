//! What CI said of a landed run's commit. A fix-CI run is judged on the
//! workflows failing at its dispatch; any other run on whether it broke a
//! workflow passing then. Design: docs/dev/ci-verdict.md.

use crate::store::Run;

/// How long a landed run waits for a decisive CI run before it is `NoRun`.
pub const WAIT: i64 = 7 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A fix-CI run: every workflow failing at dispatch passed.
    Fixed,
    /// A fix-CI run: a workflow failing at dispatch failed again.
    StillFailing,
    /// Any other run: no workflow passing at dispatch failed on its commit.
    Clean,
    /// Any other run: a workflow passing at dispatch failed on its commit.
    Regressed,
    /// No decisive run within `WAIT`.
    NoRun,
}

impl Verdict {
    const ALL: [(&'static str, Verdict); 5] = [
        ("fixed", Verdict::Fixed),
        ("still-failing", Verdict::StillFailing),
        ("clean", Verdict::Clean),
        ("regressed", Verdict::Regressed),
        ("no-run", Verdict::NoRun),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, v)| *v == self)
            .map_or("", |(n, _)| n)
    }

    pub fn parse(s: &str) -> Option<Verdict> {
        Self::ALL.iter().find(|(n, _)| *n == s).map(|(_, v)| *v)
    }

    /// CI is worse than the run meant to leave it.
    pub fn bad(self) -> bool {
        matches!(self, Verdict::StillFailing | Verdict::Regressed)
    }

    /// One line for a person.
    pub fn text(self) -> &'static str {
        match self {
            Verdict::Fixed => "fixed",
            Verdict::StillFailing => "still failing",
            Verdict::Clean => "broke nothing",
            Verdict::Regressed => "broke CI",
            Verdict::NoRun => "no CI run on the commit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Passed,
    Failed,
    /// Cancelled, skipped or neutral: decides nothing.
    Neutral,
}

impl Outcome {
    pub fn of(status: &str, conclusion: &str) -> Outcome {
        if status != "completed" {
            return Outcome::Pending;
        }
        match conclusion {
            "success" => Outcome::Passed,
            "failure" | "timed_out" | "startup_failure" => Outcome::Failed,
            _ => Outcome::Neutral,
        }
    }
}

/// One workflow run on the default branch, newest first in a list.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub sha: String,
    pub workflow: String,
    pub outcome: Outcome,
}

/// The verdict on `run`, or `None` while CI has not decided. `seen` is newest
/// first; `descends(sha)` says whether `sha` has the landed commit as an
/// ancestor.
pub fn judge(
    run: &Run,
    seen: &[Seen],
    now: i64,
    mut descends: impl FnMut(&str) -> bool,
) -> Option<Verdict> {
    let (Some(sha), Some(at), Some(before)) = (&run.landed_sha, run.landed_at, &run.ci_before)
    else {
        return None;
    };
    // The newest run of `workflow` on the landed commit that is not neutral.
    let on = |workflow: &str| {
        seen.iter()
            .find(|s| &s.sha == sha && s.workflow == workflow && s.outcome != Outcome::Neutral)
            .map(|s| s.outcome)
    };
    let decided = if run.task_key == "ci" {
        let mut all = !before.is_empty();
        let mut verdict = None;
        for w in before {
            // A commit pushed with later ones may have no run of its own; the
            // earliest decisive run on a commit after it stands in.
            let outcome = on(w).or_else(|| {
                seen.iter()
                    .rev()
                    .filter(|s| &s.sha != sha && &s.workflow == w)
                    .filter(|s| matches!(s.outcome, Outcome::Passed | Outcome::Failed))
                    .find(|s| descends(&s.sha))
                    .map(|s| s.outcome)
            });
            match outcome {
                Some(Outcome::Failed) => verdict = Some(Verdict::StillFailing),
                Some(Outcome::Passed) => {}
                _ => all = false,
            }
        }
        verdict.or(all.then_some(Verdict::Fixed))
    } else {
        // Only the commit itself: a later one could be someone else's.
        let mut workflows: Vec<&str> = seen
            .iter()
            .filter(|s| &s.sha == sha)
            .map(|s| s.workflow.as_str())
            .collect();
        workflows.sort_unstable();
        workflows.dedup();
        let outcomes: Vec<(&str, Option<Outcome>)> =
            workflows.iter().map(|w| (*w, on(w))).collect();
        if outcomes
            .iter()
            .any(|(w, o)| *o == Some(Outcome::Failed) && !before.iter().any(|b| b == w))
        {
            Some(Verdict::Regressed)
        } else if outcomes.iter().any(|(_, o)| o.is_some())
            && outcomes.iter().all(|(_, o)| *o != Some(Outcome::Pending))
        {
            Some(Verdict::Clean)
        } else {
            None
        }
    };
    decided.or((now - at >= WAIT).then_some(Verdict::NoRun))
}

/// Verdicts on each landed run still waiting, from CI read in `facts`. A
/// project whose CI was not read this time is left for a later read.
pub fn judge_landed(runs: &[Run], facts: &[crate::scan::Facts], now: i64) -> Vec<(i64, Verdict)> {
    runs.iter()
        .filter(|r| r.ci_verdict.is_none())
        .filter_map(|r| {
            let landed = r.landed_sha.as_deref()?;
            let f = facts.iter().find(|f| {
                f.name == r.project && f.ci_read && !matches!(f.ci, crate::scan::Ci::Unknown(_))
            })?;
            let mut known = std::collections::HashMap::new();
            let descends = |sha: &str| {
                *known.entry(sha.to_string()).or_insert_with(|| {
                    std::process::Command::new("git")
                        .arg("-C")
                        .arg(&r.repo)
                        .args(["merge-base", "--is-ancestor", landed, sha])
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
                })
            };
            judge(r, &f.ci_seen, now, descends).map(|v| (r.id, v))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landed(key: &str, before: &[&str]) -> Run {
        Run {
            task_key: key.into(),
            landed_sha: Some("L".into()),
            landed_at: Some(1000),
            ci_before: Some(before.iter().map(|s| s.to_string()).collect()),
            ..Run::blank()
        }
    }

    fn seen(sha: &str, workflow: &str, outcome: Outcome) -> Seen {
        Seen {
            sha: sha.into(),
            workflow: workflow.into(),
            outcome,
        }
    }

    const NEVER: fn(&str) -> bool = |_| false;
    use Outcome::*;

    #[test]
    fn a_fix_is_judged_on_the_workflows_failing_at_dispatch() {
        let run = landed("ci", &["test", "audit"]);
        let pass = [seen("L", "test", Passed), seen("L", "audit", Passed)];
        assert_eq!(judge(&run, &pass, 1001, NEVER), Some(Verdict::Fixed));
        // A workflow that was passing may fail without failing the fix.
        let other = [
            seen("L", "test", Passed),
            seen("L", "audit", Passed),
            seen("L", "wheels", Failed),
        ];
        assert_eq!(judge(&run, &other, 1001, NEVER), Some(Verdict::Fixed));
        let one = [seen("L", "test", Failed), seen("L", "audit", Pending)];
        assert_eq!(judge(&run, &one, 1001, NEVER), Some(Verdict::StillFailing));
    }

    #[test]
    fn a_fix_waits_for_every_target_then_gives_up_after_a_week() {
        let run = landed("ci", &["test", "audit"]);
        let half = [seen("L", "test", Passed), seen("L", "audit", Pending)];
        assert_eq!(judge(&run, &half, 1001, NEVER), None);
        assert_eq!(judge(&run, &[], 1000 + WAIT - 1, NEVER), None);
        assert_eq!(judge(&run, &[], 1000 + WAIT, NEVER), Some(Verdict::NoRun));
    }

    /// The newest run on the commit decides, so a re-run that passed counts,
    /// and a cancelled one is skipped.
    #[test]
    fn the_newest_decisive_run_on_the_commit_decides() {
        let run = landed("ci", &["test"]);
        let rerun = [
            seen("L", "test", Neutral),
            seen("L", "test", Passed),
            seen("L", "test", Failed),
        ];
        assert_eq!(judge(&run, &rerun, 1001, NEVER), Some(Verdict::Fixed));
    }

    /// With no run on the commit, the earliest decisive run on a later commit
    /// stands in; a commit not after it does not.
    #[test]
    fn a_later_commit_stands_in_for_a_fix_with_no_run() {
        let run = landed("ci", &["test"]);
        let later = [
            seen("C", "test", Failed),
            seen("B", "test", Passed),
            seen("A", "test", Failed),
        ];
        let after = |s: &str| s != "A";
        assert_eq!(judge(&run, &later, 1001, after), Some(Verdict::Fixed));
        assert_eq!(judge(&run, &later, 1001, NEVER), None);
    }

    #[test]
    fn another_run_is_judged_on_its_commit_alone() {
        let run = landed("5", &["audit"]);
        let clean = [seen("L", "test", Passed), seen("L", "audit", Failed)];
        assert_eq!(judge(&run, &clean, 1001, NEVER), Some(Verdict::Clean));
        let broke = [seen("L", "test", Failed), seen("L", "audit", Passed)];
        assert_eq!(judge(&run, &broke, 1001, NEVER), Some(Verdict::Regressed));
        let waiting = [seen("L", "test", Passed), seen("L", "lint", Pending)];
        assert_eq!(judge(&run, &waiting, 1001, NEVER), None);
        // A later commit's failure is not blamed on this one.
        let later = [seen("M", "test", Failed)];
        assert_eq!(judge(&run, &later, 1001, |_| true), None);
    }

    #[test]
    fn a_run_without_a_landing_or_a_ci_state_at_dispatch_is_not_judged() {
        let mut run = landed("ci", &["test"]);
        run.ci_before = None;
        assert_eq!(judge(&run, &[], 1000 + WAIT, NEVER), None);
        let mut run = landed("ci", &["test"]);
        run.landed_sha = None;
        assert_eq!(judge(&run, &[], 1000 + WAIT, NEVER), None);
    }

    #[test]
    fn names_round_trip() {
        for (n, v) in Verdict::ALL {
            assert_eq!(Verdict::parse(n), Some(v));
            assert_eq!(v.name(), n);
        }
    }
}
