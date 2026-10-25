//! `pma run`: a named job over projects, in one command. A front end over
//! `pma workflow run` and `pma dispatch`: it adds short names, paths as
//! targets, a default model per job and a gate on bulk runs, and nothing the
//! two commands do not already do. Design: `docs/dev/run.md`.

use std::path::{Path, PathBuf};

use pma_core::{api, dispatch, scan, store, workflow};
use store::{Result, Store, WorkflowRevision};

use super::{WorkflowAction, run_dispatch, workflow_command};

/// What a name runs.
#[derive(Clone, Copy)]
enum Runs<'a> {
    /// A workflow of the revision in effect.
    Workflow(&'a str),
    /// `pma dispatch project:<signal>`, whose prompt carries the failing
    /// logs or the outdated packages; a workflow `edit` would get the names
    /// alone.
    Signal(&'static str),
}

struct Entry {
    name: &'static str,
    about: &'static str,
    runs: Runs<'static>,
    /// The `claude` model asked for when nothing else names one. Reviews
    /// get the strongest: in the pilot, haiku's review findings were false
    /// and Opus's were true (`docs/dev/workflow-pilot.md`).
    model: Option<&'static str>,
}

const CATALOG: &[Entry] = &[
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

/// The worker a catalog model belongs to.
const HINTED_AGENT: &str = "claude";

/// Who a bundled revision is proposed and activated by.
const LIBRARY_BY: &str = "pma run";

pub struct Request {
    pub name: Option<String>,
    pub targets: Vec<String>,
    pub all: bool,
    pub tags: Vec<String>,
    pub worker: Option<String>,
    pub set: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
    /// Leave each edit in `pma review` rather than in the clone.
    pub review: bool,
}

pub fn run(req: Request) -> Result<()> {
    let Some(name) = req.name.clone() else {
        return list();
    };
    let store = Store::open_default()?;
    let cfg = api::config(&store)?;
    let entry = CATALOG.iter().find(|e| e.name == name);
    let runs = match entry {
        Some(e) => e.runs,
        // Any other workflow of the revision in effect runs by its own name.
        None => Runs::Workflow(&name),
    };
    if req.all && !req.targets.is_empty() {
        return Err("`--all` takes no targets".into());
    }
    if !req.all && req.tags.is_empty() && req.targets.is_empty() {
        return Err(format!(
            "name a project or a path, or pass `--all`: `pma run {name} <project>`"
        )
        .into());
    }
    let bulk = req.all || !req.tags.is_empty();

    let (agent, model) = match &req.worker {
        Some(w) => {
            let (a, m) = match w.split_once(':') {
                Some((a, m)) => (a.to_string(), Some(m.to_string())),
                None => (w.clone(), None),
            };
            if !store.agents()?.iter().any(|k| k.name == a) {
                return Err(format!("unknown agent `{a}`; `pma agent` lists them").into());
            }
            (Some(a), m)
        }
        None => (None, hinted(&store, &cfg, entry)?),
    };
    if let (Some(m), None, Some(e)) = (&model, &req.worker, entry) {
        println!(
            "model {m}: `{}`'s default; `-w agent:model` names another",
            e.name
        );
    }

    let rows = store.projects()?;
    let tagged: Vec<String> = store
        .project_tags()?
        .into_iter()
        .filter(|(_, t)| req.tags.contains(t))
        .map(|(p, _)| p)
        .collect();
    // Present and scanned: a project a full scan no longer finds is not one
    // to run anything over.
    let pool = rows.iter().filter(|r| {
        r.absent_since.is_none() && r.scanned_at.is_some() && (req.all || tagged.contains(&r.name))
    });

    match runs {
        Runs::Signal(signal) => {
            let targets: Vec<String> = match bulk {
                true => pool
                    .filter(|r| match signal {
                        "ci" => matches!(r.ci, scan::Ci::Failing(_)),
                        _ => r.deps.unwrap_or(0) > 0,
                    })
                    .map(|r| format!("{}:{signal}", r.name))
                    .collect(),
                false => req
                    .targets
                    .iter()
                    .map(|t| signal_target(&rows, t, signal))
                    .collect::<Result<_>>()?,
            };
            if targets.is_empty() {
                println!("{}", nothing_for(signal));
                return Ok(());
            }
            let local = !req.review;
            if req.dry_run || (bulk && !req.yes) {
                let goes = match local {
                    true => "each edit that passes verify written into its clone, unstaged",
                    false => "each edit left in `pma review`",
                };
                println!("would dispatch {}, {goes}:", targets.len());
                for t in &targets {
                    println!("  {t}");
                }
                if !req.dry_run {
                    println!("a bulk run; rerun with `--yes` to dispatch them");
                }
                return Ok(());
            }
            let over = dispatch::Overrides {
                agent,
                model,
                preset: None,
            };
            run_dispatch(&targets, &[], false, None, false, local, &over)
        }
        Runs::Workflow(workflow_name) => {
            let revision = effective_revision(&store, &cfg)?;
            let doc = revision.document()?;
            let Some(w) = doc.workflow(workflow_name) else {
                return Err(match entry {
                    Some(_) => format!(
                        "revision {} in effect has no `{workflow_name}`; `pma workflow library` holds it",
                        revision.revision
                    ),
                    None => format!("no `{name}`; `pma run` lists what runs"),
                }
                .into());
            };
            let projects: Vec<String> = match bulk {
                true if w.input != "project" => {
                    return Err(format!(
                        "`{name}` reads {}, not projects, so `--all` and `--tag` do not apply",
                        w.input
                    )
                    .into());
                }
                true => pool.map(|r| r.name.clone()).collect(),
                false => req
                    .targets
                    .iter()
                    .map(|t| match is_path(t) {
                        true => project_at(&rows, t),
                        false => Ok(t.clone()),
                    })
                    .collect::<Result<_>>()?,
            };
            if projects.is_empty() {
                println!("no project to run `{name}` over");
                return Ok(());
            }
            let gated = bulk && !req.yes;
            let before = latest_instance(&store)?;
            workflow_command(Some(WorkflowAction::Run {
                name: workflow_name.to_string(),
                projects,
                tag: None,
                instance: None,
                set: req.set,
                dry_run: req.dry_run || gated,
                approve: None,
                unattended: !(req.dry_run || gated),
                local: false,
                detach: false,
                cap: Vec::new(),
                agent,
                model,
                preset: None,
            }))?;
            if gated && !req.dry_run {
                println!("\na bulk run; rerun with `--yes` to run it unattended");
                return Ok(());
            }
            if let Some(id) = latest_instance(&store)?.filter(|id| Some(*id) != before) {
                print_outputs(&store, &doc, workflow_name, id)?;
                print_documents(id)?;
            }
            Ok(())
        }
    }
}

/// Input types a command-line target can name; see `pass::root_units`.
const TARGETABLE: [&str; 3] = ["project", "item", "signal"];

/// The catalog, then every other workflow of the revision in effect, or of
/// the bundled library before one is. A workflow whose input no target can
/// name is left out, and said so.
fn list() -> Result<()> {
    let store = Store::open_default()?;
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
    let mut rows = vec![["name", "reads", "what"].map(String::from).to_vec()];
    for e in CATALOG {
        let (reads, about) = match e.runs {
            Runs::Signal(_) => ("project".to_string(), e.about.to_string()),
            Runs::Workflow(w) => match doc.workflow(w) {
                Some(w) => (w.input.clone(), e.about.to_string()),
                None => ("-".into(), format!("{} (not in {from})", e.about)),
            },
        };
        rows.push(vec![e.name.to_string(), reads, about]);
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
        rows.push(vec![
            w.name.clone(),
            w.input.clone(),
            format!(
                "workflow of {from}, returns {}{effects}",
                w.output.as_deref().unwrap_or("nothing")
            ),
        ]);
    }
    println!("pma run <name> <project|path>... [--all | --tag T] [-w agent[:model]]\n");
    print!("{}", pma_core::report::table(&rows, "  "));
    if !unreachable.is_empty() {
        println!(
            "\nnot runnable from here, no target names their input: {}",
            unreachable.join(", ")
        );
    }
    Ok(())
}

/// The catalog's model for `entry`, when nothing else would name one: no
/// route policy applies, no default preset is set, and the default worker
/// is the one the model belongs to.
fn hinted(
    store: &Store,
    cfg: &pma_core::config::Config,
    entry: Option<&Entry>,
) -> Result<Option<String>> {
    let Some(model) = entry.and_then(|e| e.model) else {
        return Ok(None);
    };
    let routed = store.active_route()?.is_some_and(|r| !r.shadow);
    Ok((!routed && cfg.preset.is_none() && cfg.agent == HINTED_AGENT).then(|| model.to_string()))
}

/// The revision a run uses: one you activated, else the bundled library,
/// proposed and activated on first use and again when `pma` ships another.
/// The library goes in only within `workflow_budget`, as any revision does.
fn effective_revision(store: &Store, cfg: &pma_core::config::Config) -> Result<WorkflowRevision> {
    let library = workflow::Document::parse(workflow::LIBRARY)?;
    let text = library.to_json();
    if let Some(r) = store.active_workflow()?
        && (r.proposed_by != LIBRARY_BY || r.document == text)
    {
        return Ok(r);
    }
    let mut runs = 0;
    let mut cost = 0.0f64;
    for w in &library.workflows {
        let e = library.estimate(&w.name, 1, cfg.agent_budget)?;
        runs = runs.max(e.agent_runs);
        cost = cost.max(e.cost);
    }
    if cost > cfg.workflow_budget {
        return Err(format!(
            "the bundled workflows could cost ${cost:.2} per project, over workflow_budget of \
             ${:.2}. Allow it once with `pma config workflow_budget {}`; each run still \
             spends what its agents do.",
            cfg.workflow_budget,
            cost.ceil()
        )
        .into());
    }
    let n = store.add_workflow_revision(&library, None, LIBRARY_BY, runs, cost)?;
    store.activate_workflow(n, LIBRARY_BY)?;
    println!("revision {n}: the bundled workflows, now in effect");
    Ok(store
        .workflow_revisions()?
        .into_iter()
        .find(|r| r.revision == n)
        .expect("just stored"))
}

fn is_path(t: &str) -> bool {
    t.contains('/') || t.starts_with('~') || t == "."
}

/// The project at a path, by its canonical form.
fn project_at(rows: &[store::ProjectRow], t: &str) -> Result<String> {
    let path = match t.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?).join(rest),
        None => PathBuf::from(t),
    };
    let want = path.canonicalize().map_err(|e| format!("{t}: {e}"))?;
    let same = |p: &Path| p.canonicalize().is_ok_and(|p| p == want);
    rows.iter()
        .find(|r| same(&r.path))
        .map(|r| r.name.clone())
        .ok_or_else(|| {
            format!(
                "{t} is no project pma knows; `pma root add {}`, then `pma scan`",
                want.parent().unwrap_or(&want).display()
            )
            .into()
        })
}

/// `project:<signal>` for a project, a path or that target written out.
fn signal_target(rows: &[store::ProjectRow], t: &str, signal: &str) -> Result<String> {
    let project = match t.rsplit_once(':') {
        Some((p, s)) if s == signal => p.to_string(),
        Some(_) => {
            return Err(format!("`{t}`: this runs over `project:{signal}` targets only").into());
        }
        None if is_path(t) => project_at(rows, t)?,
        None => t.to_string(),
    };
    Ok(format!("{project}:{signal}"))
}

fn nothing_for(signal: &str) -> &'static str {
    match signal {
        "ci" => "no project has failing CI at the last scan",
        _ => "no project has outdated dependencies at the last `pma scan --deps`",
    }
}

fn latest_instance(store: &Store) -> Result<Option<i64>> {
    Ok(store.workflow_instances()?.last().map(|i| i.id))
}

/// The prose documents the instance's agents wrote, such as `REVIEW.md`,
/// kept under `artifacts/` once their trees are gone.
fn print_documents(instance: i64) -> Result<()> {
    let base = store::state_home()?
        .join("artifacts")
        .join(instance.to_string());
    let mut docs = Vec::new();
    let mut dirs = vec![base];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "md") {
                docs.push(path);
            }
        }
    }
    docs.sort();
    for d in docs {
        println!("written: {}", d.display());
    }
    Ok(())
}

/// What reached the workflow's output, one line per unit: its severity or
/// verdict, its project, its title or text, and the reason it was kept.
fn print_outputs(store: &Store, doc: &workflow::Document, name: &str, instance: i64) -> Result<()> {
    let w = doc.flatten(name)?;
    let moves = store.workflow_moves(instance)?;
    let out: Vec<store::WorkflowUnit> = store
        .workflow_units(instance)?
        .into_iter()
        .filter(|u| {
            moves.iter().any(|(id, edge, taken)| {
                *taken
                    && *id == u.id
                    && w.edges
                        .get(*edge)
                        .is_some_and(|e| e.to == workflow::To::Output)
            })
        })
        .collect();
    if out.is_empty() {
        println!("\ninstance {instance}: nothing reached the output");
        return Ok(());
    }
    println!(
        "\ninstance {instance}: {} {}(s)",
        out.len(),
        w.output.as_deref().unwrap_or("unit")
    );
    let field = |d: &serde_json::Value, keys: &[&str]| -> String {
        keys.iter()
            .find_map(|k| d.get(*k).and_then(|v| v.as_str()))
            .unwrap_or_default()
            .to_string()
    };
    let mut rows = Vec::new();
    for u in &out {
        let d: serde_json::Value = serde_json::from_str(&u.data).unwrap_or_default();
        rows.push(vec![
            field(&d, &["severity", "verdict"]),
            u.project.clone().unwrap_or_default(),
            field(&d, &["title", "text", "summary"]),
        ]);
        let reason = field(&d, &["reason"]);
        if !reason.is_empty() {
            rows.push(vec![String::new(), String::new(), format!("  {reason}")]);
        }
    }
    print!("{}", pma_core::report::table(&rows, "  "));
    Ok(())
}
