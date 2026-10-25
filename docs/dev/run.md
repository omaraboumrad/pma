# pma run

`pma run <name> <project|path>...` runs a named job in one command. It is a front end over `pma workflow run` and `pma dispatch`. It adds no behaviour of its own: it picks names, targets, a model and whether to stop at the plan, then calls one of the two.

## Names

| Name | Runs | Model when nothing names one |
|-|-|-|
| `review` | the library's `find-issues` | `claude` `opus` |
| `ensemble-review` | `ensemble-review` | `opus` |
| `doc-drift` | `doc-drift` | `opus` |
| `todo-audit` | `todo-audit` | `opus` |
| `security-review` | `security-review` | `opus` |
| `test-gaps` | `test-gaps` | `opus` |
| `review-and-fix` | `review-and-fix` | `opus` |
| `project-brief` | `project-brief` | the settings |
| `triage-issues` | `triage-issues` | the settings |
| `fix-ci` | `pma dispatch <project>:ci` | the settings |
| `deps-upgrade` | `pma dispatch <project>:deps` | the settings |

Any other workflow in the revision in effect runs by its own name. Bare `pma run` lists them all with the type each reads, from the bundled library before any revision is in effect. A workflow whose input no target can name, such as `record`, is named as not runnable from here.

`fix-ci` and `deps-upgrade` are dispatches, not workflows. A dispatch's prompt carries the failing CI logs, or the outdated packages and any failed `pma deps refresh`. A workflow `edit` node gets only its unit's fields, which for a `ci` signal are the workflow names.

## Where a fix lands

`fix-ci` and `deps-upgrade` dispatch with `--local`: each run starts from the clone's working tree, and an edit that passes verify is written into the clone unstaged. One that fails verify, changes nothing, or has no verify command is discarded; its patch stays in the run's log directory. `--review` leaves each edit in `pma review` instead.

A local `ci` pick reads its logs from the default branch, where CI failed, not from the branch the clone has checked out. The edit still applies to the working tree as it is.

## Model

Reviews ask for `opus`. In the pilot, two haiku reviews made 10 findings and none was true (one partly); two Opus reviews made 24 and all were true (`workflow-pilot.md`). Two instances each is little evidence, but the gap is wide. The catalog model applies only when no `-w` is given, no route policy is applied, no default preset is set, and the default worker is `claude`. Anything you configured wins.

## The bundled library

With no revision of your own in effect, the first `pma run` of a workflow proposes and activates `pma workflow library`, recorded as by `pma run`. A later `pma` that ships a different library replaces it the same way. A revision you activated is never replaced.

The library goes in only within `workflow_budget`, as any revision does. Its worst case is $63 per project (`ensemble-review` at `agent_budget` $1), over the default of $25, so the first run asks for `pma config workflow_budget 63`. Activation is the point where a spending ceiling is agreed; `pma run` does not agree to it for you.

## The gate

Named targets run unattended: activation already set what one project may cost. `--all` and `--tag` print the plan and the worst case, and run only with `--yes`. A gate on the worst case against `batch_budget` was rejected: every review's worst case ($41 for one project) exceeds the default $5, so nothing would ever run unattended.

## Not built

- `--local` for workflows, and `--detach`; `pma workflow run` has both.
- Registering a path outside every root. The error names the `pma root add` to run.
