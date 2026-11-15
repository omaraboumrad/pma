# CI verdict for landed runs

Built 2026-11-15, schema 52. Proposed and decided the same day.

Before this, a fix-CI run (`task_key = 'ci'`) was judged only by the project's local verify command. pma never read CI for the commit a run landed. Now each landed run gets a verdict from CI on that commit.

## What happened before

| Step | Headless | Interactive (`I`) |
|-|-|-|
| Prompt holds `gh run view --log-failed` tails | yes (`dispatch/prepare.rs:616`) | yes |
| Verify at the base commit | yes (`dispatch/prepare.rs:304`) | skipped |
| Verify after the agent exits | yes (`dispatch/attempt.rs:607`) | yes, after the pane exits |
| `verify_ok = false` blocks landing | yes, in `stage` (`publish.rs:227`) | no: `settle_interactive` moves a hand-committed run to `committed` without reading it |
| CI read for the landed commit | no | no |

On all 37 fix-CI runs to date the verify command was `make test`, run on macOS. It does not run the workflow that failed. `ci-green` returned `unknown` for any run pushed without a pull request.

## Evidence

Backfilled 2026-11-15 by hand: for each of the 19 `pushed` fix-CI runs, `gh run list --commit <sha>`, filtered to runs whose `headSha` is that commit.

| Result on the landed commit | Runs |
|-|-|
| Every workflow that ran succeeded | 15: #2 #11 #12 #13 #14 #19 #20 #21 #24 #25 #26 #27 #41 #43 #45 |
| A workflow failed | 1: #44 (gen-dsp `CI`; still failing) |
| No workflow ran on that commit | 3: #18 #28 #42 |

#44 landed with `verify_ok = 0` and failed CI, and pma showed neither. #26 and #27 also landed with `verify_ok = 0`, and CI passed both. A gate on local verify would have blocked two good fixes.

## Design

### Recorded

- `ci_before`: the workflows failing at dispatch, one per line, read from the project row in `insert_run`. It is empty when CI passed, and NULL when CI was unknown or the project has no workflows. Such a run is not judged.
- `landed_sha`, `landed_at`: set where a run enters `pushed`, by `pma push` or by `settle_committed` after a push by hand.
- `ci_verdict`: set once, during a scan. It is never revised.

### Judged

`scan::ci_state` now asks the window's `gh run list` for `headSha`, so it makes no extra call. `save_scan` judges each landed run still waiting (`ci_verdict::judge_landed`):

- **Fix-CI run.** Each workflow in `ci_before` is judged on its newest non-neutral run on `landed_sha`. If the commit has no run, the earliest decisive run on a later commit stands in (`git merge-base --is-ancestor`). The verdict is `fixed` when all of them passed and `still-failing` when one failed.
- **Any other run.** Only runs on `landed_sha` count. A later commit could be someone else's. The verdict is `regressed` when a workflow not in `ci_before` failed, and `clean` when every workflow there finished without that. A hand push can bundle other commits, so `regressed` means "this landing broke CI", not "this diff broke CI".
- **Either.** The verdict is `no-run` after 7 days without a decision (`ci_verdict::WAIT`).

### Shown

- `pma report`: a second summary line, `CI after landing: ...`, with fix-CI runs counted apart. Verdicts are in no share. "Accepted" stays the reviewer's decision, so existing figures stay comparable.
- Run detail (`ci after`): the verdict and the short SHA.
- Inbox, "Waiting on you": a project's last landed run with `still-failing` or `regressed`, while that project's CI fails. It leaves when CI passes or another run lands. No dismiss action is needed.
- Activity rows: a bad verdict in the `doing` column.
- The CI signal's task detail: `last fix #N landed: <verdict>`. It is seen before another fix is paid for. A still-failing run is not reopened; the next scan's signal is the retry.
- `ci-green` for a run pushed without a pull request. `fixed` passes. So does `clean`, but only when `ci_before` was empty: "broke nothing" is not "green". A bad verdict fails it, and a run still waiting is not ready.

### Not covered

- Runs that land as a merged pull request. `ci-green` reads the pull request's checks for those already.
- Runs landed before schema 52, which lack `landed_sha` and `ci_before`.
- A commit squashed or rebased on the way. `settle_committed` does not recognise it either (`publish.rs:854`).
- A target workflow that falls out of the 100-run window before deciding. It becomes `no-run`.

## Alternatives considered

- **Push a branch and wait on CI before `ready`.** It catches a bad fix before the default branch. It also publishes unreviewed code, which matters on public repositories.
- **Run the failing job locally as verify** (its `run:` steps, or `act`). It covers Linux and macOS jobs only. These rejections were for Windows or Linux jobs: #35 #37 #38 #39 #40.
- **Gate `settle_interactive` on `verify_ok`.** Rejected on the evidence above.
