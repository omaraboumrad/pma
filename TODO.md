# TODO

## Critical

## High

## Medium

- [ ] A project whose name contains a dot can take `projects.<name>.verify` #agent

- [ ] `pma review 3 4 --approve --minutes 5` drops the minutes #agent

- [ ] `pma config <retired key>` says the key is unknown instead of explaining the retirement #agent

- [ ] `pma project import` refuses a CSV saved with a byte-order mark #agent

- [ ] A pull request closed without merging does not count as an attempt on its task #agent

- [ ] `pma dispatch --retry` resets the attempt count before it knows the run started #agent

- [ ] `pma merge <id>...` merges a run's pull request and settles it #agent Today a `pr-open` run is merged on GitHub by hand, and the next `pma review` settles it. `pma merge` runs `gh pr merge <url> --squash --delete-branch` for each named `pr-open` run, then settles it to `merged` at once, as `publish::settle` does for a merge it observes. Refuse a run that is not `pr-open`. Refuse one whose pull request has failing or pending checks; allow one with no checks. Refuse one whose head is no longer the commit pma pushed, since that merges commits no one approved: record the pushed commit at `pma pr`, which today records only the URL. Tests in `tests/cli.rs` with the fake `gh` (`FAKE_GH_PR`): a merge, each refusal, and the run merged afterwards.

- [ ] **Render `gh:N` as a link**: `report`, `rank`, `matrix` and `tui` print a bare issue number, and the project row now stores `owner/name` to resolve it against.

- [ ] The process-group kill does not reach a child that calls `setsid` (`src/agent.rs:105-106`, `119`). `docs/dev/design.md:377` says no agent outlives the session. A survivor's writes to the worktree no longer reach an approval or a publish: an approval must match the tree last shown, and publishing starts from the approved tree. It still runs, reads what the user can, and writes outside the worktree. A cgroup or PID namespace would bound it; `sanduk` does. (REVIEW.md M10)

## Low

- [ ] **Identity by slug rather than by directory name**: `discover` skips a repository whose basename another root already supplied, so two roots cannot both hold a `py`. Worth doing when a second root exists, not before.

- [ ] `prune` deletes an open checkbox nested under a finished item; the dry run lists only the parent (`src/todo.rs:636`, `src/main.rs:952`). (REVIEW.md L1)

- [ ] `sync::gh`, `publish::gh_in` and `wt_git` have no timeout, so a stalled fetch holds the session lock (`src/sync.rs:196`, `src/publish.rs:351`, `src/dispatch.rs:168`). Reuse the bound `scan::call` has. (REVIEW.md L2)

- [ ] `forget_project` leaves `projects.<name>.verify` (`src/store.rs:1029-1036`). A later unrelated repository of the same name runs the old shell command. (REVIEW.md L3)

- [ ] `home()` accepts an empty or relative `PMA_HOME` (`src/store.rs:617-623`), so the database and session lock depend on the working directory. `state_home()` already filters empty values. (REVIEW.md L4)

- [ ] `user_version` is read before the migration transaction opens (`src/store.rs:658-693`). Two processes starting after an upgrade both replay the steps; the second fails with "already exists". (REVIEW.md L5)

- [ ] `Slot::Count` has no upper bound; `timeout * 60` wraps in release and panics in debug (`src/config.rs:263`, `src/publish.rs:154`). (REVIEW.md L6)

- [ ] CSV export does not round-trip a tag containing a comma or a project named `#x` (`src/projects.rs:42-112`). (REVIEW.md L7)

- [ ] `project import --apply` and `project tier` apply many rows without a transaction (`src/main.rs:1196-1227`). (REVIEW.md L8)

- [ ] The agent log is read as strict UTF-8; one invalid byte gives an empty string, a failed run and no cost (`src/dispatch.rs:1028-1031`). (REVIEW.md L9)

- [ ] `parse_claude` takes the last line that parses as JSON, then checks its type, so a later JSON line on stderr hides the result (`src/agent.rs:182-186`). (REVIEW.md L10)

- [ ] `pma pr` pushes `run.branch`; if the agent switched branches, the commit is elsewhere (`src/publish.rs:195`). (REVIEW.md L11)

- [ ] The publish tick uses `std::fs::write` (`src/publish.rs:138`); every other TODO.md writer uses the atomic `todo::save`. (REVIEW.md L12)

- [ ] TODO.md writers share the temp name `.TODO.md.pma-tmp` (`src/todo.rs:758`), and `prune` and `lint --ids` do not re-read before saving. (REVIEW.md L13)

- [ ] Errors in `lint`, `prune`, `sync` and publish go to stdout, and `prune --apply` on a file with lint errors exits 0 (`src/main.rs:809-933`, `3405`, `3476`). (REVIEW.md L14)

- [ ] One failed dependency tool is counted as zero outdated when another succeeds (`src/deps.rs:38-47`). Open: does `cargo update --dry-run` ever print lines `parse_cargo` (`src/deps.rs:110`) misses? (REVIEW.md L15)

- [ ] A leading byte-order mark makes the whole TODO.md a lint error with no stated cause (`src/todo.rs:226`). (REVIEW.md L16)

- [ ] The route example at `README.md:78` has no `approval` and is refused by the parser (`src/route.rs:358`). (REVIEW.md L17)

- [ ] Smaller review items, unverified: run scope stored comma-joined (`src/store.rs:1149`); a code fence indented 1-3 spaces does not hide its content (`src/todo.rs:205`); `gh:N` cast from `u64` to `i64` (`src/scan.rs:404`); the CI prompt has no byte cap and can exceed the argument limit (`src/dispatch.rs:726`); cost over budget is judged on cumulative cost across reworks (`src/accept.rs:91`); table widths count characters, not display columns. (REVIEW.md)

- [ ] `docs/dev/design.md` drifts from the code: line 463 names `serde` and `toml`, absent from `Cargo.toml`; line 46 lists `codex` and `cursor-agent`, the README lists `claude`, `opencode`, `omp`. (REVIEW.md, Design)
