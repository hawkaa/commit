---
title: Parallel feature development with git worktree agents
date: 2026-04-12
category: best-practices
module: development_workflow
problem_type: best_practice
component: development_workflow
severity: medium
applies_when:
  - "Multiple independent features or work items are ready to implement simultaneously"
  - "Work items have no shared file dependencies or merge conflicts between them"
  - "Each feature can be built and tested in isolation"
  - "You want to maximize throughput in a single conversation or sprint"
related_components:
  - tooling
  - testing_framework
tags:
  - git-worktrees
  - parallel-agents
  - ai-development
  - worktree-isolation
  - code-review
  - rust
  - claude-code
---

# Parallel feature development with git worktree agents

## Context

Solo founders and small engineering teams using AI agents face a throughput bottleneck when implementing multiple independent features: dispatching agents sequentially wastes wall-clock time when the features have no shared file dependencies. Git worktrees solve the isolation problem — each agent operates in its own checkout with no file locking or merge conflicts. The challenge is coordinating the planning, dispatch, review, and fix phases so that parallel execution stays safe and the results land cleanly.

This pattern was validated on the Commit project (Rust + Chrome Extension): three Phase 2 features — network keyring, L2 attestation pipeline, and Commit Score v2 — were planned, executed, reviewed, and fixed in parallel, completing in under 12 minutes per branch with full test coverage.

## Guidance

### 1. Audit first, plan second

Before selecting features for parallel dispatch, verify what is actually open. Check `git log` and the codebase against CLAUDE.md or any tracking document. Stale checklists cause wasted agent runs. In this session, `git log --oneline -30` revealed that CI/CD, the E2E endorsement flow, and the entire security hardening batch were already shipped — the CLAUDE.md was stale by 6 items.

### 2. Analyze file dependencies before committing to parallel dispatch

For each candidate feature, list the files each plan will touch. If two plans share a file (e.g., `endorsement.rs`, `mod.rs`), they cannot run in parallel without producing merge conflicts. Resolve by running one first or splitting the scope. Only dispatch in parallel when the file sets are disjoint.

In the Commit session, the three selected features had this overlap profile:

| Plan | Primary files | Conflicts with |
|------|--------------|----------------|
| Network keyring | `db.rs` (new column), new `network.rs`, extension popup | Score v2 (minor: both touch `endorsement.rs`) |
| L2 attestation | new `l2.rs`, `Cargo.toml` (alloy), `main.rs` (background task) | None |
| Score v2 | `score.rs`, `trust_page.rs`, `content-github.ts` | Network keyring (minor) |

The minor overlap on `endorsement.rs` was assessed as trivial (one adds a field, the other adds a cache invalidation call) — merge conflicts would be one-line resolutions.

### 3. Write self-contained agent prompts

Each dispatched agent has no memory of the planning conversation. Every prompt must include:

- The path to the plan file (agents read it themselves)
- Test commands (`cargo test`, `cargo clippy -- -D warnings`)
- Stack description (language, framework, key constraints)
- Implementation guidelines specific to that branch
- Instructions to commit each unit with conventional commit messages

Agents that receive incomplete context produce incomplete or incorrect results.

### 4. Dispatch all agents simultaneously, then wait

Use the `Agent` tool with `isolation: "worktree"` and `run_in_background: true` for each feature. Fire all in the same orchestration turn. The orchestrator is freed to continue other work (updating CLAUDE.md, drafting review prompts) while agents execute.

### 5. Review each branch independently with structured reviewer personas

After all branches complete, dispatch one review agent per branch. Give each reviewer a correctness-focused persona and request findings in a structured format (JSON with P0/P1/P2/P3 severity tiers). Per-branch review prevents cross-contamination of findings and produces actionable, branch-specific fix lists.

### 6. Fix in the worktree, then merge sequentially

Dispatch fix agents into the same worktree branches that were reviewed. Each fix agent receives the branch-specific findings and commits fixes atomically. Merge branches into main one at a time — not in parallel — to allow conflict resolution if any cross-branch issues surface.

## Why This Matters

Parallel worktree dispatch compresses a multi-day sequential implementation into a single session. For a solo founder, this means shipping three features in the time it previously took to ship one. The correctness of the approach depends on the file-dependency analysis step: skipping it converts a speedup into a debugging session.

The review phase is equally non-negotiable — agents produce working code but not necessarily production-safe code. In this session, structured per-branch review surfaced real issues:

- **P0**: batch revert bug in L2 attestation (entire batch marked as done when one item reverted)
- **P1**: sentinel tx_hash value leaked to trust card API as "on-chain" status
- **P1**: personalized network data cached in shared trust-card cache key

All were fixed by parallel fix agents before merge.

## When to Apply

- Three or more features are ready to implement simultaneously with non-overlapping file sets
- Each feature has a plan document or scope clear enough for an agent to execute without clarification
- Test suites can run independently per branch (`cargo test` or equivalent)
- The developer wants to maximize throughput in a single working session
- Features are independent enough that correctness review can be done per-branch

Do not apply if: features share core files (use sequential dispatch instead), a feature lacks a clear plan (write the plan first with `ce:plan`), or the merge order has constraints that make parallel branches risky.

## Examples

### Commit project — Phase 2 parallel sprint (2026-04-12)

Three features selected after file-dependency analysis confirmed disjoint file sets:

| Feature | Duration | Tests | Commits | Review findings |
|---------|----------|-------|---------|-----------------|
| Commit Score v2 | 7.4 min | 88 pass | 4 | 1 P3 |
| Network keyring + key sharing | 8.9 min | 60 pass | 5 | 1 P1, 1 P2, 3 P3 |
| L2 attestation pipeline | 11.1 min | 88 pass | 4 | 1 P0, 2 P1, 2 P2, 1 P3 |

Total wall-clock time from plan creation to all fixes committed: ~30 minutes for 13 implementation units across 3 branches.

The full workflow in this session:

```
ce:plan (3 plans) → 3 parallel worktree agents → 3 parallel review agents → 3 parallel fix agents → ready to merge
```

Each phase dispatched all agents simultaneously. The orchestrator waited for completions between phases, synthesized results, and dispatched the next phase.

## Related

- `docs/plans/2026-04-12-005-fix-security-hardening-batch-plan.md` — a plan whose 5 units were designed for parallel execution (same parallelizability reasoning applied within a single plan)
- `docs/plans/2026-04-12-006-feat-network-keyring-key-sharing-plan.md` — one of the three plans executed in this parallel sprint
- `docs/plans/2026-04-12-007-feat-l2-attestation-submission-plan.md` — one of the three plans executed in this parallel sprint
- `docs/plans/2026-04-12-008-feat-commit-score-v2-plan.md` — one of the three plans executed in this parallel sprint
