---
title: "refactor: Remove dead personal-keyring code (one-network model)"
type: refactor
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# refactor: Remove dead personal-keyring code (one-network model)

## Overview

Delete the personal key-sharing / "friend network" code paths that were made obsolete by the 2026-04-12 one-network decision. The product model is ONE global ZK-anonymous network — not per-user friend graphs — so the personal keyring UI, the `POST /network-query` endpoint, the `NETWORK_QUERY` background handler, the `count_network_endorsements` db method, and their tests are dead weight.

Per the CEO plan, the `endorser_key_hash` column on the endorsements table stays — it still serves "you endorsed this" indicators, sentiment flips, and future sybil analysis. This plan deletes surface area above that column, nothing below it.

## Problem Frame

The original Phase 2 design shipped a personal keyring: users could add friends' public keys to a local list, the extension would hash them and ask the backend "how many of these keys have endorsed this subject?". That model implies a social graph ("N of your friends endorse this"). The founder's vision is instead "N verified humans endorse this" — ZK anonymity is the trust primitive, not social proximity. Keeping the keyring code around is misleading (the popup still advertises "YOUR NETWORK" as a friend list), bloats the extension, and creates a surface that could regress the one-network guarantee.

## Requirements Trace

- R1. `POST /network-query` returns 404 after merge (endpoint removed)
- R2. Background worker exposes no `NETWORK_QUERY` / `KEYRING_ADD` / `KEYRING_REMOVE` message types
- R3. `content-github.ts` no longer calls `NETWORK_QUERY` or consumes `NetworkData`
- R4. Popup shows a minimal, non-empty summary: truncated public key + endorsement count + about link (no friend list, no "Add to network" form)
- R5. `endorser_key_hash` column, `POST /endorsements` handling of it, and the "ZK endorsement count" display on the GitHub card are untouched
- R6. `cargo test` and `cargo clippy -- -D warnings` pass; extension Playwright smoke test still green

## Scope Boundaries

- Not changing the endorsements schema, the `endorser_key_hash` column, or any endorsement creation path
- Not changing the trust card data contract (`endorsement_count` field stays; `network_data` field is removed from the TypeScript interface but the backend never populates it anyway — the field was produced client-side by merging `NETWORK_QUERY` results, so the backend JSON contract does not change)
- Not adding new backend endpoints for the popup summary — popup reads local state only

### Deferred to Separate Tasks

- "You endorsed this" revisit indicator (separate Phase 3 item). Once it lands, the popup can surface endorsed-subject counts from the same cache. This plan ships an interim `endorsement_count` counter (incremented on successful endorsement) so the popup is never empty.

## Context & Research

### Relevant Code and Patterns

- `src/routes/network.rs` — handler to delete in full
- `src/routes/mod.rs:3` — `pub mod network;` line to remove
- `src/main.rs:100-103` — `/network-query` route registration to remove
- `src/services/db.rs:446-474` — `count_network_endorsements` method to remove
- `tests/api.rs:1435-1622` — 10 `network_query_*` tests to delete
- `tests/api.rs:49-50` — test router registration of `/network-query` to remove
- `extension/src/background.ts:93-97` — `KeyringEntry` interface
- `extension/src/background.ts:109-128` — `KEYRING_ADD` / `KEYRING_REMOVE` / `NETWORK_QUERY` message routes
- `extension/src/background.ts:271-376` — keyring helpers and `handleNetworkQuery`
- `extension/src/content-github.ts:13-16` — `NetworkData` interface
- `extension/src/content-github.ts:34-36` — `network_data` field on `TrustCardData`
- `extension/src/content-github.ts:285-297` — `NETWORK_QUERY` call site
- `extension/src/popup.{ts,html,css}` — keyring UI to replace

### Institutional Learnings

- `docs/solutions/best-practices/parallel-worktree-agent-workflow-2026-04-12.md` — this plan is designed to execute in parallel with three other Phase 3 plans; file-set is disjoint from Plans B, C, D except for two regions of `background.ts` (Plan D adds an `onInstalled` branch; this plan deletes message handlers). Merge is trivial.

### External References

None required — pure deletion plus a small UI replacement.

## Key Technical Decisions

- **Popup endorsement count sourced from local storage, not backend.** Avoids adding a new backend endpoint just for popup chrome. Counter is incremented in `background.ts` after a successful `POST /endorsements`. Rationale: the "you endorsed this" revisit indicator (future Phase 3 item) will replace this with a cache of endorsed subject IDs; picking the simplest possible interim source keeps both plans small.
- **Delete, don't soft-deprecate.** CEO plan is unambiguous: "Delete, not mothball." Leaving the route behind a feature flag would invite regression.
- **Popup keeps the public key display.** Useful for future sybil analysis and for the user to confirm they have a keypair.

## Open Questions

### Resolved During Planning

- *Does the popup need to show an endorsement count?* — Yes (per CEO plan "minimal, not empty"). Source from local counter.
- *Does the backend trust-card JSON contract change?* — No. The backend never returned `network_data`; that field was synthesized client-side by merging `NETWORK_QUERY` results into the cached response. Removing it is a pure client-side change.

### Deferred to Implementation

- Exact wording for the popup "About" link target (extension page vs. external docs vs. the trust page at `commit-backend.fly.dev/`). Implementer chooses; a simple link to `API_BASE` is sufficient.

## Implementation Units

- [ ] **Unit 1: Remove backend `/network-query` endpoint and db query**

**Goal:** Delete the route, handler, db method, and associated tests so `POST /network-query` returns 404.

**Requirements:** R1, R6

**Dependencies:** None — self-contained backend change.

**Files:**
- Delete: `src/routes/network.rs`
- Modify: `src/routes/mod.rs` (remove `pub mod network;`)
- Modify: `src/main.rs` (remove `/network-query` route registration around lines 100-103)
- Modify: `src/services/db.rs` (remove `count_network_endorsements` method, lines 444-474)
- Modify: `tests/api.rs` (remove the test router registration around lines 49-50 AND the 10 `network_query_*` tests at lines 1435-1622, AND the `setup_network_test_data` helper if only used by these tests — verify with grep before deleting)
- Test: `tests/api.rs` (verify remaining tests still pass)

**Approach:**
- Grep for any remaining references to `network::`, `network_query`, `count_network_endorsements` after deletion; fix stragglers
- `setup_network_test_data` helper: if it is only referenced by the deleted tests, delete it; if it is used by other tests (unlikely but check), keep it

**Patterns to follow:**
- `src/routes/privacy.rs` and its registration in `main.rs` are the shape of a self-contained route — deletion is the inverse

**Test scenarios:**
- Happy path: `cargo test` passes with the 10 `network_query_*` tests removed
- Happy path: `cargo clippy -- -D warnings` passes (no unused imports, no dead code warnings)
- Integration: a new smoke test `network_query_endpoint_removed` that POSTs to `/network-query` and asserts `StatusCode::NOT_FOUND` — this locks in R1 against accidental re-introduction

**Verification:**
- `grep -r "network_query\|count_network_endorsements\|routes::network" src/ tests/` returns no matches
- New `network_query_endpoint_removed` test is green

- [ ] **Unit 2: Remove keyring + NETWORK_QUERY handlers from background worker**

**Goal:** Delete keyring state and the three obsolete message types from the service worker; add a tiny local counter for the popup summary.

**Requirements:** R2, R4

**Dependencies:** None. Can run fully in parallel with Unit 1.

**Files:**
- Modify: `extension/src/background.ts`

**Approach:**
- Remove the `KeyringEntry` interface (lines 93-97)
- Remove the `KEYRING_ADD`, `KEYRING_REMOVE`, and `NETWORK_QUERY` branches from the `onMessage` listener (lines 109-128)
- Remove `handleKeyringAdd`, `handleKeyringRemove`, `handleNetworkQuery`, and the `keyringMutex` variable (lines 271-376)
- Remove any leftover imports/types only used by the deleted code
- In `runEndorsementFlow`, after a successful `POST /endorsements` (where the existing code logs `Endorsement created: ${endorsement.id}`), increment a local counter:
  ```
  const { endorsement_count = 0 } = await chrome.storage.local.get("endorsement_count");
  await chrome.storage.local.set({ endorsement_count: endorsement_count + 1 });
  ```
  Keep this logic narrow and well-commented — it exists purely to feed the popup summary until the revisit-indicator plan replaces it.

**Coordination note:** Plan D (post-install onboarding) modifies the `chrome.runtime.onInstalled` handler in this same file. Different region (top of file vs. message dispatcher region), trivial merge.

**Patterns to follow:**
- The `START_ENDORSEMENT` branch (lines 100-107) is the shape a preserved message handler should keep

**Test scenarios:**
- Happy path (manual, via Playwright smoke or manual load): installing the extension and opening the service worker console produces no `Uncaught ReferenceError` from the deleted symbols
- Edge case: visiting a GitHub repo page triggers a trust card fetch and no `NETWORK_QUERY` message is sent (verify via background console logs — should see no `[commit] network query` output)
- Integration: full endorsement flow on a GitHub repo still works (START_ENDORSEMENT → offscreen proof → POST /endorsements → success)
- Integration: after a successful endorsement, `chrome.storage.local.get("endorsement_count")` returns an incremented integer

**Verification:**
- `grep -n "KEYRING\|NETWORK_QUERY\|keyring\|KeyringEntry" extension/src/background.ts` returns no matches
- Extension builds cleanly: `npm run build` in `extension/`
- Playwright smoke (`extension/test/extension-smoke.spec.ts`) still passes

- [ ] **Unit 3: Remove network query call from GitHub content script**

**Goal:** Strip the `NETWORK_QUERY` runtime message and `NetworkData` type from the GitHub content script; the trust card already renders endorsement count from `data.endorsement_count`, which stays.

**Requirements:** R3, R5

**Dependencies:** None. Fully parallel with Units 1, 2, 4.

**Files:**
- Modify: `extension/src/content-github.ts`

**Approach:**
- Remove the `NetworkData` interface (lines 13-16)
- Remove the `network_data?: NetworkData | null;` field on `TrustCardData` (line 35)
- Remove the `try { const networkData = await chrome.runtime.sendMessage({ type: "NETWORK_QUERY", ... }); ... }` block in `fetchTrustCard` (lines 285-297)
- Leave the `data.endorsement_count` display logic at lines 124-129 untouched — it was always sourced from the backend, not from `NETWORK_QUERY`

**Patterns to follow:**
- `content-google.ts` (already networkless) is the target shape

**Test scenarios:**
- Happy path: visiting a GitHub repo with existing endorsements shows "N ZK endorsements" line as before
- Happy path: visiting a GitHub repo with zero endorsements hides the network line (existing `if (data.endorsement_count > 0)` guard)
- Integration: no `NETWORK_QUERY` message is ever sent from this script (background console shows no inbound `NETWORK_QUERY` regardless of whether Unit 2 has shipped)

**Verification:**
- `grep -n "NetworkData\|NETWORK_QUERY\|network_data" extension/src/content-github.ts` returns no matches
- Extension builds cleanly
- Manual check: open any repo in the Playwright smoke test (or `github.com/nickel-org/nickel.rs`) — the trust card renders with its existing endorsement line intact

- [ ] **Unit 4: Replace popup keyring UI with minimal summary**

**Goal:** Popup becomes a minimal "status card": truncated public key + endorsement count + about link. No friend list, no "Add to network" form.

**Requirements:** R4

**Dependencies:** None. Can run fully in parallel with Units 1-3. (Unit 2 is what increments the counter consumed here, but this UI tolerates a missing / zero counter fine.)

**Files:**
- Modify: `extension/src/popup.html`
- Modify: `extension/src/popup.ts`
- Modify: `extension/src/popup.css`

**Approach:**
- `popup.html`: keep the existing `<h1>Commit</h1>`. Keep the "YOUR KEY" section (public key + copy button). Replace the "YOUR NETWORK" section with an "ACTIVITY" section containing the endorsement count (e.g., `<strong>N endorsements made</strong>`) and a muted-gray "About Commit" link that opens `https://commit-backend.fly.dev/` in a new tab. Remove all `keyring-list` / `keyring-add` markup.
- `popup.ts`: keep `displayOwnKey`. Delete `renderKeyring`, `setupAddForm`, `KeyringEntry` interface, and any helpers only they used (`truncateKey` and `bytesToHex` stay — they're used by `displayOwnKey`). Add a small `displayEndorsementCount()` that reads `chrome.storage.local.get("endorsement_count")` and renders it (defaulting to 0 when missing).
- `popup.css`: remove `.keyring-list`, `.keyring-entry`, `.keyring-label`, `.keyring-key`, `.keyring-empty`, `.keyring-add`, `.popup-input`, `.popup-btn--danger` (unused after deletion — verify with grep). Add a small rule for the "ACTIVITY" count display consistent with DESIGN.md (`font-variant-numeric: tabular-nums`, 13px body with 16px count figure).
- Honor DESIGN.md: Geist font stack, `#f5f5f0` paper background, `#1a1a2e` ink, `#888` for the uppercase section label.

**Patterns to follow:**
- Existing `.popup-section-title` and `.key-display` — match their spacing and type scale

**Test scenarios:**
- Happy path: fresh install shows "0 endorsements" and the truncated public key
- Happy path: after a successful endorsement (via GitHub card), re-opening the popup shows "1 endorsement"
- Edge case: copy-key button still copies the full public key to the clipboard and flashes "Copied!"
- Edge case: `chrome.storage.local` missing `endorsement_count` renders "0" (no TypeScript runtime error)
- Integration: opening the popup does not send any `KEYRING_*` or `NETWORK_QUERY` messages (would log an error now that handlers are gone, so this is a regression guard for Unit 2)

**Verification:**
- Popup builds cleanly
- Manual load-unpacked: popup renders with public key + endorsement count + about link; no empty state
- `grep -n "keyring\|KEYRING\|Add to network" extension/src/popup.{ts,html,css}` returns no matches

## System-Wide Impact

- **Interaction graph:** `POST /network-query` and `NETWORK_QUERY` are gone. Only the `START_ENDORSEMENT` path and the one-shot `KEYRING_*`-free popup remain talking to the service worker.
- **Error propagation:** Previously, a `NETWORK_QUERY` failure in `content-github.ts` was swallowed by a try/catch — deleting the call removes an entire error branch. No behavior regression.
- **State lifecycle risks:** Existing values under `chrome.storage.local.keyring` become orphaned data. Benign (ignored forever, ~KB), but the implementer should add a one-line `chrome.storage.local.remove("keyring")` in the `onInstalled` listener's `reason === "update"` branch to tidy up. Low priority.
- **API surface parity:** `POST /network-query` removal is a breaking change for any out-of-tree client, but no such client exists. The trust card JSON (`endorsement_count`, `recent_endorsements`) is unchanged.
- **Integration coverage:** The `network_query_endpoint_removed` test added in Unit 1 locks in R1 as a regression guard.
- **Unchanged invariants:** Endorsements table schema, `endorser_key_hash` column, `POST /endorsements` handling of it, `GET /trust-card` response shape, Commit Score algorithm, trust card page SSR.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| A consumer (extension version older than this refactor) still posts to `/network-query` and fails silently | Acceptable — the content script's existing `catch {}` already swallows it. Non-critical path. |
| Deleting `setup_network_test_data` breaks a test helper used elsewhere | Grep before deleting; if used elsewhere, leave the helper and only remove the 10 `network_query_*` tests. |
| Popup endorsement count diverges from backend reality after extension reinstall | Documented. Counter is an interim local-only value until the "you endorsed this" indicator lands. |
| Merge friction with Plan D on `background.ts` | Different regions; mergeable. If both land in the same PR, Plan D's `onInstalled` branch goes in the top section, Unit 2's deletions in the dispatcher/helper sections. |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "Remove dead keyring code" checked
- Update the design doc reference at `~/.gstack/projects/commit/hakon-unknown-design-20260410-131531.md`: the `NetworkMembership` entity note already exists in `CLAUDE.md` marking it superseded — leave that note in place; this plan realizes the code-level removal

## Sources & References

- **Origin document:** [ceo-plans/2026-04-12-phase3-one-network-endorsements.md](~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md)
- Prior plan that introduced this code: [docs/plans/2026-04-12-006-feat-network-keyring-key-sharing-plan.md](../plans/2026-04-12-006-feat-network-keyring-key-sharing-plan.md)
- Institutional learning: [docs/solutions/best-practices/parallel-worktree-agent-workflow-2026-04-12.md](../solutions/best-practices/parallel-worktree-agent-workflow-2026-04-12.md)
