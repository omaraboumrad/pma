# gh calls

Review, 2026-11-14. It lists every `gh` call pma makes, measures the hot ones, and records which optimizations were built.

## Call sites

| Call site | Command | When |
|-|-|-|
| `scan::ci_state` | `workflow list`, `run list --limit 100`, `run list --workflow <id>` | every scan; every 30 min with a front end open (`scan.every`); before a CI dispatch |
| `dispatch::prepare::failed_ci_logs` | `run view --log-failed` | once per failing workflow, per CI dispatch |
| `publish::settle` | `pr view` | once per `pr-open` run, on every dispatch and `y` |
| `publish::conflicting`, `publish::merge` | `pr view`, `pr merge` | `pma merge`, `F` |
| `publish` (open a PR) | `pr list`, `pr create` | `pma pr` |
| `pass` workflow checks | `issue list`, `pr view` | workflow nodes `open-issues`, `pr-merged`, `ci-green` |
| `sync` | `api user`, `issue list`, `issue create`, `issue edit`, `label create` | `pma sync` |
| `main` (`pma root clone`) | `repo clone` | by hand |

Only the scan runs without a person acting. The other call sites are left as they are.

## Measured

One full scan of 117 GitHub projects, through a `gh` wrapper that logs each call. 8 scan threads.

| | Before | After, first scan | After, second scan |
|-|-|-|-|
| `workflow list` | 117 | 117 | 117 |
| `run list`, window | 117 | 88 | 88 |
| `run list --workflow` | 77 | 18 | 0 |
| Calls | 311 | 223 | 205 |
| Wall clock | 38 s | 26 s | 24 s |

Average call time: `workflow list` 0.47 s, `run list` 1.03 s. `gh --version` takes 24 ms, so the time is in the network. The rate limit read 3993/5000 after the first scan. The CI verdicts matched the scan before for all 117 projects.

## Built

1. **No `run list` without an active workflow.** It cannot find a run. This saved 29 calls.
2. **The per-workflow reads of a project run in parallel.** Before, one project made 10 of them in sequence, about 10 s.
3. **A per-workflow read is cached in `ci_alone`** (`scan::Alone`).
   - When the window returns fewer than 100 runs, it holds every run on the branch. A workflow missing from it has no decisive run, so no call is made. This removed 59 of the 77 calls on its own.
   - Otherwise a cached read is reused while the window's oldest run is no newer than the newest run at the cached read (`through`, GitHub's clock). Any run of the workflow started since would then be in the window.
   - A cached read expires after 24 h (`ALONE_FOR`). A re-run of an old run outside the window starts no new run, so only the expiry catches it.
   - A failed read keeps the last cache, as it keeps the last CI.

## Not built

- **16 scan threads instead of 8.** It might halve the wall clock. The scan before had one connection reset at 8 threads, so measure first.
- **One GraphQL query for `settle`.** Worth it only with several `pr-open` runs; there were 0.
- **Caching `workflow list`.** A workflow added on the default branch would go unseen. The local checkout cannot tell: playr's `dev` lacks `audit.yml`, which `main` has.
- **Calling the REST API without `gh`.** It needs an HTTP dependency, and process start is 24 ms of a 0.5-1 s call.
- **Reading CI from the head commit's check suites.** A workflow with path filters, such as playr's `audit`, has no suite on most commits. Its last result would be lost.
