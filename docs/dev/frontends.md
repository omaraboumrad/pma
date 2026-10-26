# Front ends: CLI, TUI, web

Status: phases 1-6 built, 2026-10-13.

Where the build departs from the design below:

Phase 1, settings:

- The configuration tables stay, as a mirror of `settings.toml`. Opening the store refills them when the file's text differs from the text they were last filled from (`settings_sync`); every write edits the file, then refills them. Queries that join `projects.tier` or `project_tags` are unchanged. A migration that changes a configuration row must also change the file, or the next refill undoes it.
- A tier for a project no scan has recorded waits in the file; the scan that adds the project applies it.
- `project forget` removes the project's whole `[projects.<name>]` table from the file.

Phase 2, the API (`crates/pma-core/src/api.rs`):

- `Query` covers the operating reads; `Action` covers the short acts, which run in place, and the long ones, which `act` queues as a job running the CLI command `api::command_line` names. The CLI keeps running long acts in the foreground; it is the command a job runs.
- Queued commands spell every option `--name=value`, so a value starting with `-`, such as rework feedback, is not read as a flag. A test parses each one with the CLI's own parser.
- `act` refuses a scan that is both offline and reading dependencies, which the CLI would refuse only once the job started.
- `Query::Run` records the tree it showed, as `pma review <id>` did: an approval requires it.
- Not in the API yet: pending plans (a waiting job holds its plan's approve command; pricing a plan needs a workflow pass), `lint` and `prune` (they act on `TODO.md` files by path, not on the store), and `--dry-run`. Phase 4 decides whether the TUI needs them.
- The CLI renders every `View` through one exhaustive `match`. Its output is unchanged; 16 read commands were compared against the previous binary on a real store.

Phase 3, crates:

- `pma-web` is not created yet: an empty crate gives nothing. It comes with phase 5.
- The task picker `pma dispatch` opens (`choose.rs`) is a ratatui screen, so it lives in `pma-tui`.
- `crates/pma/src/main.rs` is 3,763 lines: the CLI's argument definitions and its rendering of each view, plus the configuring commands.


Phase 4, the TUI:

- A third shared module, `pma_core::screen`, sits between the API and both front ends. It turns a `View` into panes of rows, each with a `Pick` saying what it is about, or into text; and it builds the `Action` a key takes on a pick. The design had each front end do this. Shared, neither decides for itself what a row is or what a key does to it.
- Ten tabs: next, matrix, runs, jobs, agents, workflows, campaigns, report, notes, and status on `0`. `stale` has no tab.
- The hub (`Query::Hub`, `Tab::Hub`, on `H`) is a tree, `Body::Tree`, not panes: project, task, then the runs and jobs working on the task. A job is placed by its command line: a dispatch by its targets, a publish by its run id, a scan, verify or workflow by its projects. Only live jobs, and jobs failed in the last day, are shown; a finished job is told by its run. Each `Node` has an id that is its path from the root, so the same run under two branches is two nodes, and a front end keeps what is open and what is selected by id across reloads. `screen::visible` turns the tree and what the viewer opened or closed into rows; the TUI and the web both draw those rows as a table: the name, indented by depth, then each node's `cells` under the tree's `columns`, with the selected node's `fields` and `notes` beside them. Where a tab binds one key to acts on two kinds of row, as the hub binds `a` and `s` for runs and jobs, `screen::lookup` picks the act that fits the selected row, and a test holds that no row fits both.
- Keys are DOM `KeyboardEvent.key` names (`j`, `Enter`, `Escape`), so the web reads the table unchanged. A binding names the tabs it applies on, whether it confirms or prompts, and its label.
- `App` is a state machine: a key returns `Load(Query)` or `Act(Action)`, and the loop sends it through the API. Its tests need no store.
- A `Screen` is typed: each pane has columns of a kind (text, wide, number, badge), each cell a `Tone` (good, warn, bad, accent, muted, strong), and a detail has fields and sections (prose, log, diff). The tone of a run state, a job state, a priority or a project state is decided once, in `screen`, so both front ends colour it alike. Each draws it its own way: the TUI from the terminal's ANSI palette, the web as pills. The web is not laid out like the TUI; parity covers what each shows and does, not how.
- A project view (`Query::Project`, `Tab::Project`) has no key of its own: `o` opens the project of the selected row, and `Escape` returns to the tab it came from. The web also lists every project in its sidebar, from the status view, which now lists untiered projects as well. `screen::fits` says which acts apply to a row, so neither front end offers approve on a task or dispatch on a run.

Phase 5, the web:

- No HTTP crate: `std::net::TcpListener` and `httparse`, one request per connection, headers capped at 16 KB and bodies at 64 KB. The token comes from `/dev/urandom`; the page opens through `xdg-open` or `open`, which puts the token in that process's arguments.
- `Host` must be `127.0.0.1` or `localhost`, on any port, so an SSH forward to another local port works: `ssh -L 9000:127.0.0.1:<port>`. A POST's `Origin` must be the origin its own `Host` names. `--port` fixes the port, so a forward can be written once.
- Browsers do not separate cookies by port, so another server on 127.0.0.1 that the browser visits receives the session cookie. Only a local process can listen there, and one running as the user can already run `pma`.
- `--app` opens the page in Chrome or Chromium with `--app=<url>`: a window with no browser controls. It is not a webview: the server and its checks are the same, and the token is still on that process's command line.
- The event stream sends a `tick`, every 2 s while a job or run is under way and every 30 s otherwise, and the page reloads its tab. It does not push views.
- `/api/ask` refuses an act that cannot apply to the selected row before the page asks to confirm it, as the TUI does.

Phase 6, parity:

- Compile-time: `screen::screen` matches every `View`, `Act::of` every `Action`, and `screen::action` every `Act`.
- Tests: every act and tab has a key; no key means two things on one tab; the TUI handles every binding on every tab; `app.js` has a case for every command kind; the web server takes every act by name; every queued action's command line parses.
- Races: a front end's approve or reject carries the state its row showed (`seen`), and is refused once the run has left it. Rejecting now writes its state conditionally before it removes the worktree, so a reject from a stale view no longer overwrites an approval. Tested between the web and the CLI on one run, one plan and one job.
- The page's JavaScript has no unit tests. It was driven in headless Chrome against a scratch store and a real one: tabs, a prompt, a confirmation, help, run detail, and no console errors.

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
| Act | `scan`; `dispatch`; `workflow run`, approve a plan, `workflow stop`; approve, reject or rework a run; `stage`; `commit`; `pr`; `push`; `job stop`, `job approve`; `campaign run`; `sync`; `prune`; `verify`; add, edit or remove a note |

**Configuring** (CLI and `settings.toml` only): `config`, `root`, `project tier/tag/export/import/forget/clone`, `agent`, `preset`, `route`, `workflow propose/activate`, `campaign add/rm`.

## Decision

### An API in the core, front ends over it

`crates/pma/src/main.rs` is 3,915 lines with 157 print calls; most commands mix logic and output there. The library modules print almost nothing (`dispatch`, `publish`: 0; `pass`: 1). The work is to move each command's logic behind two types:

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

`dispatch`, `workflow run` (an approved plan), `campaign run` and `scan` hold `session.lock` for minutes. Started from the TUI or the web, each is queued as a job (`crates/pma-core/src/jobs.rs`), and the view follows it. Short actions (approve, reject, stop, notes) run directly. Neither front end blocks on agent work, and both behave alike.

### Live data

Both front ends poll the core every 2 s while any job or run is active, and every 30 s otherwise: the same cadence as `pma watch`. The web server pushes each changed view to the browser by server-sent events (SSE). Agent progress comes from the existing log reading in `crates/pma-core/src/watch.rs`.

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
