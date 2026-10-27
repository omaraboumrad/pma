# TODO

## Critical

## High

- [x] The web page opens blank, its footer stays on "Connecting", and its project list is empty
  Done 2026-10-27: `app.js` still named the tabs `next` and `status` from before the 2026-10-26 merge. The first draw threw on `TABS["next"]`, which stopped `start()` before it opened the event stream; `tab=status` got a 400. A test now holds that every tab `app.js` names is one the server parses.

- [x] Enter in a web prompt cancelled it
  Done 2026-10-27: the form's default button was its first, Cancel. Found driving the page in headless Chromium.

- [x] Keep the web view in the URL, so a reload keeps it
  Done 2026-10-27: `?tab=project&project=x&open=run:52`, each change a history entry. No React: the page has no build step by design. Not the fragment: the launch token is there.

- [x] Say what a reject did: "Run #52 rejected; reverted 2 files."
  Done 2026-10-27, in the CLI and both front ends.

- [x] A pull request's body holds the agent's summary, the verify result and the run number
  Done 2026-10-27. It was the commit message after its first blank line, empty when the message is one line.

- [x] Show the commit message before committing, and let it be changed
  Done 2026-10-27: `m` prompts for the subject, prefilled with the task text; `pma commit --subject`. The trailers (`Closes #N`, `Co-Authored-By`) stay and are not shown. A multi-line editor would need a new `Ask` kind in both front ends.

- [x] Refuse a queued action already queued or running
  Done 2026-10-27: the same command from the same directory is refused while one is queued or running, in one statement.

- [x] One click opens a row's detail in the web
  Done 2026-10-27: a row with a detail opens on one click, since the detail carries its acts; other rows select.

- [x] `z` in both front ends filters by name as well as moving with the arrows; the web aligns names and descriptions as the TUI does
  Done 2026-10-27: `/` filters in the TUI; a filter box in the web.

- [x] Admin from the TUI and the web
  Done 2026-10-26: a settings tab (keys, roots, agents, presets, routes), project tier, tags, verify and sandbox from the project view, prune, campaign add and remove, rescan with dependencies or offline, deps refresh, scan history and route documents. Still CLI-only: `project export`, `import`, `forget` and `clone`, `route replay`, `lint --ids` (`docs/dev/frontends.md`).

- [x] `pma report` counts `applied` runs as neither accepted nor landed
  21 runs in state `applied` (`--local` workflow fixes on `xtgen` and `argdec`) are missing from the headline, which reads "25% of 8 decided runs accepted, 2 landed". Count `applied` as landed, or give it its own column, and name it in the headline. (REVIEW.md U2)

- [ ] Finish the pilot cohort and apply its gate #manual
  1 of 20 items shipped since 2026-09-24 (`docs/dev/pilot.md`). Fix the `report` count above first, or the shares will mislead. (REVIEW.md U1)

- [x] Rank untiered projects instead of leaving them out
  77 of 80 projects are untiered, so `pma next` holds 12 of about 1,200 open items. Rank them at a default tier, below every tiered project. Optionally, propose tiers from idle days, CI and fork state. (REVIEW.md U3)

- [ ] Decide whether pma is the dashboard or the dispatch loop #manual
  The evidence supports `scan`/`status`/`history`; most of the code serves dispatch and workflows. pma is for other developers, one per install, so the adoption items below are in scope; multi-user use comes later. (REVIEW.md, Questions)

- [x] `pma lint --fix` for mechanical errors
  `nanosrv` has 46 errors, all "a list entry in this section must be a `- [ ]` item", and a broken file hides every item it holds. Rewrite `- text` to `- [ ] text` under a priority heading; keep the parser strict. (REVIEW.md U9)

- [ ] npm or pnpm dependency signals, and a better Python verify fallback
  deps covers `cargo`, `uv` and `go` (`crates/pma-core/src/deps.rs:24-26`). Without `uv.lock`, verify detection picks `python3 -m pytest` (`crates/pma-core/src/agent.rs:263-268`), likely without the project's dependencies (unverified). The README's Requirements now state what is supported. (REVIEW.md U7)

## Medium

- [ ] Retest live updates in the web after the blank-page fix #manual
  The event stream existed but was never opened (see High). Many of the out-of-sync views reported on 2026-10-27 likely came from that. Retest before building more.

- [ ] Group a row's acts under one menu in the web toolbar
  They overflow even on a wide screen. Keep the primary act of the view and of the row visible; the rest under "More". Keys are unchanged.

- [ ] Finished runs (rejected, pushed, merged) in the activity tab, with a filter by state
  Activity lists only what is under way.

- [ ] Say where to go after staging
  After `s`, nothing says that `m` commits next. A toast naming the next act, with the run kept selected, may be enough.

- [ ] Show a hint of a workflow's state machine when it is selected in `z`
  The server would send a short form of the graph with each choice.

- [ ] Project settings as one field per setting, not one typed line
  Fields need a new `Ask` kind in `keys`, built in both front ends. Cheaper: one row per setting in the project view, each edited alone, as the settings tab does.

- [ ] Make the worktree option easier to find
  It exists: "edit in a worktree" in `z`, `--worktree`, and `pma config workspace worktree`. Colleagues asked to revisit the clone default; `docs/dev/clone-runs.md` holds why it was chosen. Ask which of its reasons fails for them before changing the default.

- [ ] Re-run a failed task as it was, from the failed run
  `Pick::Task` has `retry` (`crates/pma-core/src/screen.rs`); unverified whether a failed run's detail offers it.

- [ ] Undo the last rework: return to the previous attempt rather than rejecting the run
  Reject reverts every attempt. Attempts are append-only rows, but a clone run reverts one patch, so this needs each attempt's patch kept. Open: is the need "discard the latest rework, keep the earlier one approvable"?

- [ ] Scheduled runs within a budget, finding issues and adding them to TODO.md with duplicates merged
  Runs start by hand. `pma web` is not a daemon, so an in-process schedule needs a resident process. First try a cron or launchd entry running a read-only audit workflow under `workflow_budget`; the new part is merging duplicate findings. Tapping into an agent's session is the session-id item below.

- [ ] Click through the web page after the drawer removal #manual
  The 2026-10-26 rewrite (toolbar acts, in-place detail, chooser, toasts with Open) had a syntax check and the server tests only; no Chrome on the machine. Try run, the chooser, a run detail, a toast's Open button, and the 1100 px and 720 px widths.

- [ ] Show open pull requests and issues opened by others as signal tasks
  `scan` reads only CI from GitHub (`crates/pma-core/src/scan.rs:680`), and `sync` lists issues by others as untriaged but ranks none. A project with unanswered contributor pull requests shows as `clear`. Read them with `gh pr list` and `gh issue list` during `scan`, rank them by age, and do not make them dispatchable by default. (REVIEW.md U6)

- [ ] A task source that is never pushed, for public repositories
  Dispatch needs the item in `TODO.md` on origin (`crates/pma-core/src/dispatch.rs:601`), so every task an agent takes on an OSS repository is published first. Options: a gitignored `.pma/TODO.md`, or items held in the database. This conflicts with "TODO.md is the only task file" (`docs/dev/design.md`); decide that first, or document the exposure. (REVIEW.md U5)

- [ ] A per-project setting that refuses host mode
  For private code, `projects.<name>.worker_mode = "container"` would stop a dispatch from running unconfined by accident. Host mode leaves `~/.ssh`, reads and the network open. (REVIEW.md U10)

- [ ] Propose `#agent` tags across the portfolio
  15 of 1,201 open items carry `#agent`, so `dispatch --auto` has little to take. A read-only workflow, as `todo-audit` is, could propose tags; or a per-project default could mark items that have a description. (REVIEW.md U4)

- [ ] Per-project session locks
  One agent command and one job run at a time across all projects (`docs/dev/async_dispatch.md`, open question 1), so a detached job on one project blocks a dispatch on another. Needs a shared `batch_budget` ledger. (REVIEW.md U8)

- [ ] A project whose name contains a dot can take `projects.<name>.verify` #agent

- [ ] `pma review 3 4 --approve --minutes 5` drops the minutes #agent

- [ ] `pma prune --apply` exits 0 when it skips a file for lint errors #agent

- [ ] `pma project export` writes a CSV that does not read back when a tag holds a comma or a project is named `#x` #agent

- [ ] One failed dependency tool is counted as zero outdated when another succeeds #agent

- [ ] A code fence indented 1-3 spaces does not hide its content from the TODO.md parser #agent

- [ ] `pma merge <id>...` merges a run's pull request and settles it #agent Today a `pr-open` run is merged on GitHub by hand, and the next `pma review` settles it. `pma merge` runs `gh pr merge <url> --squash --delete-branch` for each named `pr-open` run, then settles it to `merged` at once, as `publish::settle` does for a merge it observes. Refuse a run that is not `pr-open`. Refuse one whose pull request has failing or pending checks; allow one with no checks. Refuse one whose head is no longer the commit pma pushed, since that merges commits no one approved: record the pushed commit at `pma pr`, which today records only the URL. Tests in `tests/cli.rs` with the fake `gh` (`FAKE_GH_PR`): a merge, each refusal, and the run merged afterwards.

- [x] **Render `gh:N` as a link**: the inbox, matrix and project rows and the run and task details link the issue to its page from the stored `owner/name`; `pma review` prints the number.

- [ ] The process-group kill does not reach a child that calls `setsid` (`src/agent.rs:105-106`, `119`). `docs/dev/design.md:377` says no agent outlives the session. A survivor's writes to the worktree no longer reach an approval or a publish: an approval must match the tree last shown, and publishing starts from the approved tree. It still runs, reads what the user can, and writes outside the worktree. A cgroup or PID namespace would bound it; `sanduk` does. (REVIEW.md M10)

- [ ] Record each run's agent session id, so `resumes` does something
  `Worker::resumes` is stored, settable and listed by `pma agent`, but no dispatch path reads it (`crates/pma-core/src/worker.rs:210`). `opencode()` sets it `true`, so the listing claims a feature pma lacks. A `{session}` placeholder in worker args (`claude --session-id <uuid>`, as wallfacer does) plus a session column on the run would let a rework continue the earlier session instead of re-prompting, and let `pma review N --open` hand the terminal to `claude --resume <id>` in the worktree. Takeover must run under the `agent::restrict` environment, or it gains push credentials. Open: does a `-p` session resume interactively; does a `sanduk` transcript survive its container; what is opencode's flag. If neither use is wanted, delete the field.

## Low

- [x] The TUI footer names only the last transition of a batch #agent
  `App::noticed` keeps the last of what one `Watch::poll` returned. Three runs reaching review within a second name one. Queue them, or name the count.

- [x] A CLI command that opens a task in full #agent
  `Query::Task` serves the TUI and the web only. `pma project show` lists tasks one line each. A `pma task <project:line>` would print what the detail shows.

- [ ] Mark commands without a recorded pilot as experimental in `--help` and the README
  34 top-level commands; routes and campaigns have no recorded runs. (REVIEW.md U11)

- [ ] Date or range the README's scan time
  It says 95 repositories in about 15 s; 80 took 24.6 s on 2026-10-25. (REVIEW.md U12)

- [ ] Record a hand estimate of each task's time, so a time-saved figure exists
  `--minutes` records review time only. (REVIEW.md, Questions)

- [ ] Citations of the earlier REVIEW.md (`M10`, `L5`, `D4`, ...) in TODO.md and `docs/dev/workflows.md` point at a file that now holds another review
  Rewrite them, or keep earlier reviews under `docs/dev/`.

- [ ] Dispatched runs likely leave untracked transcripts in `~/.claude/projects/<encoded-worktree>/`
  Each worktree is a new cwd, so each run probably adds a session pma neither records nor removes, and it shows in `claude --resume`. Unverified: check after a real run. Then choose: keep them as an audit trail of what the agent did, which the diff does not show, or remove them with the worktree. wallfacer's `CompanionFiler` handles the same cleanup problem.

- [ ] Accept an item id, or a unique prefix of one, as a dispatch target: `myproject:^k3f`
  Targets are `project:line` (`crates/pma/src/main.rs:230`), and a line number goes stale when TODO.md is edited after the scan; an id does not. An ambiguous prefix lists the candidates rather than guessing, as wallfacer resolves its refs.

- [ ] `retry` on `edit`, then lap edges, then recursion (`docs/dev/workflows.md` section 19, parts 6c-6e). Gated: 6c needs 10 runs published through batch approval.

- [ ] **Identity by slug rather than by directory name**: `discover` skips a repository whose basename another root already supplied, so two roots cannot both hold a `py`. Worth doing when a second root exists, not before.

- [ ] `user_version` is read before the migration transaction opens (`src/store.rs:658-693`). Two processes starting after an upgrade both replay the steps; the second fails with "already exists". (REVIEW.md L5)

- [ ] `project import --apply` and `project tier` apply many rows without a transaction (`src/main.rs:1196-1227`). (REVIEW.md L8)

- [ ] `pma pr` pushes `run.branch`; if the agent switched branches, the commit is elsewhere (`src/publish.rs:195`). (REVIEW.md L11)

- [ ] `prune` and `lint --ids` do not re-read TODO.md before saving, so an edit made between the read and the save is lost (`crates/pma/src/main.rs`). The shared temp name is fixed. (REVIEW.md L13)

- [ ] Errors in `lint`, `prune`, `sync` and publish go to stdout (`crates/pma/src/main.rs`). The `prune --apply` exit code is a pilot cohort item in Medium. (REVIEW.md L14)

- [ ] Open: does `cargo update --dry-run` ever print lines `parse_cargo` (`crates/pma-core/src/deps.rs`) misses? The counting bug beside it is a pilot cohort item in Medium. (REVIEW.md L15)

- [ ] Smaller review items, unverified: run scope stored comma-joined (`src/store.rs:1149`); the CI prompt has no byte cap and can exceed the argument limit (`src/dispatch.rs:726`); cost over budget is judged on cumulative cost across reworks (`src/accept.rs:91`); table widths count characters, not display columns. (REVIEW.md)

- [ ] Windows support
  pma-core assumes Unix, so a Windows build stops at a `compile_error!`; `release.yml` builds Linux and macOS only. Sites: the `sh` watchdog and `kill -KILL -<pgid>` (`crates/pma-core/src/agent.rs:121`, `:134`; `scan.rs:357`; `jobs.rs:186`), `process_group(0)` (`agent.rs:147`, `scan.rs:314`, `jobs.rs:63`), `Command::new("sh")` in `agent.rs`, `jobs.rs:152`, `scan.rs:1081` and `dispatch.rs:1401`, and mode bits in `agent.rs:70` and the hook snapshot (`dispatch.rs:375`). Needs a job object to kill a process tree (`windows-sys`), `CREATE_NEW_PROCESS_GROUP` for the runner, a Windows rule for which hooks git runs, and a Windows CI job. sanduk-sandbox has no Windows backend, so confined verify fails closed there. Open: is WSL, which runs the Linux binary unchanged, enough? Until then, a `compile_error!` in `pma-core/src/lib.rs` names the limit; remove it with the port.
