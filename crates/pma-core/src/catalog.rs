//! The names `pma run` takes: a catalog of short names over the library's
//! workflows and the two dispatch signals, and every other workflow of the
//! revision in effect that a target can name. The CLI lists them; the TUI
//! and the web offer them in a chooser. Design: `docs/dev/run.md`.

use crate::store::{Result, Store};
use crate::workflow;

/// What a name runs.
#[derive(Clone, Copy)]
pub enum Runs<'a> {
    /// A workflow of the revision in effect.
    Workflow(&'a str),
    /// `pma dispatch project:<signal>`, whose prompt carries the failing
    /// logs or the outdated packages; a workflow `edit` would get the names
    /// alone.
    Signal(&'static str),
}

pub struct Entry {
    pub name: &'static str,
    pub about: &'static str,
    pub runs: Runs<'static>,
    /// The `claude` model asked for when nothing else names one. Reviews
    /// get the strongest: in the pilot, haiku's review findings were false
    /// and Opus's were true (`docs/dev/workflow-pilot.md`).
    pub model: Option<&'static str>,
}

pub const CATALOG: &[Entry] = &[
    Entry {
        name: "review",
        about: "Review a project, then confirm each finding against the code",
        runs: Runs::Workflow("find-issues"),
        model: Some("opus"),
    },
    Entry {
        name: "ensemble-review",
        about: "Review for bugs, missing tests and misusable interfaces; confirm each",
        runs: Runs::Workflow("ensemble-review"),
        model: Some("opus"),
    },
    Entry {
        name: "doc-drift",
        about: "Find README and doc statements the code contradicts",
        runs: Runs::Workflow("doc-drift"),
        model: Some("opus"),
    },
    Entry {
        name: "todo-audit",
        about: "Find open TODO.md items the code already does, as notes",
        runs: Runs::Workflow("todo-audit"),
        model: Some("opus"),
    },
    Entry {
        name: "security-review",
        about: "Review for security faults; confirm each by tracing input to its sink",
        runs: Runs::Workflow("security-review"),
        model: Some("opus"),
    },
    Entry {
        name: "test-gaps",
        about: "Find code no test reaches and tests that cannot fail; confirm each",
        runs: Runs::Workflow("test-gaps"),
        model: Some("opus"),
    },
    Entry {
        name: "review-and-fix",
        about: "Review, confirm, then fix the critical and high findings; edits wait in `pma review`",
        runs: Runs::Workflow("review-and-fix"),
        model: Some("opus"),
    },
    Entry {
        name: "project-brief",
        about: "One note per project: what it does, for whom, in what",
        runs: Runs::Workflow("project-brief"),
        model: None,
    },
    Entry {
        name: "triage-issues",
        about: "Judge each open GitHub issue against the code, by severity",
        runs: Runs::Workflow("triage-issues"),
        model: None,
    },
    Entry {
        name: "fix-ci",
        about: "Fix failing CI from its logs; each verified edit lands unstaged",
        runs: Runs::Signal("ci"),
        model: None,
    },
    Entry {
        name: "deps-upgrade",
        about: "Update outdated dependencies; each verified edit lands unstaged",
        runs: Runs::Signal("deps"),
        model: None,
    },
];

/// Who a bundled revision is proposed and activated by.
pub const LIBRARY_BY: &str = "pma run";

/// Input types a command-line target can name; see `pass::root_units`.
pub const TARGETABLE: [&str; 3] = ["project", "item", "signal"];

/// A name `pma run` takes: what it reads and what it does.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub name: String,
    pub reads: String,
    pub about: String,
}

/// The catalog, then every other workflow of the revision in effect, or of
/// the bundled library before one is; and the workflows left out because no
/// target can name their input, each with that input.
pub fn choices(store: &Store) -> Result<(Vec<Choice>, Vec<String>)> {
    // A library revision is replaced at the next run when `pma` ships
    // another, so the bundled text is what would run.
    let (doc, from) = match store.active_workflow()? {
        Some(r) if r.proposed_by != LIBRARY_BY => {
            (r.document()?, format!("revision {}", r.revision))
        }
        _ => (
            workflow::Document::parse(workflow::LIBRARY)?,
            "the bundled library".to_string(),
        ),
    };
    let mut out = Vec::new();
    for e in CATALOG {
        let (reads, about) = match e.runs {
            Runs::Signal(_) => ("project".to_string(), e.about.to_string()),
            Runs::Workflow(w) => match doc.workflow(w) {
                Some(w) => (w.input.clone(), e.about.to_string()),
                None => ("-".into(), format!("{} (not in {from})", e.about)),
            },
        };
        out.push(Choice {
            name: e.name.to_string(),
            reads,
            about,
        });
    }
    let named = |n: &str| {
        CATALOG
            .iter()
            .any(|e| matches!(e.runs, Runs::Workflow(w) if w == n))
    };
    let mut unreachable = Vec::new();
    for w in doc.workflows.iter().filter(|w| !named(&w.name)) {
        if !TARGETABLE.contains(&w.input.as_str()) {
            unreachable.push(format!("{} (reads {})", w.name, w.input));
            continue;
        }
        let effects = match w.effects.names().as_slice() {
            [] => String::new(),
            names => format!("; {}", names.join(", ")),
        };
        out.push(Choice {
            name: w.name.clone(),
            reads: w.input.clone(),
            about: format!(
                "workflow of {from}, returns {}{effects}",
                w.output.as_deref().unwrap_or("nothing")
            ),
        });
    }
    Ok((out, unreachable))
}
