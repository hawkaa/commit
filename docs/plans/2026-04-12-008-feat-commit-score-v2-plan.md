---
title: "feat: Commit Score v2 — Layer 1 + Layer 2 blending"
type: feat
status: active
date: 2026-04-12
---

# feat: Commit Score v2 — Layer 1 + Layer 2 blending

## Overview

Finalize the Commit Score algorithm to blend Layer 1 (public signals) and Layer 2 (endorsement signals). Most scaffolding exists: `compute_score()` already has the L1*0.3 + L2*0.7 formula, `score_github_repo_with_endorsements()` computes `endorsements` and `proof_strength` fields with status weighting. This plan adds the missing `tenure` field, leaves `network_density` as 0 until the network keyring plan lands, renders the L2 breakdown in the trust page, and surfaces Layer 2 signals in the extension content script.

## Problem Frame

The score today is functionally Layer 2-aware — when endorsements exist, the `has_layer2` flag activates the blended formula. But two of the four Layer 2 fields are always 0:

- `tenure` (max 10 points) — measures how long endorsers have been around. The data exists: endorsement `created_at` timestamps. The computation is missing.
- `network_density` (max 15 points) — measures unique endorsers. Requires `endorser_key_hash` from the network keyring plan. Not implementable until that plan lands.

Additionally, the trust card SSR page (`/trust/{kind}/{id}`) only renders Layer 1 breakdown items (Longevity, Maintenance, Community, Financial). When endorsements exist, the L2 fields are computed but invisible. The extension content script similarly shows only L1 signals.

The CEO plan formula specifies: `tenure: min(avg_endorser_months * 1, 10)`. This measures average endorser age — how long ago endorsers first endorsed *anything*, not just this subject. For Phase 2, a simpler proxy is acceptable: average age of endorsements for *this subject*.

## Requirements Trace

- R1. `tenure` field must be computed from endorsement timestamps: `min(avg_months_since_endorsement, 10)`
- R2. `network_density` must use `endorser_key_hash` data when available, defaulting to 0 when not
- R3. Trust page SSR must render Layer 2 breakdown items (endorsements, proof strength, tenure) when endorsements exist
- R4. Extension content script must display Layer 2 signal indicators when the score includes endorsement data
- R5. The `layer1_only` label must disappear from trust card when Layer 2 data contributes to the score
- R6. Existing Layer 1-only score computation must not change for subjects with no endorsements

## Scope Boundaries

- `network_density` computation — blocked on network keyring plan (006). This plan sets it to 0 and structures the code so wiring in key-hash-based counts is straightforward.
- Score tuning or A/B testing — Phase 3 concern. Hardcoded weights are fine for Phase 2.
- Negative signals (spam penalties, revoked endorsements) — not in scope.
- Business subject scoring (Norwegian registry data) — only GitHub repos have Layer 2 data now.

### Deferred to Separate Tasks

- `network_density` wiring: after network keyring plan (006) lands, a small follow-up connects unique endorser counts to the score
- Score tooltip or explanation UI: detailed breakdown visible on hover/click
- Score history tracking: how scores change over time

## Context & Research

### Relevant Code and Patterns

- `src/models/signal.rs:40-101` — `CommitScore`, `ScoreBreakdown` (all 8 fields), `compute_score()` with L1/L2 blend
- `src/services/score.rs:50-86` — `score_github_repo_with_endorsements()` computes `endorsements` and `proof_strength`, leaves `network_density` and `tenure` at 0
- `src/services/score.rs:10-12` — `VERIFIED_WEIGHT = 1.0`, `PENDING_WEIGHT = 0.3`
- `src/routes/trust_card.rs:134-146` — conditionally calls `score_github_repo_with_endorsements()` when endorsements exist
- `src/routes/trust_page.rs:570-592` — `render_breakdown()` only renders L1 items
- `extension/src/content-github.ts` — `formatSignals()` renders L1 signals only
- `tests/score.rs` — comprehensive L1 and L2 weighting tests

### Score Formula (from CEO plan)

```
LAYER 2 (0-70 points):
  endorsements:    min(count * 5, 30)              // status-weighted, already implemented
  network_density: min(unique_endorsers * 3, 15)   // requires key hashes, deferred
  proof_strength:  avg(confidence) * 15             // status-weighted, already implemented
  tenure:          min(avg_endorser_months * 1, 10) // needs implementation

BLENDED: (L1 * 0.3) + (L2 * 0.7), capped at 100
```

## Key Technical Decisions

- **Tenure uses per-subject endorsement age, not global endorser age.** The CEO plan says "avg_endorser_months" meaning how long endorsers have been in the system. We don't track global endorser history yet (no `endorser_key_hash` in most endorsements). The practical proxy: average months since each endorsement was created for this subject. This measures "how long has this subject been endorsed?" — a meaningful signal. When network keyring adds endorser identity, a follow-up can upgrade to true endorser tenure.

- **Trust page renders L2 breakdown conditionally.** When `layer1_only = false`, the breakdown section shows both L1 and L2 items. L1 items use the existing neutral/gray style. L2 items use the ZK violet accent (`#7c3aed`) to visually distinguish endorsement-derived signals.

- **Extension shows endorsement signal line, not full L2 breakdown.** The injected trust card on GitHub is compact. Instead of rendering all L2 fields individually, show a single "Endorsed" signal line with the weighted endorsement count and a ZK tag. The full breakdown is visible on the trust card page.

- **`network_density` is structurally ready but zeroed.** The `ScoreBreakdown.network_density` field already exists. The scoring function will accept a `unique_endorser_count` parameter (defaulting to 0) so wiring in the network keyring data later is a one-line change at the call site.

## Implementation Units

- [ ] **Unit 1: Backend — Add tenure computation to scoring**

**Goal:** Compute the `tenure` Layer 2 field from endorsement timestamps.

**Requirements:** R1, R6

**Dependencies:** None

**Files:**
- Modify: `src/services/db.rs` (add `get_endorsement_tenure_months` query)
- Modify: `src/services/score.rs` (compute tenure in `score_github_repo_with_endorsements`)
- Test: `tests/score.rs`

**Approach:**
- In `db.rs`: add `get_endorsement_tenure_months(subject_id: &Uuid) -> Result<f64>`. Query: `SELECT AVG((julianday('now') - julianday(created_at)) / 30.44) FROM endorsements WHERE subject_id = ? AND status != 'failed'`. Returns average months since endorsement creation. Returns 0.0 if no endorsements.
- In `score.rs`: extend `score_github_repo_with_endorsements()` to accept `avg_tenure_months: f64` parameter. Compute `breakdown.tenure = avg_tenure_months.min(10.0)`.
- Update call sites in `trust_card.rs` and `trust_page.rs` to pass the tenure value from a new DB query.
- Add parameter `unique_endorser_count: u32` (default 0) for future `network_density` wiring: `breakdown.network_density = (f64::from(unique_endorser_count) * 3.0).min(15.0)`. Pass 0 for now.

**Patterns to follow:**
- `get_endorsement_counts_by_status()` query pattern
- Existing `score_github_repo_with_endorsements()` structure

**Test scenarios:**
- Happy path: subject with endorsements aged 3 months average → `tenure = 3.0`
- Happy path: subject with endorsements aged 15 months average → `tenure = 10.0` (capped)
- Edge case: subject with no endorsements → `tenure = 0.0`
- Edge case: single endorsement created just now → `tenure ≈ 0.0`
- Regression: existing L1-only tests unchanged (tenure defaults to 0)
- Regression: existing L2 weighting tests still pass (tenure = 0 in those tests)
- New: full L2 score with tenure > 0 produces higher score than tenure = 0

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean

---

- [ ] **Unit 2: Backend — Render Layer 2 breakdown in trust page**

**Goal:** The SSR trust card page shows Layer 2 score components when endorsements contribute to the score.

**Requirements:** R3, R5

**Dependencies:** Unit 1

**Files:**
- Modify: `src/routes/trust_page.rs` (`render_breakdown` function)

**Approach:**
- Extend `render_breakdown()` to accept a `layer1_only: bool` parameter.
- When `layer1_only = false`, append Layer 2 items after the Layer 1 items: Endorsements (max 30), Proof Strength (max 15), Tenure (max 10). Skip Network Density if 0 (not yet available).
- Style L2 items with the ZK violet color (`#7c3aed`) for the label text. Add a small "ZK" prefix tag to distinguish them from L1 items.
- Add a section label separator: "Public Signals" above L1 items, "ZK Endorsement Signals" above L2 items.
- Update the "Public data only" label in the score display to show "Public + ZK data" when Layer 2 is active.

**Patterns to follow:**
- Existing `render_breakdown()` structure with `items` array
- CSS classes from DESIGN.md (`.layer-badge-zk`, ZK violet accent)

**Test scenarios:**
Test expectation: none — SSR HTML rendering. Verified visually via the trust page.

**Verification:**
- `cargo test` passes (no rendering tests, but no regressions)
- Visual: visit `/trust/github/owner/repo` for a repo with endorsements, confirm L2 breakdown renders with violet styling
- Visual: visit a repo with no endorsements, confirm only L1 breakdown renders

---

- [ ] **Unit 3: Extension — Display Layer 2 indicator in trust card**

**Goal:** The injected trust card on GitHub shows an endorsement signal line with a ZK tag when the score includes Layer 2 data.

**Requirements:** R4, R5

**Dependencies:** Unit 1

**Files:**
- Modify: `extension/src/content-github.ts`

**Approach:**
- The trust card API already returns `score.layer1_only` and `score.breakdown` with all fields. Check `layer1_only === false` to determine if L2 data is present.
- When L2 is active, add a signal line after the existing signals: "Score includes ZK endorsements" with the `.commit-zk-tag` badge (violet, already styled in `trust-card.css`).
- Update the score circle label from "Public data" to "Public + ZK" when `layer1_only` is false.
- The endorsement count line (added in the E2E plan) already shows "{N} ZK endorsements". The new line here is about the score composition, not the count.

**Patterns to follow:**
- Existing signal line rendering in `createTrustCard()`
- `.commit-zk-tag` and `.commit-card-network` CSS classes

**Test scenarios:**
Test expectation: none — extension UI. Manual verification.

**Verification:**
- Extension builds without errors
- Manual: visit a repo with endorsements, confirm "Public + ZK" label on score circle and ZK signal line
- Manual: visit a repo without endorsements, confirm "Public data" label unchanged

---

- [ ] **Unit 4: Backend — Cache invalidation for endorsement-aware scores**

**Goal:** Ensure cached trust card scores are refreshed when endorsements change the score composition.

**Requirements:** R1, R6

**Dependencies:** Unit 1

**Files:**
- Modify: `src/routes/endorsement.rs` (invalidate cache after successful endorsement)
- Modify: `src/services/db.rs` (add `invalidate_signal_cache` method)

**Approach:**
- In `db.rs`: add `invalidate_signal_cache(subject_id: &Uuid) -> Result<()>`. Query: `DELETE FROM signal_cache WHERE subject_id = ?`.
- In `endorsement.rs`: after successfully creating an endorsement, call `db.invalidate_signal_cache(&subject_id)`. This ensures the next trust card request recomputes the score with the new endorsement data.
- Currently, the cache TTL is 1 hour. Without invalidation, a new endorsement wouldn't be reflected in the score for up to an hour. With invalidation, the next request triggers a fresh computation.
- Also invalidate after `update_endorsement_status` to `verified` (which changes the status weighting).

**Patterns to follow:**
- `cache_signals()` method structure in `db.rs`

**Test scenarios:**
- Happy path: create endorsement → signal cache for that subject is deleted → next trust card request recomputes
- Edge case: no cache entry exists for the subject → invalidation is a no-op (DELETE affects 0 rows)
- Regression: trust card still works after cache invalidation (just triggers a fresh GitHub API call)

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean

## System-Wide Impact

- **Score values change for subjects with endorsements.** When `tenure > 0`, the L2 total increases, which increases the blended score. The magnitude depends on endorsement age. For a subject with 3 endorsements aged 2 months: `tenure = 2.0`, adding 1.4 points to the blended score (2.0 * 0.7). This is a subtle increase — users won't notice a dramatic shift.
- **`score_github_repo_with_endorsements()` signature change.** Adds `avg_tenure_months` and `unique_endorser_count` parameters. Two call sites (`trust_card.rs`, `trust_page.rs`) must be updated. The compiler catches any missed sites.
- **Cache invalidation on endorsement.** New endorsements now trigger a cache delete, causing the next trust card request to re-fetch from GitHub API. At current volumes (< 10 endorsements/day), this adds negligible GitHub API load. The cache TTL (1 hour) already limits re-fetches.
- **Trust page HTML changes.** The breakdown section renders more items when L2 is active. No CSS changes needed — existing `.breakdown-item` styles apply. Violet color for L2 labels is inline or a new small CSS class.
- **Extension trust card changes.** Score label text changes conditionally. No new API calls — uses data already in the trust card response.
- **Unchanged invariants:** Badge SVG endpoint uses the score value (which may be slightly higher now) but the color thresholds (>70 green, >40 amber) are unchanged. MCP server returns the same score structure.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Tenure proxy (per-subject age) diverges from CEO plan (per-endorser age) | Document as Phase 2 proxy. The per-endorser metric requires global endorser tracking from the network keyring plan. Upgrade when that data is available. |
| Score increase surprises users | Phase 2 has very few endorsements. The tenure contribution is small (max 7 points in blended score). Document the scoring methodology on the trust page. |
| Cache invalidation increases GitHub API calls | Marginal: one extra API call per endorsement creation. Current rate limit (5000/hr) is far from exhausted. |
| `network_density` permanently 0 until keyring lands | Structural placeholder ready. The score is still meaningful with 3 of 4 L2 fields active. |

## Sources & References

- CEO plan score formula: `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md`
- Score model: `src/models/signal.rs:40-101`
- Score computation: `src/services/score.rs`
- Trust page rendering: `src/routes/trust_page.rs:570-592`
- Existing score tests: `tests/score.rs`
