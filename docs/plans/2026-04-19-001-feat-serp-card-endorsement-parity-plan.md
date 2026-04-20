---
title: "feat: Add endorsement count and endorse button to SERP cards"
type: feat
status: active
date: 2026-04-19
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# feat: Add endorsement count and endorse button to SERP cards

## Overview

The Google SERP content script (`content-google.ts`) currently shows only the Commit Score circle and text. The GitHub content script already has endorsement count display, endorse/not-for-me buttons, and full endorsement flow integration. This plan brings the SERP card to parity with the endorsement features available on GitHub cards, minus "Not for me" (CEO decision: too little context on SERP for negative signals).

## Problem Frame

Users who discover Commit through Google search results can see scores but cannot endorse inline. This breaks the secondary growth loop: developer searches → sees score → wants to endorse → has to navigate to the repo or trust page first. Adding the endorse button to SERP completes the "endorse everywhere" Phase 3 goal.

(see origin: CEO plan item #1 — SERP card parity)

## Requirements Trace

- R1. SERP cards display endorsement count (e.g., "3 ZK endorsements") when count > 0
- R2. SERP cards show a compact "Endorse" button that triggers the TLSNotary proof flow
- R3. No "Not for me" button on SERP cards (CEO decision — insufficient context)
- R4. Endorsed-cache revisit indicator continues to work (already implemented, must not regress)
- R5. Stale cached SERP entries (missing `endorsement_count`) degrade gracefully

## Scope Boundaries

- No "Not for me" button on SERP (CEO decision documented in file header)
- No signals breakdown or ZK tag on SERP (compact layout constraint, 28px circle)
- No "Add badge" CTA on SERP (GitHub-only per CEO plan item #7)
- No MutationObserver needed (Google SERP is not a SPA)
- No backend changes — the `/trust-card` API already returns `endorsement_count`

## Context & Research

### Relevant Code and Patterns

- `extension/src/content-github.ts` — reference implementation with full endorse flow (~400 lines)
- `extension/src/content-google.ts` — target file, currently 134 lines
- `extension/src/endorsed-cache.ts` — already imported and used by SERP script
- `extension/src/background.ts` — handles `START_ENDORSEMENT` from any content script, calls `setEndorsement()` on success
- `extension/src/trust-card.css` — has `.commit-endorse-btn`, `.commit-endorse-row`, `.commit-endorse-indicator` classes
- `extension/src/config.ts` — shared constants (`API_BASE`, `CACHE_TTL_MS`)

### Institutional Learnings

- **Promise.race ghost endorsement** (`docs/solutions/logic-errors/`): Any `Promise.race` usage must guard against the losing promise executing side effects. The cancellation flag pattern (`{ cancelled: boolean }` object, not primitive) is required. The SERP endorse flow doesn't use `Promise.race` directly (background handles it), but if timeout wrappers are added, this pattern applies.
- **Three-layer relay architecture** (`docs/solutions/best-practices/`): Content script → background service worker → offscreen document → WASM. The SERP script sends `START_ENDORSEMENT` to background; no direct WASM interaction needed.

### Key Interface Gap

`SerpTrustCardData` (lines 8-11 of `content-google.ts`):
```
{ subject: { identifier: string }, score: { score: number | null } }
```

`TrustCardData` in `content-github.ts` additionally has:
```
endorsement_count: number, recent_endorsements: [...], score.layer1_only, score.breakdown
```

The backend `/trust-card` endpoint returns the full response including `endorsement_count`. The SERP script casts to `SerpTrustCardData` and drops the extra fields. Widening the interface is the only change needed to surface endorsement count.

## Key Technical Decisions

- **Extend interface, don't replace**: Add `endorsement_count` to `SerpTrustCardData` rather than switching to the full `TrustCardData`. SERP doesn't need breakdown, recent_endorsements, or layer1_only — keeping the interface narrow signals what the SERP card actually uses.
- **Stale cache: graceful fallback, not versioned keys**: Old cached entries will have `endorsement_count: undefined`. Treat as 0 (don't show count). The 1-hour TTL handles natural expiry. No cache key versioning needed.
- **Inline `startEndorsement` and `errorCodeToLabel`**: The SERP version is simpler than GitHub's (no "Not for me" button, no optimistic count increment, no dual-button state management). Duplicating a simplified version is cleaner than extracting a shared module for two consumers with divergent behavior. If a third surface is added, extract then.
- **Compact text link, not full button**: CEO plan specifies the endorse button on SERP should be a compact text link ("Endorse" / "Endorsed"), not a full button. Fits the inline SERP card layout.

## Open Questions

### Resolved During Planning

- **Share or duplicate `startEndorsement`?** Duplicate a simplified version. The GitHub version handles dual-button state, optimistic count increment, and "Not for me" — none of which apply to SERP. Shared extraction adds indirection for two divergent consumers.
- **Cache invalidation strategy?** Graceful fallback. `endorsement_count ?? 0` handles stale entries. 1-hour TTL expires them naturally.

### Deferred to Implementation

- **Exact CSS for compact endorse link in `--light` theme**: The existing `.commit-endorse-btn` class may need minor adjustments for the compact SERP layout. Implementer should verify visual fit.

## Implementation Units

- [x] **Unit 1: Extend SERP interface and render endorsement count**

**Goal:** Show endorsement count on SERP cards when count > 0, matching GitHub card behavior.

**Requirements:** R1, R5

**Dependencies:** None

**Files:**
- Modify: `extension/src/content-google.ts`

**Approach:**
- Add `endorsement_count?: number` to `SerpTrustCardData` (optional for stale cache compat)
- In `createSerpCard()`, after the score text span, conditionally render endorsement count text (e.g., "· 3 endorsements") when `endorsement_count > 0`
- Use muted styling consistent with the SERP card's compact aesthetic (11px, `#70757a` like existing meta text)

**Patterns to follow:**
- `content-github.ts:126-131` — endorsement count rendering in `.commit-card-network`
- `content-google.ts:83-89` — existing meta span construction pattern

**Test scenarios:**
- Happy path: SERP card for a repo with 5 endorsements shows "· 5 endorsements" after score text
- Happy path: SERP card for a repo with 1 endorsement shows "· 1 endorsement" (singular)
- Edge case: SERP card for a repo with 0 endorsements shows no count text
- Edge case: Stale cached entry with `endorsement_count: undefined` renders without count (no error)

**Verification:**
- SERP cards on Google results for endorsed GitHub repos show the endorsement count inline

- [x] **Unit 2: Add compact Endorse button to SERP card**

**Goal:** Users can endorse repos directly from Google search results via a compact text link.

**Requirements:** R2, R3, R4

**Dependencies:** Unit 1

**Files:**
- Modify: `extension/src/content-google.ts`
- Modify: `extension/src/trust-card.css` (if compact button styling adjustments needed for `--light` theme)
- Test: `extension/test/extension-smoke.spec.ts`

**Approach:**
- Add a simplified `startEndorsement()` function that sends `START_ENDORSEMENT` to background with `sentiment: "positive"` only
- Add `errorCodeToLabel()` inline (same switch statement as GitHub, ~10 lines)
- In `createSerpCard()`, append a compact "Endorse" text link after the meta text. If `cachedEndorsement?.sentiment === "positive"`, render as muted "Endorsed ✓" indicator instead (already reading cache). If negative, show "Not for me ✓" (read-only indicator, no button).
- Button states: "Endorse" → "Proving..." (disabled) → "Endorsed" (success, 3s) → "Endorse" (reset) / error label (3s) → "Endorse" (reset)
- After success, clear the trust card cache for this repo (`chrome.storage.local.remove`) so next page load fetches fresh count. Background already calls `setEndorsement()` to update endorsed-cache.
- No optimistic count increment on SERP (the count text is inline, not a separate element worth updating live)

**Patterns to follow:**
- `content-github.ts:233-312` — `startEndorsement()` flow and error handling
- `content-github.ts:314-329` — `errorCodeToLabel()` mapping
- `content-google.ts:92-101` — existing revisit indicator rendering pattern

**Test scenarios:**
- Happy path: SERP card for an unendorsed repo shows "Endorse" text link
- Happy path: Clicking "Endorse" sends `START_ENDORSEMENT` message with `sentiment: "positive"` and correct repo owner/name
- Happy path: On success, button shows "Endorsed" then resets after 3s
- Edge case: Revisit after endorsing shows "Endorsed ✓" muted indicator (via endorsed-cache)
- Edge case: Revisit after negative endorsement (from GitHub card) shows "Not for me ✓" read-only on SERP
- Error path: Background returns `{ success: false, errorCode: "timeout" }` → button shows "Timed out" then resets
- Error path: `chrome.runtime.sendMessage` throws (background unreachable) → button shows "Offline" then resets
- Integration: Full flow — see score on SERP → click Endorse → TLSNotary proof runs → endorsement stored → trust card cache cleared

**Verification:**
- SERP cards show a functional endorse button that triggers the proof flow
- Revisit indicators display correctly for previously endorsed repos
- No "Not for me" button appears on SERP cards

## System-Wide Impact

- **Message passing:** `START_ENDORSEMENT` handler in `background.ts` already works from any content script — no changes needed
- **Endorsed-cache:** `setEndorsement()` is called by background on success, not by content scripts — SERP gets revisit indicators for free
- **Trust card cache:** SERP and GitHub scripts use the same cache key format (`trust-card:github:{owner/repo}`). Endorsing from SERP and then visiting GitHub (or vice versa) will show fresh data after cache clear
- **CSS:** SERP uses `--light` theme variant. Existing `.commit-endorse-btn` styles should work but may need minor width/padding adjustments for the compact inline layout
- **Unchanged invariants:** GitHub card behavior, background endorsement flow, backend endpoints, score algorithm — all unchanged

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Endorse button styling doesn't fit compact SERP layout | Use compact text link (not full button). Existing `--light` theme CSS as starting point, adjust padding/size if needed |
| Stale cache entries cause TypeScript errors | `endorsement_count` is optional in interface, defaulted to 0 in rendering logic |

## Sources & References

- **Origin document:** [CEO plan: Phase 3](~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md) — item #1
- **Test plan:** [Eng review test plan](~/.gstack/projects/commit/hakon-main-eng-review-test-plan-20260412-210049.md)
- Related code: `extension/src/content-github.ts` (reference implementation)
- Related code: `extension/src/content-google.ts` (target file)
- Learnings: `docs/solutions/logic-errors/promise-race-ghost-endorsement-after-timeout-2026-04-12.md`
