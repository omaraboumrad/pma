# Stage, commit, push

Status: built, 2026-10-21.

## Problem

`pma push N` committed, rebased onto origin, ran the check again and pushed to the default branch, all in the run's worktree. The name covered the last step only. The clone never saw the commit, so the diff could not be reviewed with the user's own tools before it was public.

## Decision

Three commands named after the git steps, acting in the user's clone. Each runs the steps before it that the run has not done.

| Command | Does | From | Run state after |
|-|-|-|-|
| `pma stage N` | `git apply --index` of the run's change, task tick included; removes the worktree | `ready`, `approved` | `staged` |
| `pma commit N` | stage, then `git commit` with pma's message | `approved`, `staged` | `committed` |
| `pma push N` | commit, then push that commit to origin's default branch | `approved`, `staged`, `committed` | `pushed` |

`pma pr N` is unchanged: it publishes from the worktree.

- Staging is the review, so `stage` needs no approval. `commit` and `push` on an unstaged run still need `pma review N --approve`.

- `stage` refuses a ready run whose check failed, and leaves it ready.

## What the clone must hold

- **HEAD at the run's base.** The check ran against that base. A clone behind or ahead of it is refused, with the cause. pma does not pull or rebase the user's branch.

- **An empty index**, for `stage`. A commit would otherwise include the user's staged changes.

- **The staged change unchanged**, for `commit`. The index tree must equal the tree `stage` wrote; it is kept in `runs/<id>/staged.tree`. A different index is the user's to commit.

- **HEAD is the run's commit**, for `push`: its parent is the base and its tree is the staged tree. pma pushes that commit, never other local commits.

`git apply --index` refuses on its own when a file it touches has unstaged edits.

## The publish check is dropped

The old `push` ran the check again on the rebased tree. With the base required to be unmoved, that tree is the one the run's check passed, plus the `TODO.md` tick. A push that origin moved under is rejected by git as non-fast-forward.

Two runs in one project: the second finds HEAD past its base and is refused. Publish it with `pma pr`, or rework it.

## Rejected

- **Stage only, detect the commit by patch-id.** One verb fewer, but the user writes the message and the task stays open until the next scan.

- **Keep publishing from the worktree.** The clone never shows the commit, which was the complaint.
