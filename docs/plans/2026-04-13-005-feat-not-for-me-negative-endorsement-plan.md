---
title: "Not for me" negative endorsement signal
type: feat
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# "Not for me" negative endorsement signal

## Overview

Add a negative sentiment to endorsements so users can signal "not for me" alongside the existing positive endorse action. One endorsement per device per subject, mutable sentiment via upsert. Negative signals subtract from the endorsement sub-score. Surfaced on the GitHub trust card and the SSR trust page; not on the compact SERP card.

## Problem Frame

The endorsement model today only captures positive signal. Honest trust networks need both directions — "I use this and recommend it" and "I tried this and don't recommend it." Without a negative path, the score conflates "no signal" with "negative signal," and users can't express dissent without abstaining. The CEO plan accepts negative signals as scope for Phase 3 because shipping both from day one establishes Commit as honest, and the effort is small.

The mechanism must be flip-friendly (a user can change their mind), abuse-resistant at current scale (ZK proof cost is the brigading deterrent), and preserve the existing per-subject rate limit while adding a per-endorser dimension so coordinated groups can't exhaust the limit on one polarity to block the other.

## Requirements Trace

- R1. Negative endorsements are persisted with a `sentiment` field; existing rows default to `positive` (no backfill).
- R2. A device can only have one endorsement per subject; clicking the opposite action flips sentiment via upsert (no second row).
- R3. Score algorithm distinguishes positive vs negative when computing the endorsement sub-score; negative endorsements reduce the sub-score.
- R4. Per-endorser-key-hash rate limit dimension is added so coordinated brigading on one polarity can't starve the other.
- R5. Extension renders "Endorse" (primary) + "Not for me" (subdued, secondary) on the GitHub card. SERP card is unchanged.
- R6. Endorsements with NULL `endorser_key_hash` (legacy webhook-created) remain insert-only and are not constrained by the new uniqueness.

## Scope Boundaries

- SERP compact card does not get a "Not for me" link (CEO decision: too little context on SERP for a negative signal).
- Trust page SSR endorse CTA is not added here — that link goes to CWS, not to a backend action (CEO plan §2). The trust page is read-only for endorsements.
- No moderation tools, no appeals flow, no display of "X people said not for me" on the public card. Only the aggregate impact on the score is visible in this plan.

### Deferred to Separate Tasks

- "You endorsed this" revisit indicator (positive vs negative state in the cache): planned in `docs/plans/2026-04-13-007-feat-you-endorsed-this-revisit-indicator-plan.md`. That plan reads the new sentiment field; this plan only ensures the field is present and returned on the endorsement summary.
- L2 attestation behavior on flip: see Open Questions; current proposal is to attest only on INSERT, not on UPDATE.

## Context & Research

### Relevant Code and Patterns

- `src/services/db.rs:79–88` — `migrate()` runs the schema migrations idempotently. Existing migration at line 167 added `endorser_key_hash` with the comment "useful for revisit indicators, sentiment flips, sybil analysis" — explicit landing zone for this work.
- `src/models/endorsement.rs:83–103` — `Endorsement` and `EndorsementSummary` structs; both will gain a `sentiment` field.
- `src/routes/endorsement.rs:31–135` — `submit_endorsement()` is currently INSERT-only via `db.create_endorsement()`. Needs to become an upsert that flips sentiment when `(endorser_key_hash, subject_id)` already exists.
- `src/routes/endorsement.rs:137–186` — GET endorsements; response includes `sentiment` going forward.
- `src/routes/trust_card.rs:65–190` — `/trust-card` returns `endorsement_count` and `recent_endorsements`. Needs separate counts (positive, negative) and the score consumer needs both.
- `src/services/db.rs:367` (`get_endorsement_count`) and `:377–397` (`get_endorsement_counts_by_status`) — query layer that aggregates for the score.
- `src/services/score.rs:58–94` — Layer 2 endorsement scoring. The negative-weight knob lives here.
- `src/lib.rs:11–13` — current per-subject rate limit (5/60min). Needs a per-endorser dimension added.
- `extension/src/content-github.ts:172–243` — endorse button creation and `startEndorsement()` flow. The new "Not for me" link sits adjacent and reuses `chrome.runtime.sendMessage({ type: "START_ENDORSEMENT" })` with a `sentiment` field.
- `extension/src/background.ts:122–131,179,241` — service worker handler and POST to `/endorsements`. Needs to forward `sentiment` and increment the local count regardless of polarity.
- `extension/src/trust-card.css` — shared styles for both cards. New `.endorse-secondary` class for the subdued "Not for me" link.

### Institutional Learnings

- `docs/solutions/` has prior CI gate parity learnings (2026-04-12) that reinforce running `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test` locally before push. New migrations and route changes must clear all three gates.
- The P0 proof-binding work (`docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md`) and security hardening batch (`docs/plans/2026-04-12-005-fix-security-hardening-batch-plan.md`) established that the request payload subject must match the proof transcript subject. The `sentiment` field is application-layer metadata that the proof does not bind — note that explicitly in code comments and tests so future reviewers don't mistake unbound fields.

### External References

- None gathered for this plan; the work is bounded by existing patterns in the repo (SQLite migrations, axum routes, reqwest-driven extension flow). No new framework decisions.

## Key Technical Decisions

- **Sentiment as a dedicated TEXT column**, not encoded in `category` or `status`. Values: `'positive'` or `'negative'`. Default `'positive'`. Keeps existing `category` and `status` semantics intact and allows clean SQL filters.
- **Upsert via UNIQUE `(endorser_key_hash, subject_id)`** with `INSERT ... ON CONFLICT(endorser_key_hash, subject_id) DO UPDATE SET sentiment = excluded.sentiment, status = excluded.status, attestation_data = excluded.attestation_data, proof_hash = excluded.proof_hash, proof_type = excluded.proof_type, created_at = excluded.created_at`. Treat a flip as a fresh endorsement event (re-prove, re-attest), not a metadata-only edit. This keeps the proof binding honest: every sentiment a row reports has a current ZK proof backing it.
- **NULL `endorser_key_hash` rows are exempt** from the unique constraint. SQLite's UNIQUE treats NULL as distinct, so legacy webhook-created rows (no key hash) continue to work without modification. New rows from the extension always carry `endorser_key_hash`.
- **Negative weight in the score algorithm**: subtract negative endorsements from the weighted sum at `src/services/score.rs:71–74` using `NEGATIVE_WEIGHT = -1.0` (mirrors `VERIFIED_WEIGHT = 1.0`). `network_density` counts unique endorsers regardless of polarity (a person who said "not for me" is still a verified person in the network). `proof_strength` and `tenure` use the absolute-value count so they aren't artificially deflated by negatives.
- **Floor the endorsements component at 0**: `endorsements = max(0.0, min(weighted_sum * 5.0, 30.0))`. Negative endorsements can pull the contribution to zero but not below — Layer 2 should never be net-negative on its own; that combines awkwardly with Layer 1 blending.
- **Per-endorser rate limit dimension**: enforce `5 endorsements per endorser_key_hash per subject per 60 minutes` (in addition to the existing per-subject limit). Sliding window in memory is fine for now; the existing limiter pattern in `src/lib.rs` extends naturally.
- **Extension UX**: two adjacent compact text links — `Endorse` (primary, current treatment) and `Not for me` (smaller, muted gray, no border). On click, both go through the same `START_ENDORSEMENT` flow with a `sentiment` field on the message. The button label updates to reflect the persisted sentiment after success.
- **L2 attestation on flip**: only attest on INSERT, not UPDATE. The L2 record represents a one-way "this device made an endorsement of this subject"; flipping sentiment doesn't warrant a second on-chain write. Captured in Open Questions for confirmation.

## Open Questions

### Resolved During Planning

- **Should sentiment flips re-prove or just metadata-edit?** Resolved: re-prove. Keeps proof binding tight and avoids a class of bugs where the stored proof references a stale assertion.
- **Negative weight magnitude?** Resolved: `-1.0` (symmetric with positive). Symmetry is the simplest defensible default; the founder can tune later from real data.
- **Should the SERP card show "Not for me"?** Resolved: no (per CEO plan).
- **Display public counts of negatives?** Resolved: no in this plan. Only the score impact is user-visible. Avoids early piling-on dynamics at low N.

### Deferred to Implementation

- **Exact SQLite upsert syntax for the conflict target with NULL key_hash rows**: rusqlite's parameterized upsert plus a partial unique index is the likely shape, but the implementer should confirm against the rusqlite version pinned in `Cargo.toml`. Document the chosen approach in a code comment.
- **Whether to emit a new L2 attestation on flip**: leaning "no, INSERT only." The implementer should confirm the L2 submission code path does not assume every persisted endorsement triggers a tx, and add a guard if needed.
- **Per-endorser rate limit storage**: in-process HashMap is fine for now (single Fly.io instance). If the deploy ever scales horizontally this needs revisiting — note as a TODO comment.

## Implementation Units

- [ ] **Unit 1: Schema migration — `sentiment` column + unique index**

**Goal:** Add `sentiment` column with default `'positive'` and a partial unique index on `(endorser_key_hash, subject_id)` that excludes NULL key hashes.

**Requirements:** R1, R2, R6

**Dependencies:** None

**Files:**
- Modify: `src/services/db.rs` (append migration after line 170)
- Test: `src/services/db.rs` (extend existing migration tests)

**Approach:**
- Add `ALTER TABLE endorsements ADD COLUMN sentiment TEXT NOT NULL DEFAULT 'positive'`.
- Add `CREATE UNIQUE INDEX IF NOT EXISTS idx_endorser_subject_unique ON endorsements(endorser_key_hash, subject_id) WHERE endorser_key_hash IS NOT NULL`.
- Migrations are idempotent; preserve existing rows.

**Patterns to follow:**
- The `endorser_key_hash` migration at `src/services/db.rs:160–170` — same shape (ALTER then optionally an index), wrapped in the existing migration runner.

**Test scenarios:**
- Happy path: fresh DB after `migrate()` exposes a `sentiment` column with default `'positive'` on a freshly inserted row.
- Edge case: idempotent re-run — calling `migrate()` twice doesn't error or duplicate the column/index.
- Edge case: rows inserted with NULL `endorser_key_hash` do not collide on the unique index, even when `(NULL, subject_id)` repeats.

**Verification:**
- `cargo test` passes; the new migration applies cleanly to a DB that already contains pre-migration rows.

- [ ] **Unit 2: Model + DAL upsert**

**Goal:** Update `Endorsement`/`EndorsementSummary` to carry `sentiment`. Add `upsert_endorsement()` on the DB service that performs `INSERT ... ON CONFLICT DO UPDATE` against the new index.

**Requirements:** R1, R2, R6

**Dependencies:** Unit 1

**Files:**
- Modify: `src/models/endorsement.rs` (add `sentiment: Sentiment` field to both structs; add `Sentiment` enum with `serde` round-trip)
- Modify: `src/services/db.rs` (new `upsert_endorsement()`; update `get_endorsement_counts_by_status()` to also bucket by sentiment; add `get_endorsement_counts()` returning `(positive_verified, positive_pending, negative_verified, negative_pending)` or a small struct)
- Test: `src/services/db.rs` and `tests/api.rs`

**Approach:**
- `Sentiment` enum is a 2-variant `Positive`/`Negative` with `#[serde(rename_all = "lowercase")]`. Defaults to `Positive` for backward compatibility on old rows during deserialization.
- `upsert_endorsement` accepts the same fields `create_endorsement` does today and additionally `sentiment`. On conflict, update sentiment, status, attestation_data, proof_hash, proof_type, created_at — i.e., treat the conflict row as fully refreshed.
- `create_endorsement` remains for NULL-key-hash inserts (webhook path) so the existing webhook flow doesn't change shape.

**Patterns to follow:**
- Existing `EndorsementStatus` enum in `src/models/endorsement.rs` for the serde shape.
- `get_endorsement_counts_by_status` at `src/services/db.rs:377–397` for the count-bucketing pattern.

**Test scenarios:**
- Happy path: `upsert_endorsement` inserts when no row matches; row is readable with sentiment set.
- Happy path: `upsert_endorsement` flips an existing row from positive to negative when called with the same `(endorser_key_hash, subject_id)` and a different sentiment; row count is unchanged; latest proof_hash and created_at are stored.
- Edge case: `upsert_endorsement` on the same key/subject with the same sentiment refreshes proof and timestamp (no-op semantically but the row is "renewed").
- Edge case: NULL `endorser_key_hash` rows can be inserted multiple times for the same subject without conflict (legacy webhook behavior preserved).
- Integration: GET `/endorsements?kind=...&id=...` returns `sentiment` in the JSON response for both newly-upserted and legacy rows (legacy rows default to `positive`).

**Verification:**
- `cargo test` passes; manual SQL inspection (or a focused test) confirms a flip leaves a single row with the new sentiment.

- [ ] **Unit 3: POST /endorsements accepts and persists sentiment; per-endorser rate limit**

**Goal:** Extend the request payload with optional `sentiment` (default `positive`), route it through `upsert_endorsement` when `endorser_key_hash` is present, and enforce a per-endorser rate limit alongside the existing per-subject limit.

**Requirements:** R1, R2, R4, R6

**Dependencies:** Unit 2

**Files:**
- Modify: `src/routes/endorsement.rs` (add `sentiment` to `SubmitEndorsementRequest`; switch to `upsert_endorsement` when `endorser_key_hash` is `Some`; route through `create_endorsement` otherwise)
- Modify: `src/lib.rs` (extend rate limiter to include a per-endorser dimension)
- Test: `tests/api.rs`

**Approach:**
- `sentiment: Option<Sentiment>` with `serde(default)` falling back to `Positive`. Keeps older clients working without a manifest bump.
- Rate limit: when `endorser_key_hash` is `Some`, additionally track `(endorser_key_hash, subject_id)` in a sliding-window counter with the same 5-per-60-min limit. Per-subject limit remains.
- Webhook-created endorsements (no key hash) take the existing `create_endorsement` path unchanged.

**Patterns to follow:**
- Existing rate-limit pattern in `src/lib.rs:11–13` — extend the same `RateLimiter` with a second keyed map, or compose a second limiter instance.
- Existing request validation flow at `src/routes/endorsement.rs:31–135` for sentiment field placement.

**Test scenarios:**
- Happy path: POST with `sentiment: "negative"` and an `endorser_key_hash` returns 201 and persists with sentiment=negative.
- Happy path: POST without `sentiment` defaults to positive (backward compatibility).
- Happy path: POST with `sentiment: "negative"` for a subject the endorser previously endorsed positively returns 200/201 (whichever is conventional for upsert) and the row is flipped.
- Error path: invalid sentiment string returns 400.
- Error path: 6th rapid POST from the same `endorser_key_hash` for the same subject within 60 minutes returns 429 even when polarities differ.
- Edge case: 6th rapid POST from different endorsers for the same subject still returns 429 due to the per-subject limit (existing behavior preserved).
- Integration: webhook-style POST with `endorser_key_hash: null` and `sentiment` omitted continues to insert (not upsert) and is exempt from the per-endorser limit.

**Verification:**
- `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` all pass.

- [ ] **Unit 4: Score algorithm respects sentiment**

**Goal:** Update Layer 2 scoring to subtract negative endorsements from the weighted sum, floor the endorsements component at 0, and treat unique endorsers polarity-agnostically for `network_density`.

**Requirements:** R3

**Dependencies:** Unit 2 (needs the new count bucketing)

**Files:**
- Modify: `src/services/score.rs` (around lines 58–94)
- Modify: `src/models/signal.rs` (if its inputs change)
- Modify: `src/routes/trust_card.rs` (pass positive and negative counts into the score function)
- Test: `tests/score.rs`

**Approach:**
- Introduce `NEGATIVE_WEIGHT: f64 = -1.0` constant alongside `VERIFIED_WEIGHT` and `PENDING_WEIGHT`.
- Weighted sum = `(positive_verified * VERIFIED_WEIGHT) + (positive_pending * PENDING_WEIGHT) + (negative_verified * NEGATIVE_WEIGHT) + (negative_pending * NEGATIVE_WEIGHT * 0.3)` — negatives use the same pending-discount factor as positives so a flip-pending negative doesn't outweigh a verified positive.
- `endorsements = max(0.0, min(weighted_sum * 5.0, 30.0))`.
- `proof_strength` and `tenure` use total endorsement counts ignoring sentiment (the proof itself is what they measure).
- `network_density` uses `unique_endorser_count` ignoring sentiment.

**Patterns to follow:**
- The constants and `min`/`max` capping idioms already in `src/services/score.rs`.

**Test scenarios:**
- Happy path: 3 positive verified, 0 negative → identical score to today.
- Happy path: 3 positive verified, 1 negative verified → endorsements component reduced by `1.0 * 5.0 = 5.0` (capped at 0 floor).
- Edge case: 0 positive, 2 negative → `endorsements = 0` (floored); network_density still reflects 2 unique endorsers.
- Edge case: equal positive and negative → endorsements component is 0; rest of Layer 2 reflects the proof activity.
- Integration: a flip from positive to negative on the same `endorser_key_hash` reduces the endorsement count for positive and increases for negative; recomputed score reflects this without double-counting the endorser.

**Verification:**
- `cargo test` passes; the existing scoring tests at `tests/score.rs:1–150` still pass (no regressions for all-positive data).

- [ ] **Unit 5: Extension UI — adjacent "Not for me" link, sentiment in message**

**Goal:** Render a subdued "Not for me" link next to the existing "Endorse" button on the GitHub trust card. Both actions go through `START_ENDORSEMENT` with a `sentiment` field. Button labels reflect persisted sentiment after success.

**Requirements:** R5

**Dependencies:** Unit 3 (backend must accept sentiment) — coordinate merge order, but client can ship first if it sends `sentiment: "positive"` only until backend lands.

**Files:**
- Modify: `extension/src/content-github.ts:172–243` (add the secondary link, route both clicks through one handler)
- Modify: `extension/src/background.ts:122–131,241` (forward `sentiment` from the message to the POST body)
- Modify: `extension/src/trust-card.css` (add `.endorse-secondary` muted styling)
- Modify: `extension/src/content-google.ts` (no UI change; just confirm the SERP path is untouched and document the decision in a comment)
- Test: `extension/test/extension-smoke.spec.ts` (extend smoke to assert both links render on the GitHub card)

**Approach:**
- HTML structure: a small flex row containing `<button class="endorse-primary">Endorse</button>` and `<button class="endorse-secondary">Not for me</button>`. Both elements go through a single `startEndorsement(subject, sentiment, btn)` function.
- Message shape: `{ type: "START_ENDORSEMENT", repoOwner, repoName, sentiment: "positive" | "negative" }`.
- Visual states (post-success): primary becomes `Endorsed`, secondary stays `Not for me`. If user clicks the secondary, primary returns to `Endorse` and secondary becomes `Not for me ✓`. (The full revisit indicator with persistence is in plan 007.)
- DESIGN.md compliance: secondary link is muted gray (no border, smaller size), primary keeps existing treatment.

**Patterns to follow:**
- Existing endorse button creation at `extension/src/content-github.ts:172–183`.
- The `chrome.runtime.sendMessage` shape already used for `START_ENDORSEMENT`.

**Test scenarios:**
- Happy path: both `Endorse` and `Not for me` render adjacent on a GitHub repo page.
- Happy path: clicking `Not for me` triggers a `START_ENDORSEMENT` message with `sentiment: "negative"` (assert via mocked `chrome.runtime` or a network intercept).
- Edge case: SERP card render is unaffected — only the `Endorse` link appears (or none, depending on whether SERP parity has shipped).
- Integration: after a successful negative endorsement POST, the secondary button state reflects `Not for me ✓` until page reload (full persistence is plan 007).

**Verification:**
- `npm run build` succeeds, Playwright smoke passes, manual load on a real GitHub repo shows the two-link layout.

- [ ] **Unit 6: Trust page (SSR) sentiment surfacing — minimal**

**Goal:** Ensure the SSR trust page consumes the new positive/negative counts cleanly through the score function and does not crash on rows with the new column. No new visible UI in this plan (per Scope Boundaries).

**Requirements:** R3 (consumer side)

**Dependencies:** Units 2, 4

**Files:**
- Modify: `src/routes/trust_page.rs` (verify the score call passes through the new counts; no template changes required)
- Test: `tests/api.rs` (existing trust page tests should stay green; add one asserting the page renders when the DB contains a mix of positive and negative endorsements)

**Approach:**
- Audit the call path from `render_github_trust_page` through to `score_github_repo_with_endorsements` and confirm the new count struct is threaded end-to-end.
- No new copy or visual elements — the score itself is the user-visible signal here.

**Patterns to follow:**
- Existing trust page rendering flow at `src/routes/trust_page.rs:19–186`.

**Test scenarios:**
- Happy path: trust page renders 200 with mixed positive/negative endorsements in the DB; score reflects the algorithm.
- Edge case: trust page renders 200 with only negative endorsements (score reflects the floor).

**Verification:**
- `cargo test` passes; manual smoke against `commit-backend.fly.dev/trust/github/<owner>/<repo>` after deploy.

## System-Wide Impact

- **Interaction graph:** Endorsement POST now branches on `endorser_key_hash` presence (upsert vs insert). Score function consumers (`/trust-card`, `/trust/{kind}/{id}`) all read the new bucketed counts. Extension `background.ts` forwards a new field to the backend.
- **Error propagation:** Upsert conflicts that aren't sentiment flips (e.g., proof verification fails) must still return the same 4xx as today — don't silently overwrite a verified row with a failed one. Wrap the upsert in an explicit "verify proof first, then persist" order.
- **State lifecycle risks:** A flip that persists but fails to L2-attest leaves the DB ahead of the chain. Acceptable per Open Questions decision (don't attest on UPDATE), but document this in the L2 submitter so future readers understand the asymmetry.
- **API surface parity:** Both `/endorsements` and `/trust-card` responses gain `sentiment` in their summary objects. The MCP server (`src/mcp/`) wraps `/trust-card` — confirm its response schema either passes through additional fields untouched or is updated in lockstep.
- **Integration coverage:** The most important cross-layer scenario is "flip via upsert + score recomputation" — covered in Unit 4's integration scenario. The second is "flip + extension UI reflects backend state" — covered by smoke in Unit 5.
- **Unchanged invariants:** Webhook-created endorsements (NULL `endorser_key_hash`) keep their insert-only, no-rate-limit-per-endorser behavior. Existing positive-only test data continues to score identically. Proof binding semantics (subject in transcript must match request subject) are unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Upsert syntax difference in pinned rusqlite version causes the migration or DAL to fail | Implementer confirms exact syntax against `Cargo.toml`'s rusqlite version before writing the query; falls back to "SELECT then INSERT-or-UPDATE" if upsert isn't supported |
| Negative weight `-1.0` is too aggressive at low N (one negative cancels one positive completely) | The endorsements component is floored at 0; full algorithm tunability is captured as a follow-up. If the founder sees the score behaving badly post-launch, the constant is a one-line change |
| L2 attestation submitter assumes every persisted endorsement triggers a tx | Implementer audits the L2 path in Unit 4/Unit 6; adds an explicit guard if needed |
| Brigading via coordinated negative endorsements at low N | Per-endorser rate limit (Unit 3) plus the ZK proof cost (~5s MPC-TLS per endorsement) make sustained brigading expensive. Acceptable risk at <50 users; revisit if the user base grows fast |
| Extension and backend ship out of sync (extension sends sentiment field before backend understands it, or vice versa) | Backend uses `serde(default)` so the field is optional; extension can ship first sending only `positive`. Coordinate merge order to land backend (Units 1–4) before extension (Unit 5) |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist to mark "Not for me" complete after merge.
- No runbook changes; no new env vars; no new external dependencies.
- The new partial unique index is small and the upsert is idempotent — no special migration window needed for Fly.io deploy.

## Sources & References

- **Origin document:** `~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md` (§4 "Do not endorse" negative signal)
- Related code: `src/services/db.rs:79–170`, `src/routes/endorsement.rs:31–186`, `src/services/score.rs:58–94`, `extension/src/content-github.ts:172–243`
- Related plans: `docs/plans/2026-04-12-005-fix-security-hardening-batch-plan.md` (proof binding context), `docs/plans/2026-04-12-008-feat-commit-score-v2-plan.md` (score architecture this plan extends), `docs/plans/2026-04-13-007-feat-you-endorsed-this-revisit-indicator-plan.md` (downstream consumer of `sentiment`)
