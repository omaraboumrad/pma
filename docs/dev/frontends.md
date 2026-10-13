# Front ends: CLI, TUI, web

Status: design, 2026-10-13. Nothing built.

## Goal

Three ways to run `pma`:

1. `pma <command>`: the CLI.
2. `pma tui`: a terminal UI. Used over SSH, so it must not lag the web view.
3. `pma web`: a local web UI, opened in the browser with a token.

The TUI and the web view have operating parity: anything one shows or does, the other shows or does. They differ only in the view. Configuring is out of parity scope: it is done in `~/.config/pma/settings.toml`.

## Scope

**Operating** (in parity):

| Kind | What |
|-|-|
| Read | `next`, `matrix`, `status`, `stale`, `report`; runs awaiting review and run detail (diff, summary, changelog draft, attempts, verify); live agents (`watch`); jobs and job logs; workflow revisions, instances and pending plans; campaigns; notes; lint results |
| Act | `scan`; `dispatch`; `workflow run`, approve a plan, `workflow stop`; approve, reject or rework a run; `pr`; `push`; `job stop`, `job approve`; `campaign run`; `sync`; `prune`; `verify`; add, edit or remove a note |

**Configuring** (CLI and `settings.toml` only): `config`, `root`, `project tier/tag/export/import/forget/clone`, `agent`, `preset`, `route`, `workflow propose/activate`, `campaign add/rm`.

## Decision

### An API in the core, front ends over it

`src/main.rs` is 3,915 lines with 157 print calls; most commands mix logic and output there. The library modules print almost nothing (`dispatch`, `publish`: 0; `pass`: 1). The work is to move each command's logic behind two types:

- `Query`: one variant per read. It returns a plain struct, never text.
- `Action`: one variant per act. It returns an outcome struct, or a job id.

Each front end renders those structs. The TUI and the web each handle `Action` and `Query` with an exhaustive `match`. Adding a variant fails the build until both handle it. That is the parity rule, enforced by the compiler rather than by review.

The CLI moves onto the same API over time. It keeps the configuring commands, which the other two do not have.

### Crates

```
crates/pma-core   store, scan, dispatch, pass, workflow, jobs, settings; Query and Action. No printing, no UI deps.
crates/pma-tui    ratatui front end
crates/pma-web    HTTP server and embedded HTML/JS; behind a cargo feature
crates/pma        the binary: CLI, `pma tui`, `pma web`
```

One installed binary. The split stops UI code leaking into the core: `pma-core` cannot name `ratatui` or the HTTP crate. The split comes after the API extraction; before it, it only moves code.

### Long actions are jobs

`dispatch`, `workflow run` (an approved plan), `campaign run` and `scan` hold `session.lock` for minutes. Started from the TUI or the web, each is queued as a job (`src/jobs.rs`), and the view follows it. Short actions (approve, reject, stop, notes) run directly. Neither front end blocks on agent work, and both behave alike.

### Live data

Both front ends poll the core every 2 s while any job or run is active, and every 30 s otherwise: the same cadence as `pma watch`. The web server pushes each changed view to the browser by server-sent events (SSE). Agent progress comes from the existing log reading in `src/watch.rs`.

### Web security

The web view is strictly local.

- Bind `127.0.0.1` on a random port. Never another interface.
- A 32-byte random token per launch. `pma web` opens `http://127.0.0.1:<port>/#<token>`. The page trades the token for an `HttpOnly`, `SameSite=Strict` cookie, then replaces the history entry without it. A fragment is never sent in a request, so it reaches no log.
- Reject a request whose `Host` is not `127.0.0.1:<port>`: this blocks DNS rebinding, where a remote page's hostname resolves to 127.0.0.1.
- Reject a POST whose `Origin` is not the server's: this blocks CSRF, a request another page sends in the user's name.
- A strict Content-Security-Policy; no third-party scripts, styles or fonts.
- HTML and JS embedded with `include_str!`; no build step.

Residual risk: a process running as the same user can read the token (from the browser's argv, for one). That process can already run `pma` itself, so the token adds no exposure. It exists to stop other users and web pages.

Approving from a browser makes the browser part of the approval gate. With the checks above it is as safe as the terminal for a single local user.

### Server crate

`tiny_http` fits: `pma` is synchronous and uses threads. `axum` would bring `tokio` and an async runtime into a sync codebase. Inference: `tiny_http` is maintained but slow to change; check its state before adding it.

## settings.toml

Configuration moves from the database to `~/.config/pma/settings.toml`. The file is the only source; the database keeps no configuration. The first run imports the current rows into the file once.

| Into the file | Stays in the database |
|-|-|
| `config` keys, including `projects.<name>.*` | workflow revisions |
| roots | route revisions |
| agents (workers) and presets | scanned project facts, tasks, runs, jobs, units |
| project tiers and tags | |

Workflow and route revisions stay in the database. Each records who proposed and activated it. Activating a workflow checks its worst case against `workflow_budget`. A file the user edits would skip both. `settings.toml` may name a workflow source file, but `workflow propose` and `activate` stay the only way to put one into effect.

Costs:

- A parser: `toml` (reading) and `toml_edit` (writing, keeping comments) are new dependencies, with `serde`.
- `pma config <key> <value>` either edits the file through `toml_edit` or goes away in favour of editing the file. Keeping it costs little and keeps scripts working.
- An invalid file stops every command with the line and the error. A half-read configuration would run agents with the wrong budget.
- Tiers and tags for 69 projects move into the file. `project export/import` already writes that shape and may then be retired.

## Phases

1. Settings: `settings.toml`, the one-time import, `pma config` writing the file.
2. API: `Query` and `Action` in the core; the CLI prints from them; output unchanged. The 75 CLI tests guard this. The largest phase.
3. Workspace split into the four crates.
4. TUI over the API: jobs, runs and review, live agents, workflows and plans, reports, notes, actions.
5. `pma web`: server, token, SSE, the same views and actions.
6. A parity test: every `Query` and `Action` variant through both front ends; every `Action` has a key; two front ends racing on one run, one plan and one job.

Phase 1 is independent of 2-6 and can come later. It comes first here because it removes configuring from the API's scope.

## Settled

1. `pma config <key> <value>` stays and writes `settings.toml` through `toml_edit`, keeping comments. It checks a value when it is written; a hand edit is checked when the file is next read. The CLI tests call `pma config` 47 times.
2. `pma tui` and `pma web` may run at once. Every action that changes state is a conditional update: it writes only while the row is in the state the view showed (`update_run_from`, `approve_job`). The losing side gets "changed since shown" and refreshes. Plans are settled by the plan check; long actions queue as jobs.
3. Both use one key table in `pma-core`, mapping each key to an `Action` or a view. Both help screens list it; the web reads it as embedded JSON. Keys are plain letters with no modifier, and the web takes them only when focus is not in a text field. The web also has buttons, for mouse and screen-reader use.
