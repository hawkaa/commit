---
title: Auto memory not consulted during parallel worktree sprint — CI gate parity gap
date: 2026-04-13
category: workflow-issues
module: development_workflow
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - Running parallel worktree agents that merge independently to main
  - Sprint velocity prioritizes speed over pre-merge checklists
  - Auto memory contains workflow standards that agents do not actively consult
  - CI runs gates that are not fully documented in CLAUDE.md's testing section
related_components:
  - rust-toolchain
  - github-actions
  - claude-code-auto-memory
tags:
  - cargo-fmt
  - parallel-worktrees
  - auto-memory
  - pre-push-gate
  - ci-drift
  - workflow-parity
  - rust
  - claude-code
---

# Auto memory not consulted during parallel worktree sprint — CI gate parity gap

## Context

On 2026-04-13, a Phase 3 sprint shipped 4 features across 4 parallel worktree branches, merged sequentially to `main`. Each worktree agent ran `cargo test` and `cargo clippy -- -D warnings` before committing — the two gates listed in CLAUDE.md's `## Testing` section at the time. All branches passed local verification. After `git push origin main`, the GitHub Actions Deploy workflow failed within approximately 30 seconds on the `cargo fmt --check` step in `.github/workflows/deploy.yml`. Six files had accumulated formatting drift.

This was not a new failure. The same `cargo fmt --check` failure had been recurring on every push to `main` since 2026-04-12, silently blocking the Fly.io auto-deploy for roughly 24 hours. Earlier commits on 2026-04-12 — including `fix: resolve merge conflicts and adapt L2 tests for cache invalidation` (`724720a`) and `chore: mark Phase 2 items as complete in CLAUDE.md` (`8ef4d89`) — had triggered identical Deploy failures that went unnoticed because feature delivery was the focus, not CI status.

The root cause was a gate-parity gap between CI and the documented local verification standard. The Deploy workflow ran three sequential checks: `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test`. CLAUDE.md's `## Testing` section documented only the latter two. Every worktree agent in the Phase 3 sprint received task prompts derived from CLAUDE.md and faithfully ran exactly those two gates. The third gate — `cargo fmt --check` — already existed as a documented standard in the auto memory file `~/.claude/projects/-Users-hakon-code-commit/memory/feedback_rust_workflow.md`, which stated: "Never merge code that doesn't pass `cargo clippy` and `cargo fmt --check`" (auto memory [claude]). That memory entry was 2 days old at the time of the incident but was not consulted during sprint dispatch or execution.

## Guidance

Before pushing any Rust changes to `main` in this repository, run all three CI gates locally, in the same order CI runs them:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

If `cargo fmt --check` reports drift, run `cargo fmt` to fix it, then re-run the check. The order mirrors the "Rust checks" job in `.github/workflows/deploy.yml` — `cargo fmt --check` is the first step, and a failure there blocks both subsequent steps and the downstream Fly.io deploy.

**Meta-rule for agent-dispatched sprints.** Before dispatching parallel worktree agents, consult the auto memory feedback entries at `~/.claude/projects/<project>/memory/` for any workflow or quality standards that may not yet be reflected in CLAUDE.md. The verification checklist in each dispatched agent's prompt must include every gate CI runs — not a local subset. A 30-second diff between CLAUDE.md's testing section and the CI workflow's test job, run once before dispatch, prevents the entire class of gate-parity failures from recurring across N parallel branches.

## Why This Matters

**CI was silently broken for ~24 hours.** Between 2026-04-12 and 2026-04-13, every push to `main` failed the Deploy workflow at the `cargo fmt --check` step. The Fly.io auto-deploy was blocked the entire time. Because the failure was in formatting — not tests or clippy — it was easy to overlook, and the issue was not noticed until the Phase 3 sprint push also failed.

**Feedback memory already specified the gate — memory compounds only when consulted.** The auto memory entry in `feedback_rust_workflow.md` (auto memory [claude]) explicitly required `cargo fmt --check` as a merge gate. The entry was created on 2026-04-11, two days before the incident. Its value was effectively zero because no orchestrator or dispatched agent read it during the sprint. This demonstrates that documented knowledge has no effect unless it is actively consulted at the point of decision — in this case, at prompt-writing time for worktree agents.

**Pre-push gate parity is the cheap-to-run equivalent of a post-merge canary.** `cargo fmt --check` takes under 2 seconds on this workspace. Catching formatting drift locally costs nearly nothing. Catching it in CI costs a 30-second feedback loop per push, plus the cognitive overhead of diagnosing a remote failure and re-pushing. For a solo founder, every failed push-fix-push cycle is a context switch that breaks flow.

**Multi-agent parallel sprints amplify the cost.** When N agents each follow the same incomplete checklist, all N branches accumulate the same class of drift. In this incident, 4 worktree agents each independently carried forward or introduced formatting issues across 6 files. The cost is not 1× the fix — the probability that at least one branch has drift approaches 1.0 as N grows, making the post-merge failure nearly guaranteed. Fixing the checklist once prevents the failure across all future agents.

## When to Apply

- **Any Rust work in this repository.** This is a hard requirement from the project's feedback memory (auto memory [claude]). All three gates (`cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`) must pass before pushing to `main`.
- **Any agent-dispatched sprint where the fix agent's verification checklist is written upstream.** The dispatching orchestrator must ensure the checklist in each agent's prompt includes every gate from `.github/workflows/deploy.yml`. If CI adds a new gate (for example `cargo deny check`), dispatched agent prompts must be updated in the same session — not in a future session.
- **Before dispatching parallel work.** Diff the gates in CLAUDE.md's `## Testing` section against the steps in the CI workflow's test job. Any discrepancy must be resolved before dispatch. This audit takes under 30 seconds and prevents the entire class of gate-parity failures.
- **When inheriting an auto-memory-equipped project for the first time.** Read `~/.claude/projects/<project>/memory/MEMORY.md` and every file it references as the first orientation step. Feedback memories carry workflow requirements that may not yet be reflected in repo-level docs.

## Examples

### Before (gate-parity gap)

CLAUDE.md `## Testing` section:

```bash
cargo test
cargo clippy -- -D warnings
```

Worktree agent prompts specified these two gates. Each agent ran them locally. All passed. Push to `main` failed in 30 seconds at `cargo fmt --check` in the "Rust checks" job. CI had been silently broken since 2026-04-12.

### After (gate parity restored)

CLAUDE.md `## Testing` section (commit `d0ff6f1`):

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

With a note: "All three gates must pass before pushing to `main`. CI (`.github/workflows/deploy.yml`) runs `cargo fmt --check` as part of the 'Rust checks' job, and a failure there blocks the Fly.io auto-deploy. Run `cargo fmt` locally to fix formatting drift."

Push passes in 7m44s (full CI including Fly.io deploy to both `commit-backend` and `commit-verifier`).

### The actual "Rust checks" job from `.github/workflows/deploy.yml`

```yaml
jobs:
  test:
    name: Rust checks
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.94.0
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check      # the missed gate
      - run: cargo clippy -- -D warnings
      - run: cargo test
```

The job installs `rustfmt` and `clippy` components explicitly, then runs all three checks sequentially. `cargo fmt --check` is the first check — a failure there blocks `clippy`, `test`, and the downstream `deploy-backend` / `deploy-verifier` jobs (both `needs: test`).

### Enrichment recommended for the parallel-worktree best-practices doc

The companion doc `docs/solutions/best-practices/parallel-worktree-agent-workflow-2026-04-12.md` currently lists test commands in section 3 ("Write self-contained agent prompts") as `cargo test`, `cargo clippy -- -D warnings`. That doc should be amended to state:

> Dispatched agent prompts must mirror the full CI gate list, not a local subset. Before writing prompts, diff the test commands in CLAUDE.md against the steps in the CI workflow (for example `.github/workflows/deploy.yml`). Any gate present in CI but absent from the prompt will be missed by every parallel agent — and the cost scales linearly with the number of agents dispatched.

This would have prevented the Phase 3 incident: the orchestrator would have noticed `cargo fmt --check` in CI but not in the prompt, and added it before dispatch.

## Related

- `docs/solutions/best-practices/parallel-worktree-agent-workflow-2026-04-12.md` — the pattern this incident occurred within. High overlap on problem context; different solution proposed. Consider running `/ce:compound-refresh` on that doc to add the gate-parity amendment.
- Auto memory: `~/.claude/projects/-Users-hakon-code-commit/memory/feedback_rust_workflow.md` — the feedback entry that already specified `cargo fmt --check` as a required gate (auto memory [claude]).
- `CLAUDE.md` `## Testing` section — the canonical pre-push gate list, updated in commit `d0ff6f1` (2026-04-13) to include `cargo fmt --check`.
- `.github/workflows/deploy.yml` — the authoritative CI gate definition. Local gates must mirror this file.
