---
title: "feat: Add 'Add badge to README' CTA to GitHub trust cards"
type: feat
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# feat: Add "Add badge to README" CTA to GitHub trust cards

## Overview

The `GET /badge/{kind}/{id}.svg` endpoint has existed since Phase 1b, and the trust page SSR shows a static markdown snippet — but the GitHub trust card (the surface a repo maintainer stares at most) doesn't expose either. Add a compact "Add badge" link to the extension-injected GitHub card that copies the markdown snippet to clipboard with a single click. This closes a maintainer-facing growth loop: maintainers spread the badge → badge links spread the trust page → trust page CTA converts installs.

## Problem Frame

Maintainers are the most motivated user segment in the growth loop. A repo owner seeing a score they like should have a one-click path to embed it. The current path is: click the score circle → open the trust page in a new tab → find the badge section → manually select the markdown → copy → paste into README. That's five friction points for what should be one click.

The CTA lives on the extension-injected card specifically — the trust page already has its own static badge block (this plan does not touch that). This is additive: a small chrome on top of the existing trust card.

## Requirements Trace

- R1. The GitHub trust card includes a compact, subdued "Add badge" link/button (secondary action — not competing with the primary "Endorse" CTA)
- R2. Clicking the link copies the markdown snippet `[![Commit Score]({API_BASE}/badge/github/{owner}/{repo}.svg)]({API_BASE}/trust/github/{owner}/{repo})` to the clipboard
- R3. Both URLs are absolute (use `API_BASE`) so the snippet works when pasted into any README
- R4. The button briefly flashes "Copied!" on success, then reverts to "Add badge" after ~1.5s
- R5. If `navigator.clipboard.writeText` fails or is unavailable, the UI falls back to showing the snippet in an inline selectable text field (`user-select: all`) so the user can copy manually
- R6. The CTA only appears on the GitHub content script's trust card — not on the SERP card (too compact) or the trust page (already has static block)
- R7. Extension builds cleanly; Playwright smoke test still passes; existing endorsement flow is unaffected

## Scope Boundaries

- Not redesigning the GitHub trust card layout
- Not adding the CTA to the SERP card (deferred — SERP card is a 28px compact surface per DESIGN.md, no room)
- Not adding a CTA to the trust page (it already renders a static badge block at `src/routes/trust_page.rs:580-586`)
- Not adding `clipboardWrite` to `manifest.json` — MV3 does not require it when the write happens in a user-gesture handler

### Deferred to Separate Tasks

- Unified "badge CTA" component across surfaces (CEO plan explicitly dropped unified-component extraction until a third surface is added)

## Context & Research

### Relevant Code and Patterns

- `extension/src/content-github.ts:90-155` — `createTrustCard` is the single injection site
- `extension/src/content-github.ts:143-149` — existing `.commit-endorse-btn` is the pattern the "Add badge" link should *contrast with* (subdued secondary, not a second primary button)
- `extension/src/trust-card.css` — shared card styling (file root, not yet read in full; follow the `.commit-card-network` muted-text pattern for the link's resting state and the `.commit-endorse-btn--done` momentary state for the "Copied!" flash)
- `extension/src/config.ts:4` — `API_BASE = "https://commit-backend.fly.dev"` — absolute URL source
- `extension/src/popup.ts:52-58` — existing `navigator.clipboard.writeText` pattern (copy public key, flash "Copied!", revert after 1500ms) — reuse this exact shape

### Institutional Learnings

- MV3 clipboard writes work from user-gesture handlers without the `clipboardWrite` manifest permission. The existing popup code (line 53) already relies on this — follow the same pattern.

### External References

- MDN: `navigator.clipboard.writeText` returns a Promise that rejects on permission / document-focus failures. In content scripts running on github.com, the clipboard permission inherits from the page's origin, which Chrome grants when the click is a trusted user gesture. Fallback path (R5) covers the remaining edge cases (e.g., iframe-injected cards).

## Key Technical Decisions

- **Link, not button.** The primary action on the card is "Endorse". The badge CTA is secondary maintainer tooling; making it a text link (styled like the existing `.commit-card-network` muted text with underline on hover) keeps visual hierarchy clean.
- **Snippet uses `API_BASE` for both badge and link URLs.** Ensures the markdown works when pasted into a README served from github.com and rendered anywhere — an absolute URL is required.
- **Fallback UI is inline, not a modal.** A small `<code>` block appears below the link, pre-selected via `user-select: all`. Less disruptive than a dialog, and if clipboard ever fails (e.g., focus lost), the user still has a one-gesture manual copy.
- **No analytics hook.** Explicit non-goal — we're not tracking clicks yet.

## Open Questions

### Resolved During Planning

- *Where does the CTA sit on the card?* — Below the details block (`.commit-card-details`), right-aligned on the same row as or directly under `.commit-endorse-btn`. Concretely: inside a new `.commit-card-secondary` row beneath the main card row, so the primary "Endorse" button stays visually dominant.
- *What does the link say?* — "Add badge". Short, action-oriented, matches the voice of "Endorse".

### Deferred to Implementation

- Exact positioning between "Add badge" and `.commit-card-network` (ZK endorsement count line). Both live in `.commit-card-details`; implementer may choose to place the link immediately below the network line or to the right of it. Low-stakes CSS decision.

## Implementation Units

- [ ] **Unit 1: Add "Add badge" link with clipboard copy + fallback to GitHub trust card**

**Goal:** Inject a compact "Add badge" link into the extension-rendered GitHub trust card. Clicking copies the markdown snippet; a fallback selectable text block appears if the clipboard write fails.

**Requirements:** R1, R2, R3, R4, R5, R7

**Dependencies:** None.

**Files:**
- Modify: `extension/src/content-github.ts`
- Modify: `extension/src/trust-card.css`
- Test: `extension/test/extension-smoke.spec.ts` (add assertion that the link renders — see scenarios)

**Approach:**
- In `createTrustCard`, construct the snippet string once at card creation time:
  ```
  const snippet = `[![Commit Score](${API_BASE}/badge/github/${subject.identifier}.svg)](${API_BASE}/trust/github/${subject.identifier})`;
  ```
- Create an "Add badge" anchor/span with class `.commit-add-badge` and an initial textContent of "Add badge". Attach a click handler that:
  1. Calls `navigator.clipboard.writeText(snippet)` inside a `try`
  2. On success: set text to "Copied!", add class `.commit-add-badge--done`, schedule `setTimeout` to revert after 1500ms
  3. On failure: create a `<code class="commit-badge-snippet">` sibling with `textContent = snippet`, insert it below the link, and style it `user-select: all; overflow-x: auto;`. Subsequent clicks should not create duplicates — keep a reference and toggle visibility.
- Append the "Add badge" element to `.commit-card-details` (so it sits with the other metadata, not competing with the endorse button). If desired, wrap `.commit-card-network` + `.commit-add-badge` in a flex row so they sit side by side.
- CSS (`trust-card.css`): add `.commit-add-badge` (11px, `#888`, underline on hover, cursor pointer), `.commit-add-badge--done` (text color shifts to `#16a34a` — green success), `.commit-badge-snippet` (11px JetBrains Mono, `#f5f5f0` background, 4px padding, 4px margin-top, 4px radius, block display, `user-select: all`, `overflow-x: auto`, `max-width: 100%`).

**Patterns to follow:**
- `extension/src/popup.ts:52-58` — exact clipboard copy / "Copied!" / revert pattern
- `extension/src/content-github.ts:124-129` — how new elements are appended to `.commit-card-details`
- DESIGN.md — Geist 11px for small labels, `#f5f5f0` for inline code backgrounds

**Test scenarios:**
- Happy path: on a repo with a score, the trust card contains an element matching `.commit-add-badge` with text `Add badge` (add to `extension/test/extension-smoke.spec.ts` — extend the existing "trust card appeared" check)
- Happy path: the computed snippet starts with `[![Commit Score](https://commit-backend.fly.dev/badge/github/` (assert via `page.evaluate` inspecting the click handler's closure OR by faking a click and reading `navigator.clipboard.readText` in the test — Playwright context supports clipboard permissions)
- Edge case: `navigator.clipboard.writeText` rejects — the `.commit-badge-snippet` fallback block appears with the full markdown and `user-select: all` is visible in computed styles
- Edge case: clicking "Add badge" twice in fast succession does not create two fallback blocks (re-entrant protection)
- Integration: the new link does not break the existing `.commit-endorse-btn` — Playwright smoke still sees both elements
- Integration: the card cache-hit path still renders the CTA (the snippet is derived from `subject.identifier`, which is cached — no extra fetch needed)

**Verification:**
- `npm run build` in `extension/` passes
- `npm run test` (Playwright smoke) in `extension/` passes
- Manual load-unpacked on `github.com/nickel-org/nickel.rs`: the "Add badge" link sits in the card details, clicking it copies the correct markdown (verify by pasting into a scratch file), fallback appears when clipboard is denied (test by running Chrome with clipboard permission revoked, or by temporarily forcing the `catch` branch)

## System-Wide Impact

- **Interaction graph:** One new DOM element per trust card, one new click handler, one call to `navigator.clipboard.writeText`. No network, no runtime messaging.
- **Error propagation:** Clipboard failures do not affect the endorse button or card rendering. Fallback is local, additive, and reversible.
- **State lifecycle risks:** None persistent. "Copied!" state is per-card and reverts by `setTimeout`.
- **API surface parity:** Trust page's existing static badge block is untouched — both surfaces now offer a badge snippet via different affordances, which is fine.
- **Integration coverage:** Playwright smoke already exercises trust-card injection; extending it to assert the CTA presence keeps coverage tight.
- **Unchanged invariants:** Endorsement flow, card fetch/cache logic, SERP card, trust page, backend endpoints.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Clipboard fails silently on some Chromium builds | Fallback UI (R5) — always-available manual copy path. |
| Users mistake "Add badge" for "Endorse" | Visual hierarchy: "Endorse" stays a button, "Add badge" is a muted 11px underline link. |
| Badge markdown URL changes later (e.g., custom domain migration) | Snippet is derived from `API_BASE`; change is one line in `config.ts`. |
| `clipboardWrite` permission gets required by a future Chrome version | Documented as deferred — would add to `manifest.json` if ever needed, not a breaking change. |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "Add badge to README" CTA as complete
- No store-listing changes needed (feature is organic discovery on existing surfaces)

## Sources & References

- **Origin document:** [ceo-plans/2026-04-12-phase3-one-network-endorsements.md](~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md) (§Scope Decisions row 7, §Accepted Scope §7)
- Badge endpoint: `src/routes/badge.rs` (unchanged, consumer only)
- Clipboard pattern reference: `extension/src/popup.ts:52-58`
- DESIGN.md — badge dimensions, type scale, and link color conventions
