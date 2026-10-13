# Async dispatch

Status: phases 1-4 built, 2026-10-12.

Where the build departs from the design below:

- `--detach` is opt-in, not the default. Approval mode prints a plan and exits at once, and scripts read a command's stdout; both would change.
- The runner does not hold `session.lock`. It runs each job as a child `pma <args>`, which takes the session as a foreground command does. The runner holds `runner.lock`, and waits for the session to be free before starting a job, so a foreground command does not fail the job.
- Each job runs under the agent watchdog (`agent::spawn_guarded`). A dead runner kills its job; a killed job's agents die through their own watchdogs. `pma job stop` sends `SIGTERM` to the job's process group, so it kills runs in flight rather than letting them finish. Those runs are marked `failed` when the next session starts.
- Progress has no `events.jsonl` and no heartbeat columns. The agent's log already is the event stream, so `pma watch` reads it when called: tool calls, the last tool, the last text, and the log's age. Nothing writes progress while a run works.
- `pma watch` takes no job id. Jobs run one at a time, so every running agent belongs to the running job or to a foreground command.
- Approval is `pma job approve <id>`, which queues the command the job recorded: `workflow run <name> --instance <n> --approve <plan>`, with the job's `-a`, `-m` and `-p` flags. The plan id hashes the resolved worker, so without them the plan is refused. The job learns its id from `PMA_JOB`, which the runner sets.
- `notify` gets the summary in `PMA_JOB_SUMMARY`, with `PMA_JOB_ID` and `PMA_JOB_STATE`, not on stdin: `notify-send` reads arguments. It runs for 30 seconds at most, logged to `jobs/<id>.notify.log`.
- Open questions 1-3 were answered by default: one job at a time, plain-text `pma job`, no notification unless `notify` is set.

## Problem

`pma workflow run` and `pma dispatch` block the terminal until every agent they start has finished. An unattended review-fix pass over `xtgen` (instance 2) ran 15 agents serially and held the terminal throughout. Closing the terminal sends SIGHUP, kills `pma`, and the next session marks its runs `failed`.

Wanted: start work, return at once, read progress while it runs, and be told when it ends. Both modes must keep working: rigorous plan approval, and `--unattended`.

## What exists

- Workflow state lives in SQLite. A pass re-derives its frontier from the store on each invocation, so any process can resume an instance (`crates/pma-core/src/pass.rs`, `advance`).
- `Session` is an `flock` on `session.lock` (`crates/pma-core/src/store.rs:593`). The kernel releases it on exit, so a crash leaves no stale lock. One process at a time may run agents.
- `fail_interrupted_runs` marks `queued` and `running` runs `failed` when the next session starts.
- The `claude` worker runs with `--output-format json`, which yields one record at exit. `stream-json` yields one per event and is already read on the `sanduk` path (`crates/pma-core/src/worker.rs`).

## Decision

A job queue in SQLite, drained by a runner that `pma` spawns on demand. Progress is observed by the runner, never reported by the agent.

### Why observe rather than let agents push

- `pma` does not trust an agent's report: it runs verify itself. A pushed progress message is the same kind of claim. It can inform; it cannot be evidence.
- A model may not call a progress tool. Process exit, stream events and worktree changes occur whether or not it does.
- A push channel is a socket or token inside the agent's environment. `pma` strips that environment of credentials; this would add one back for every worker.

### Design

1. **Jobs.** `pma workflow run ...` and `pma dispatch ...` insert a row in a new `jobs` table and print its id. The row holds the command's arguments, its mode (`unattended` or approval) and a state: `queued`, `running`, `awaiting-approval`, `done`, `failed`, `stopped`.
2. **Runner.** After inserting, the command tries `Session::try_acquire`. If the lock is free, it spawns `pma runner` detached (`setsid`, stdin from `/dev/null`, stdout and stderr to `state/runner.log`) and exits. The runner takes the lock, drains jobs oldest first, and exits when the queue is empty. If the lock is held, a runner exists and will take the job. No permanent daemon.
3. **Progress.** The `claude` worker switches to `--output-format stream-json --verbose`. The runner appends each record to `runs/<id>/events.jsonl` and updates a heartbeat on the run row: last event time, tool calls, cost so far. `pma status`, `pma tui` and a new `pma watch [job]` read these. The per-run `timeout` still applies.
4. **Approval mode.** A job that reaches a plan without `--unattended` stops with state `awaiting-approval` and the plan id. `pma workflow run --instance N --approve <plan>` queues a new job for that plan. The wait becomes a row, not a blocked process.
5. **Notification.** `pma config notify <command>` runs once per finished job with a one-line summary on stdin, e.g. `notify-send pma`. Unset by default.
6. **Control.** `pma job` lists jobs. `pma job stop <id>` marks a job `stopped`; the runner finishes the agent runs in flight and starts no more for it.

### Failure modes to test

- The runner dies while an agent runs. The agent must die with it: spawn agents in the runner's process group and kill the group on exit. Otherwise `fail_interrupted_runs` marks a run `failed` while its agent still writes to the worktree.
- Two commands race to spawn a runner. The loser fails `try_acquire` and exits; its job is already queued.
- A job is queued just as the runner finds the queue empty. The runner re-checks the queue after releasing the lock and respawns if it finds one.
- The machine sleeps. Heartbeats go stale with no fault; the timeout must use process time or wall time consistently.

## Alternatives considered

| Option | Cost | Gives | Lacks |
|-|-|-|-|
| `systemd-run --user --unit pma-<p> pma workflow run ...` | none | non-blocking, survives the terminal | progress beyond stdout; a second job is refused by the lock |
| `--detach` flag only | ~50 lines | non-blocking | queue, progress |
| Queue and on-demand runner (chosen) | ~500 lines | queue, progress, notification, both modes | concurrent jobs |
| Daemon with a socket API agents push to | largest | push updates, remote clients | daemon lifecycle, socket auth, a client per worker, the trust problem above |
| Caller is the async layer (Claude Code background shell, herdr) | small: `status --json`, stable exit codes | no change to `pma`'s process model | ties the workflow to one orchestrator |

`systemd-run` is the interim answer until the runner exists.

## Open questions

1. Should two jobs run at once, e.g. on two projects? The global `Session` lock serialises all agent work. Concurrency needs per-project locks and a shared `batch_budget` ledger.
2. Is progress read by a person or by another agent? An agent needs `pma job --json` and exit codes more than a TUI.
3. Where should the end-of-job notice go: desktop, terminal, phone, or a `pma note`?

## Phases

1. `jobs` table, `pma runner`, detached spawn, `pma job` and `pma job stop`. Output to `runner.log` only.
2. `stream-json` for `claude`, `events.jsonl`, heartbeat columns, `pma watch`.
3. `awaiting-approval` and re-queue on `--approve`.
4. `notify`.

## Remaining work

As of 2026-10-13, most urgent first.

### First real run

`review-fix` on `argdec`, instance 3, detached, in approval mode. 1 review and 6 fixes; all 6 passed verify and were applied unstaged. About $2.50.

Confirmed with a real agent: `--detach`, the runner, `pma job`, `job log`, two `job approve` cycles, settling a job by a foreground `--approve`, and `pma watch` reading `stream-json`.

Found and fixed: a review run stayed `ready`, and `pma review` called its removed tree "tampered with". Fix agents edited `CHANGELOG.md` unevenly (2 of 6). Every dispatched agent now drafts entries in `.pma/changelog.md`, which pma keeps out of the change and shows in `pma review`.

Not yet run for real: `notify`, `--unattended --detach`, `job stop`.

### Known defects

| Issue | Effect | Fix size (estimate) |
|-|-|-|
| Local fixes in one chunk conflict when `max_parallel` > 1 | The conflicting fix is discarded, using no attempt | medium: rebase onto the prior result, or keep 1 as the default |
| The working-tree snapshot leaves a dangling commit in the clone | Disk use until `git gc` | ignore, or prune after the run |
| `pma watch` parses only `claude` logs | Other workers show their last log line | defer until a second worker is in use |
| The review agent may not run code to confirm a finding | Findings rest on reading alone | a worker setting, not a code change |

### Missing tests

- `--unattended` without `--local`: auto-approve, then publish a branch or PR.

### Open questions, answered by default

- Concurrent jobs on different projects. Needs per-project locks and a shared `batch_budget` ledger.
- `pma job --json`, if an agent rather than a person reads progress.

### Older items

- Pilot cohort: 1 of 20 runs shipped as of 2026-09-24. Finishing it is the milestone.
- Agent sandbox and permissions: deferred until pma is tested. `sanduk` is the candidate.
- `TODO.md` lost 62 lines on 2026-10-12, before this work. It looked like `pma prune --apply`. Confirm it was intended.
