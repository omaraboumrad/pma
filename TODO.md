# TODO

## Critical

## High

## Medium

- [ ] A project whose name contains a dot can take `projects.<name>.verify` #agent

- [ ] `pma review 3 4 --approve --minutes 5` drops the minutes #agent

- [ ] `pma prune --apply` exits 0 when it skips a file for lint errors #agent

- [ ] `pma project export` writes a CSV that does not read back when a tag holds a comma or a project is named `#x` #agent

- [ ] One failed dependency tool is counted as zero outdated when another succeeds #agent

- [ ] A code fence indented 1-3 spaces does not hide its content from the TODO.md parser #agent

- [ ] `pma merge <id>...` merges a run's pull request and settles it #agent Today a `pr-open` run is merged on GitHub by hand, and the next `pma review` settles it. `pma merge` runs `gh pr merge <url> --squash --delete-branch` for each named `pr-open` run, then settles it to `merged` at once, as `publish::settle` does for a merge it observes. Refuse a run that is not `pr-open`. Refuse one whose pull request has failing or pending checks; allow one with no checks. Refuse one whose head is no longer the commit pma pushed, since that merges commits no one approved: record the pushed commit at `pma pr`, which today records only the URL. Tests in `tests/cli.rs` with the fake `gh` (`FAKE_GH_PR`): a merge, each refusal, and the run merged afterwards.

- [ ] **Render `gh:N` as a link**: `report`, `rank`, `matrix` and `tui` print a bare issue number, and the project row now stores `owner/name` to resolve it against.

- [ ] The process-group kill does not reach a child that calls `setsid` (`src/agent.rs:105-106`, `119`). `docs/dev/design.md:377` says no agent outlives the session. A survivor's writes to the worktree no longer reach an approval or a publish: an approval must match the tree last shown, and publishing starts from the approved tree. It still runs, reads what the user can, and writes outside the worktree. A cgroup or PID namespace would bound it; `sanduk` does. (REVIEW.md M10)

- [ ] Record each run's agent session id, so `resumes` does something
  `Worker::resumes` is stored, settable and listed by `pma agent`, but no dispatch path reads it (`crates/pma-core/src/worker.rs:210`). `opencode()` sets it `true`, so the listing claims a feature pma lacks. A `{session}` placeholder in worker args (`claude --session-id <uuid>`, as wallfacer does) plus a session column on the run would let a rework continue the earlier session instead of re-prompting, and let `pma review N --open` hand the terminal to `claude --resume <id>` in the worktree. Takeover must run under the `agent::restrict` environment, or it gains push credentials. Open: does a `-p` session resume interactively; does a `sanduk` transcript survive its container; what is opencode's flag. If neither use is wanted, delete the field.

## Low

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
