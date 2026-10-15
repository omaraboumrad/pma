# pma-agent

Design note, 2026-10-15. How `pma-agent`, the manager layer of [design.md](design.md) goal 5, would be built. Nothing here is built. It builds on minos option 4.2 ([pma-minos-integration.md](pma-minos-integration.md)) and on workflow agent nodes ([workflows.md](workflows.md)).

## 1. Summary

[design.md](design.md) gives `pma-agent` three jobs:

- classify and specify tasks;
- read a worker's output and instruct it mid-run;
- propose what to approve.

minos [design.md](https://github.com/shakfu/minos/blob/main/docs/dev/design.md) section 2 adds a fourth: proposals about which workflow a situation gets, and revisions of routing policy.

This note splits the jobs by mechanism:

| Job | Mechanism | New principal? |
|-|-|-|
| Classify, specify, estimate, summarise a review | An agent `map` node in a workflow | No. A worker routed by `route.rs` |
| Instruct a running worker | A supervisor process per run, on the run's minos room | Yes, the `pma-agent` minos account |
| Propose a workflow, a trigger or a policy revision | A judgment node whose output a person activates | No |

So `pma-agent` is not one standing agent. It is a routing role for judgment nodes, plus one short-lived supervisor per supervised run. No process holds a context across runs or projects.

## 2. Constraints carried forward

- **An LLM step proposes; code acts** ([design-review.md](design-review.md), "Manager and workers"). `pma-agent` commits nothing, pushes nothing, and approves nothing.
- **An agent's authority must not exceed that of the least-trusted principal who can write to it** (minos design.md section 3). `pma-agent` reads worker output and repository text, so it is bounded by both.
- **Per task, not one fleet context.** A fleet context reading every room is a channel between workers ([pma_feedback.md](https://github.com/shakfu/minos/blob/main/docs/dev/pma_feedback.md) section 1; minos D10). Cross-project judgment stays in `rank.rs` and `route.rs`, from stored scores.
- **The developer reaches a worker when `pma-agent` is down or wrong** (minos design.md section 5, "The three conversations"). Supervision is advisory; a run must not depend on it.
- **Agent and model live in the routing policy** (W12). Whether a run is supervised, and by which model, is a route field, not a node field.
- **`pma-agent` may propose a workflow revision or trigger; it may not activate one** (W15).

## 3. Judgment steps: workflow nodes

Every judgment in design-review's table that an LLM makes fits a primitive already built:

| Judgment | Primitive | Example |
|-|-|-|
| Class and agent-suitability | `map out: 1`, agent; verdict fields in `writes` | `triage-issues` (workflows.md 15.4) |
| Spec with acceptance criteria | `map out: 1`, agent | `specify` (15.5) |
| Complexity estimate | `map out: 1`, agent; `complexity` in `writes` | 3.7 |
| Review summary with risk flags | `map out: 1` after `edit` and `check` | -- |

`pma` checks each output: the declared type, and that no field outside `writes` changed. The node's model comes from a route matching `node` and `lap`. No new code path is needed beyond the nodes and their prompts.

Gate: implementation plan 3.7, measurement 3 from the pilot. If spec detail does not predict success, the classifier does not earn its cost, and routing stays on deterministic features.

## 4. Supervision

### 4.1 Shape

One supervisor per supervised run. It starts before the worker, reads the run's room, and dies when the worker's run ends. Its account is `pma-agent`, invited to the run's room only. Its scope is one room, so it cannot carry one project's text into another's run.

A route turns it on:

```json
{"node": "fix", "model": "sonnet", "approval": "each",
 "supervise": {"model": "opus", "max_turns": 6}}
```

### 4.2 Process: a second broker on the run's room

The supervisor is a model CLI under its own `minosb`, on the same room as the worker's:

```text
minosb -user pma-agent -room {room} -task {brief} -quiet PASS -max-turns {max_turns} \
  -socket {sup_socket} -events {sup_events} -- \
  claude -p --input-format stream-json --output-format stream-json --verbose \
    --restricted --tools "Read,Grep,Glob" --strict-mcp-config \
    --model {model} --append-system-prompt-file {supervisor_prompt}
```

The two brokers relay each other:

1. The worker's turn ends. Its `minosb` posts the final reply to the room.
2. The supervisor's `minosb` pushes it to the supervisor as `worker: <text>`.
3. The supervisor replies with an instruction, or with `PASS`.
4. An instruction is relayed to the room. The worker's `minosb` holds it until the worker's turn ends, then pushes it as `pma-agent: <text>`.

This needs no minos client in pma and no new conversation code. The worker's `-interrupters` lists the developer and `pma-agent`, so the supervisor can stop a turn with `/interrupt <correction>`.

Start order matters. A broker pushes only messages after the room's end when it attached (`attach` in minos `broker.go`). The supervisor must therefore report `ready` before the worker starts, or it misses the worker's first turn.

### 4.3 Tools

`--restricted --tools "Read,Grep,Glob" --strict-mcp-config` leaves the supervisor three read-only file tools, confined to the worktree. It has no Bash, so it cannot run `pma`, `git` or `minosa`. Its only output is its reply, which the broker relays. Escalation is a reply starting `ESCALATE:`, read by the developer in the room.

This flag set is enforcement inside the `claude` process, not an OS boundary. It stops a model that was steered by worker output from acting. It does not stop a compromised binary.

### 4.4 The brief

`{brief}`, the supervisor's first frame, holds:

- the task text and its spec, if a `specify` node wrote one;
- the verify command and the path scope;
- the rules: the developer outranks `pma-agent`; reply `PASS` when nothing needs saying; `ESCALATE:` for what needs a person; never approve.

The worker's own prompt gains one line: a message from the developer outranks one from `pma-agent`. Authors arrive as `name: text` (minos `forPush`).

### 4.5 Ending the conversation

Without limits, the two brokers loop. Each reply starts the other's next turn, so the worker never idles and `-linger` never fires. Two limits are needed in `minosb`:

| Flag | Effect | Size |
|-|-|-|
| `-quiet TEXT` | A turn whose final message is exactly TEXT is not relayed | about 10 lines and a test |
| `-max-turns N` | After N turns, close stdin, as `-linger` does | about 15 lines; reuses the close path |

`PASS` ends an exchange. `max_turns` bounds the supervisor's spend. The worker's `-linger` must exceed the supervisor's reply time, or an instruction arrives after the worker's stdin closed. It is then reported as `undelivered`. The proposed `2m` covers a typical reply of 5-30 s.

### 4.6 pma changes

| Change | Where |
|-|-|
| `supervise` on a route: model and `max_turns` | `route.rs` |
| A `claude-supervisor` worker record, with the template in 4.2 | `worker.rs` |
| Start the supervisor with `spawn_guarded`, wait for `ready`, run the worker, then drop the supervisor's lifeline | `dispatch.rs`, around `run_limited` |
| Invite `pma-agent` when filing the room | `dispatch.rs`, with `minos room ensure` |
| Supervisor cost, from its log's last `result`, added to the run | `store.rs`, `agent.rs` (`parse_claude`) |
| Interventions per run: developer and supervisor messages delivered | `store.rs`; needs authors on minos `turn` events |

The last row is the measurement minos design.md section 5 asks for: "Record interventions per run beside the rest of the run row. That rate decides whether the design works." It needs `minosb` to name the authors in each `turn` event's batch. pma then counts them from the events file, still with no minos client.

### 4.7 Failure modes

| Failure | Effect | Bound |
|-|-|-|
| The two brokers loop | Turns and cost grow until the timeout | `-quiet`, `-max-turns` |
| Worker output steers the supervisor | Bad instructions to the same worker | The supervisor can only read the worktree and talk to this worker. The worker already writes that worktree. Verify still decides |
| The supervisor is wrong | The worker follows a bad instruction | The developer outranks it, and can interrupt |
| The supervisor is down | No supervision | The run proceeds; supervision is advisory |
| Host mode: same uid | The worker can read the supervisor's `MINOS_PASSWORD` from `/proc/<pid>/environ` (issue 5 in [pma-minos-integration.md](pma-minos-integration.md)) and post as `pma-agent` | None on the host. Author names are not evidence there. Only containers (minos 4.3) or a separate uid fix it |
| Cost | A supervisor turn per worker turn; an opus supervisor can cost more than a sonnet worker | `max_turns`; recorded per run |
| A late instruction | It arrives after the worker's stdin closed | Reported `undelivered` |

The host-mode row decides how far 4.2 supervision can be trusted. On the host it is a convenience for an attended run. The author distinction minos D11 needs holds only when the worker is contained.

## 5. Proposals

Proposals are outputs of judgment nodes:

- which workflow a situation gets;
- inputs the deterministic rules cannot read, such as priority implied by prose;
- a revision of the routing policy.

A node writes a unit, and an `emit` records it as pending. A person activates it, and `pma workflow activate` records who (W15). No live agent is needed, and nothing reaches minos. A minos submission becomes useful only if approvals should leave the host (minos option 4.1).

## 6. Build order and gates

| Step | Builds | Gate |
|-|-|-|
| 1 | Judgment nodes: `specify`, the 3.7 classifier, a review summary | Measurement 3 from the pilot |
| 2 | minos option 4.2: `room ensure`, the `claude-minos` template, the room id on the run row | Decision 1 in [pma-minos-integration.md](pma-minos-integration.md): are mid-run corrections needed? |
| 3 | `minosb -quiet`, `-max-turns`, authors on `turn` | Step 2 |
| 4 | Supervision: the route field, the supervisor template, and dispatch | Intervention data from step 2 shows corrections a model could have made |
| 5 | Contained supervision | minos 4.3 and grants |

Step 4 is the first point where `pma-agent` exists as a principal. Steps 1 and 3 are useful without it.

## 7. Tests

- **minos:** `-quiet` and `-max-turns` against the scripted fake agent in `relay_test.go`. Plus a two-broker test: two fakes on one room exchange turns until `PASS`, and stop at `max_turns`.
- **pma:** a fake supervisor beside `FAKE_CLAUDE` in `crates/pma/tests/cli.rs`, against a test `minosd`. Check that:
  - the supervisor starts before the worker;
  - its cost is added to the run;
  - it is killed when the worker ends;
  - a route without `supervise` starts no supervisor.
- **Real:** a `make host-run` variant with a real supervisor, to find whether `PASS` is reliable and how many turns a run takes.

## 8. Alternative framings

- **No supervising principal.** The developer is the supervisor, through option 4.2, and every judgment is a workflow node. `pma-agent` then names a routing role, not an agent. This is the default until step 4's gate passes, and may be the end state.
- **An interactive head.** design-review "Manager architecture" proposes exposing `pma` over MCP so any agent session can act as the manager while `pma` keeps the gates. That is a person's session with a model in it, not an autonomous supervisor. It needs an MCP api over `pma_core::api` and no minos at all.

## 9. Open questions

1. Is `PASS` reliable enough as an empty reply? If the model writes prose around it, `-quiet` needs a prefix match, which a worker cannot exploit, since only the supervisor's broker applies it.
2. Should the supervisor see the worker's trace, or only its final replies? Final replies keep the room readable (minos D15). The trace would add tool calls and cost.
3. Who supervises a lap of `apply-fixes`: one supervisor per run, or one per workflow instance? Per run matches D10 and the room grain question.
4. Does a supervised run use `approval: each`, or can supervision relax approval? This note assumes it cannot. A supervisor that relaxes approval holds authority W15 denies it.
