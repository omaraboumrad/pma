# pma and minos integration

State as of 2026-10-14. The proposal is minos `docs/dev/pma-integration.md` ([github](https://github.com/shakfu/minos/blob/main/docs/dev/pma-integration.md)). This file records pma's side: what is verified, what pma would change, and what is undecided. Room filing is proposed in minos `docs/dev/run-rooms.md`.

## Status

No pma code depends on minos. The leading option is minos 4.2: a `claude-minos` worker template that runs `claude` under `minosb` on the host. A developer could then read, correct and interrupt a run from a minos room. It waits on decision 1 below.

## Built in minos

| Flag | Purpose |
|-|-|
| `minosb -task TEXT` | Writes the task as the agent's first stdin frame. A pma template cannot express a wrapper script. |
| `minosb -events FILE` | Moves control events to FILE. stdout then carries the agent's stream unchanged, for pma's log, `pma watch` and `parse_claude`. |
| `minosb -linger DUR` | Closes the agent's stdin after DUR idle between turns. Without it a run never ends by itself (issue 1). |

`make host-run` in minos drives a real `claude` (2.1.299) through all three, plus held messages and both interrupt lanes. It also saves the agent's stream as `agent.jsonl`.

## Template

```text
minosb -user worker -room {room} -interrupters demo -task {prompt} -linger 2m \
  -socket {socket} -events {events} -- \
  claude -p --input-format stream-json --output-format stream-json --verbose ...
```

`2m` is unvalidated. It sets how long a finished run holds the single job runner waiting for corrections.

## Issues

1. **Run end. Resolved in minos.** `claude --input-format stream-json` reads stdin until it closes, and `minosb` never closed it. A run held the job runner until the timeout and counted as killed. `-linger` fixes it; a real `claude` exits 0 on end of input.

2. **Summary.** `parse_claude` (`agent.rs`) takes the last `result` record. In a corrected run that is the reply to the last correction, not the task's outcome.

3. **Success.** An interrupted turn ends with `subtype: error_during_execution`. If it is the last turn, `parse_claude` reports failure even when the work is done. Candidate fix: judge success by verify alone for this worker.

4. **Cost.** `total_cost_usd` is cumulative per session, so the last record gives the right total. An interrupted turn adds $0 and reports 0 tokens, though output was in progress. Whether that output is billed is unknown, so the total may undercount.

5. **Credential exposure.** `minosb` needs `MINOS_PASSWORD`. It removes it from the agent's environment, but the agent runs as the same uid. Under Yama `ptrace_scope=1` a same-uid process can read `/proc/<pid>/environ` (checked on this host). The agent can therefore read the worker account's password. Mitigation: give the worker account only the run's room and channel. Only a container run (minos 4.3) keeps the credential out of reach.

6. **Credential storage.** A worker's `env` (`worker.rs`) is applied after `agent::restrict` and would carry the password. Stored there, it sits in plain text in `settings.toml`. Inheriting it from pma's own environment avoids the file, not issue 5.

7. **Sandbox. Not a blocker.** The `sanduk_sandbox` policy confines verify only (`dispatch.rs`, `verify_once`). Agent runs pass none, so `minosb` reaches the server and creates its socket.

8. **Who can steer.** Anyone who can post in the run's room can direct an unsandboxed agent. Room membership is a trust decision.

9. **Record split.** After a correction, the stored prompt no longer describes what the agent was told. The room holds the rest. pma stores no room id today.

10. **Stale pma docs. Resolved 2026-10-14.** `using-containers.md` and `implementation-plan.md` 2b.6-2b.8 described `minosd` on the sealed network, a minos client in the container, and a Rust wire client in pma. minos rejected all three. Both now follow the broker design.

## pma work, if 4.2 goes ahead

In dependency order:

1. minos: `minos room ensure` (proposed, not built).
2. A room id column on the run row, with migration; shown by `pma review` and the front ends.
3. A minos credential in settings, and the `ensure` call before a worker whose template names `{room}`.
4. `{room}`, `{socket}` and `{events}` placeholders in `worker.rs`. `{socket}` must be outside the worktree and under 103 bytes.
5. Issues 2 and 3 in `parse_claude`, for this worker.
6. Exit test: a real dispatch whose room shows in the minos TUI.

## Decisions

1. **Are mid-run corrections needed?** Approval-mode runs already stop at plan approval. If unsure, record during the remaining cohort runs each point where a correction would have helped.
2. **Room grain.** One room per task gives up to 1031 rooms, and minos cannot delete one. One per dispatched run bounds the count by work done.
3. **Interrupter.** The developer's account in `-interrupters`, or a pma account that posts `/interrupt`. pma cannot write `minosb`'s stdin: the watchdog gives the worker `/dev/null`.
4. **Server down.** Does a run fail, or run without a room?

Alternative if decision 1 is no: a `notify` command that posts each job's summary to a room. No broker and no template. The room is then a record only.
