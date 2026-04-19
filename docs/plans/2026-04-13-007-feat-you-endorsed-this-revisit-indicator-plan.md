---
title: "You endorsed this" revisit indicator
type: feat
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# "You endorsed this" revisit indicator

## Overview

Cache `(subject_kind, subject_id, sentiment)` tuples in `chrome.storage.local` after every successful endorsement. On trust card render, look up the current subject and, if found, show the endorsed state on the relevant button (`Endorsed ✓` for positive, `Not for me ✓` for negative — both in muted treatment) instead of the active call-to-action. Purely client-side; no backend endpoint, no server state.

## Problem Frame

Once a user endorses a repo, the trust card has no memory of that action across page loads. They see the same "Endorse" button as someone who has never engaged, which feels broken — their input vanished. The CEO plan accepts this as small effort, high delight: users get a quiet acknowledgement that their action was registered, and the cache also serves as the read side of the sentiment-flip behavior introduced in plan 005.

The cache is local-only by design. Building a "fetch my endorsements from the server" path would either require deanonymizing the endorser (defeating the ZK premise) or adding an authenticated read endpoint scoped by `endorser_key_hash` — both heavier than the value warrants. Reinstall losing history is acceptable at current scale.

## Requirements Trace

- R1. After a successful endorsement POST, the extension persists `(subject_kind, subject_id, sentiment, timestamp)` in `chrome.storage.local`.
- R2. On trust card render (GitHub and SERP), the extension reads the cache and reflects the persisted state on the relevant button(s) before showing the default CTA.
- R3. The cache is sentiment-aware so a flip from positive → negative (per plan 005) is reflected on the next render.
- R4. Cache writes happen for both insert and upsert (flip) flows — the cache mirrors the latest persisted sentiment.
- R5. Storage namespace is distinct from `keypair`, `trust-card:*`, and `endorsement_count`.

## Scope Boundaries

- Not adding a backend "my endorsements" endpoint. Cache is client-only.
- Not syncing across devices (no `chrome.storage.sync`). A user with the extension on two browsers won't see consistent revisit state across them.
- Not surfacing aggregate "you've endorsed N repos" stats — the popup already exposes a count separately.
- Not adding undo. To "undo" a positive endorsement, the user clicks "Not for me" (which flips it via plan 005). There's no neutral state.

### Deferred to Separate Tasks

- Sentiment storage backend support: depends on `docs/plans/2026-04-13-005-feat-not-for-me-negative-endorsement-plan.md`. This plan can ship with sentiment hardcoded to `"positive"` until plan 005 lands; once it does, the cache writer reads sentiment from the backend response or the user gesture.
- SERP card endorse button: SERP card doesn't currently surface an endorse button consistently across the page. When SERP parity ships in a future plan, the same cache read should drive the same revisit indicator.

## Context & Research

### Relevant Code and Patterns

- `extension/src/content-github.ts:84–184` — `createTrustCard()` builds the GitHub card. The endorse button section (lines 172–183) is where the read-side indicator applies.
- `extension/src/content-github.ts:186–243` — `startEndorsement()` flow. Success handling is where the write-side cache update can hook, but the cleaner home is the service worker (see Approach below).
- `extension/src/content-google.ts` — analogous SERP card render. Uses `chrome.storage.local` for `trust-card:*` cache; the same module structure suits the new cache.
- `extension/src/background.ts:122–131,179,241` — service worker handler for `START_ENDORSEMENT` and the actual POST. The success path here is the right place to write the cache: it sees the authoritative server response and runs even if the content script has been torn down.
- `extension/src/background.ts:24–44` — keypair storage pattern (`chrome.storage.local.get`/`set`). Reuse the same patterns; new namespace key.
- `extension/src/config.ts` — shared constants. Add the new storage key and any TTL/version constants here.
- `extension/test/extension-smoke.spec.ts` — Playwright smoke tests; extend with a render-after-endorse scenario.

### Institutional Learnings

- The 2026-04-12 docs/solutions entries on dead keyring code reinforce: don't build personal-network features that conflict with the one-network model. This cache stores only "what did this device endorse" — it's strictly local memory, not a network identity layer.
- Manifest V3 `chrome.storage.local` writes from a service worker survive the worker's idle suspension, so a write inside the POST success handler is durable.

### External References

- `chrome.storage.local` quota (~10MB) is more than enough for the foreseeable cache size — even at 10K endorsements per device, payload is well under 1MB.

## Key Technical Decisions

- **Single storage key `endorsed_subjects` holding a map.** Shape: `{ [`${kind}:${subject_id}`]: { sentiment: 'positive' | 'negative', timestamp: number } }`. Map (not array) for O(1) lookup and natural overwrite semantics on flip.
- **Service-worker-side write, content-script-side read.** The service worker writes after the backend POST returns 2xx, so the cache reflects authoritative state. Content scripts read on render — synchronously enough via `chrome.storage.local.get` (it's promise-based but fast; cards already do this for the trust-card cache).
- **Sentiment defaults to `'positive'` until plan 005 lands.** The cache shape is sentiment-aware from day one so plan 005 doesn't require a migration. Until then, every cached entry is `positive`.
- **No TTL on the cache.** Endorsement memory should persist indefinitely; the user will reinstall (and reset) eventually. A TTL would spuriously hide endorsements the user actually made.
- **Cache invalidation on flip is implicit via overwrite.** When the service worker writes after a successful flip, the same key overwrites with the new sentiment. No separate invalidation logic.
- **Eviction policy:** none. If the cache somehow grows past a few thousand entries (per-user reality), evict oldest by timestamp at write time. Bounded LRU keeps storage well under quota. Defer the eviction guard until profiling shows it's needed — note as a TODO.
- **Dedicated module for cache access.** New file `extension/src/endorsed-cache.ts` exporting `getEndorsement(kind, id): Promise<Endorsed | null>` and `setEndorsement(kind, id, sentiment): Promise<void>`. Keeps storage shape changes localized.

## Open Questions

### Resolved During Planning

- **TTL on cache?** No — endorsements are durable acts; their memory should be too.
- **Sync across devices?** No — out of scope; would need `chrome.storage.sync` and conflict resolution.
- **Where to write the cache — content script or background?** Background. Authoritative response, survives content-script teardown.
- **Cache shape?** Map keyed by `${kind}:${subject_id}` with `{ sentiment, timestamp }` value.

### Deferred to Implementation

- **Whether to import this new module from `content-google.ts` immediately or defer until SERP parity ships.** Recommendation: import and wire it now (read-side only on SERP) so the moment SERP gets an endorse button, the indicator is already there. Implementer should confirm SERP card has somewhere visible to render the muted state — if not, leave SERP wiring as a follow-up note in the same module.

## Implementation Units

- [ ] **Unit 1: Cache module — `endorsed-cache.ts`**

**Goal:** New module exposing `getEndorsement()` and `setEndorsement()` against `chrome.storage.local`. Single storage key, sentiment-aware shape.

**Requirements:** R1, R3, R5

**Dependencies:** None

**Files:**
- Create: `extension/src/endorsed-cache.ts`
- Modify: `extension/src/config.ts` (export `ENDORSED_CACHE_KEY = "endorsed_subjects"` and any related constants)
- Test: `extension/test/endorsed-cache.spec.ts` (new — unit test the module against a stub `chrome.storage.local`)

**Approach:**
- Module exposes:
  - `type Sentiment = 'positive' | 'negative'`
  - `type EndorsedEntry = { sentiment: Sentiment; timestamp: number }`
  - `async function getEndorsement(kind: string, subjectId: string): Promise<EndorsedEntry | null>`
  - `async function setEndorsement(kind: string, subjectId: string, sentiment: Sentiment): Promise<void>`
  - `async function clearAll(): Promise<void>` (test-only export, but useful for future "reset" flows)
- Internal: read the full map on each get (it's small), update key on set, write back. No in-memory cache layer — keeps the model simple and avoids stale state across SW suspensions.

**Patterns to follow:**
- `extension/src/background.ts:24–44` for the `chrome.storage.local.get/set` Promise pattern.

**Test scenarios:**
- Happy path — `setEndorsement('github', 'owner/repo', 'positive')` followed by `getEndorsement('github', 'owner/repo')` returns `{ sentiment: 'positive', timestamp: <number> }`.
- Happy path (flip) — `setEndorsement` with `'positive'` then `'negative'` for the same `(kind, id)` overwrites; `getEndorsement` returns `'negative'`.
- Edge case — `getEndorsement` for an unset key returns `null`, not undefined or throw.
- Edge case — keys with colons in the subject ID (e.g., `'github', 'owner/repo:branch'`) are handled correctly (use a non-colliding separator or escape).
- Edge case — module survives a malformed/preexisting value at the storage key (e.g., from a future migration) by treating it as empty rather than throwing.

**Verification:**
- `npm test` (or Playwright equivalent) passes; manual extension reload preserves the map.

- [ ] **Unit 2: Background writes cache on endorsement success**

**Goal:** After a successful POST to `/endorsements`, the service worker writes the cache entry with the sentiment that was sent.

**Requirements:** R1, R4

**Dependencies:** Unit 1

**Files:**
- Modify: `extension/src/background.ts` (around line 241, the POST success path; also bump `endorsement_count` adjacent to the cache write so both happen atomically)

**Approach:**
- Just after the 2xx response is received and parsed, call `setEndorsement(subject_kind, subject_id, sentiment)`.
- Until plan 005 ships, `sentiment` is hardcoded to `'positive'` at the call site. After plan 005, the value comes from the original `START_ENDORSEMENT` message (`sentiment` field).
- Don't write the cache on 4xx/5xx. The endorsement didn't persist; cache must reflect server state.

**Patterns to follow:**
- Existing post-success behavior at `extension/src/background.ts:241–260` (where `endorsement_count` is incremented).

**Test scenarios:**
- Happy path — successful POST writes the cache entry with the expected sentiment and current timestamp.
- Error path — failed POST (mocked 4xx or network error) does NOT write the cache.
- Integration — flip flow: positive endorsement, then negative endorsement on the same subject; cache reflects only the latest.

**Verification:**
- `npm run build` succeeds; Playwright smoke confirms the cache contains the expected entry after a mocked successful endorse.

- [ ] **Unit 3: GitHub content script reads cache and reflects state**

**Goal:** On `createTrustCard()`, look up the cached entry and render the muted indicator state on the relevant button(s) in place of the active CTA.

**Requirements:** R2, R3

**Dependencies:** Units 1, 2

**Files:**
- Modify: `extension/src/content-github.ts:172–183` (button section) and `:84–184` (`createTrustCard`)
- Modify: `extension/src/trust-card.css` (`.endorse-indicator` muted style — small, gray, no border, with a checkmark)

**Approach:**
- Inside `createTrustCard`, await `getEndorsement(subject.kind, subject.identifier)` early. The function is already async-friendly (or wrap the cache read in a small render-then-update if a sync path is preferred).
- If `entry` is null: render `Endorse` (and `Not for me` once plan 005 lands) as today.
- If `entry.sentiment === 'positive'`: render `Endorsed ✓` as a muted, non-interactive span (or a button styled as an indicator) where the primary `Endorse` button would have been. The `Not for me` link (post plan 005) remains active so users can flip.
- If `entry.sentiment === 'negative'`: render `Not for me ✓` muted on the secondary side; `Endorse` remains active for re-flipping.
- Indicator is non-interactive (no click handler) on the active-state side; clicking the still-active opposite-sentiment button performs a flip via the existing `START_ENDORSEMENT` flow.

**Patterns to follow:**
- DESIGN.md muted treatment for secondary actions; mirror the `endorse-secondary` styling that plan 005 introduces.

**Test scenarios:**
- Happy path — when the cache is empty for the current subject, the card renders the default endorse button(s).
- Happy path — when the cache contains `{ sentiment: 'positive' }` for the current subject, the primary slot renders `Endorsed ✓` muted; the secondary `Not for me` (if plan 005 has shipped) is still clickable.
- Happy path — when the cache contains `{ sentiment: 'negative' }`, the secondary slot renders `Not for me ✓`; `Endorse` remains active.
- Edge case — cache read fails or returns malformed data: card falls back to the default state without throwing or crashing the render.
- Integration — endorse flow end-to-end: render with empty cache → click `Endorse` → on success, the next render of the same card shows `Endorsed ✓`.

**Verification:**
- `npm run build` succeeds; Playwright smoke covers an end-to-end render-then-click-then-rerender; manual on a real GitHub repo confirms the muted state appears after endorsing.

- [ ] **Unit 4: SERP content script reads cache (read-only)**

**Goal:** SERP card render also reflects cached state. Write side is unchanged — SERP doesn't have an endorse button yet (per Scope Boundaries), but if the user endorsed via the GitHub card or trust page, the SERP card should show `Endorsed ✓` muted near the score.

**Requirements:** R2, R3

**Dependencies:** Units 1, 3 (shares the cache module and indicator styling)

**Files:**
- Modify: `extension/src/content-google.ts` (card render path)

**Approach:**
- Same `getEndorsement(kind, id)` call as Unit 3.
- If cached, render a small `Endorsed ✓` (or `Not for me ✓`) muted text near the compact score circle. Even if there's no endorse button to replace, the indicator alone provides continuity across surfaces.
- If SERP card markup has no obvious slot for the indicator, scope the indicator to "next to the score" or similar; document the placement decision in a comment.

**Patterns to follow:**
- Existing SERP card structure; mirror the muted indicator class from Unit 3.

**Test scenarios:**
- Happy path — when `endorsed_subjects` cache contains the subject (regardless of sentiment), SERP card renders the indicator.
- Edge case — cache miss: SERP card renders unchanged.

**Verification:**
- `npm run build` succeeds; manual on a Google search result for an endorsed repo shows the indicator.

- [ ] **Unit 5: Smoke + storage hygiene tests**

**Goal:** Playwright smoke covers the cache-write-then-read cycle. Confirm the new key doesn't collide with existing storage namespaces.

**Requirements:** R1, R2, R5

**Dependencies:** Units 1–4

**Files:**
- Modify: `extension/test/extension-smoke.spec.ts`
- Possibly: `extension/test/endorsed-cache.spec.ts` from Unit 1 (already covers module-level)

**Approach:**
- Add a smoke test that loads a GitHub repo page in the extension test browser, mocks the endorse POST to succeed, clicks `Endorse`, navigates away and back, and asserts the card now renders `Endorsed ✓`.
- Add an assertion that after the test, `chrome.storage.local` has both `keypair` (untouched) and `endorsed_subjects` (new entry) — confirming no namespace collision.

**Patterns to follow:**
- Existing Playwright setup at `extension/test/extension-smoke.spec.ts`.

**Test scenarios:**
- Integration — render → endorse → re-render shows the muted state.
- Integration — a second endorsement on a different subject leaves the first entry intact (map semantics).

**Verification:**
- `npm test` passes; CI gates green.

## System-Wide Impact

- **Interaction graph:** New module is consumed by `content-github.ts`, `content-google.ts`, and `background.ts`. No backend changes. No new permissions in `manifest.json` (`storage` is already granted).
- **Error propagation:** Cache read failures must not break card render — catch and treat as cache miss. Cache write failures (storage quota exceeded, etc.) should log to `console.warn` but not block the success notification to the user.
- **State lifecycle risks:** Service worker write happens after the POST success but before the user is notified. If the worker is suspended mid-flow, `chrome.storage.local.set` is durable, so the cache survives. If it doesn't, the user sees success but the cache misses — next render shows the default state. Acceptable.
- **API surface parity:** No public API changes. The `START_ENDORSEMENT` message shape gains an optional `sentiment` field (only meaningful once plan 005 lands).
- **Integration coverage:** The end-to-end render-endorse-rerender flow is the critical scenario; covered in Unit 5.
- **Unchanged invariants:** Keypair storage, trust-card cache, endorsement count, popup behavior, manifest permissions — all unchanged. Card rendering logic is additive: add a read at the top of `createTrustCard`, branch the button rendering accordingly.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Plan 005 hasn't landed yet — cache structure assumes sentiment | Sentiment defaults to `'positive'` at the call site until plan 005 lands. Cache shape is forward-compatible; no migration needed when 005 ships |
| Card render becomes async-blocking on cache read | `chrome.storage.local.get` is fast; if it ever feels slow, render the default state first then update once the cache resolves. Defer this optimization |
| User reinstalls extension and "loses" endorsement memory | Acceptable per CEO plan. Re-endorsement is the natural recovery path; the backend correctly upserts (per plan 005) |
| Storage quota exceeded at scale | At 10MB quota and ~50 bytes per entry, quota holds 200K entries. Realistic users are 4+ orders of magnitude below. LRU eviction is a TODO if profiling ever shows it matters |
| Race: user endorses, cache write begins, second render fires before write completes | Subsequent renders see the older state for a moment; next render after write completes shows correct state. No correctness issue, only a visual blink |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "'You endorsed this' revisit indicator" complete after merge.
- No new env vars, no manifest changes, no permission prompts.
- `endorsement_count` storage key keeps incrementing as today; the new `endorsed_subjects` key is additive.

## Sources & References

- **Origin document:** `~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md` (§6 "You endorsed this" revisit indicator)
- Related code: `extension/src/content-github.ts:84–243`, `extension/src/background.ts:24–260`, `extension/src/config.ts`
- Related plans: `docs/plans/2026-04-13-005-feat-not-for-me-negative-endorsement-plan.md` (sentiment field source of truth), `docs/plans/2026-04-13-002-feat-trust-page-get-extension-cta-plan.md` (related growth-loop context)
