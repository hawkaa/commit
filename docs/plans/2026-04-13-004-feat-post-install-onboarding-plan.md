---
title: "feat: Post-install onboarding page"
type: feat
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# feat: Post-install onboarding page

## Overview

When a user installs the Commit extension from the Chrome Web Store, nothing happens visibly — the extension silently generates a keypair and waits for the user to navigate to GitHub or Google. That silence is a growth-loop leak: visitors who installed specifically because they saw a trust page or badge lose momentum at the very moment they converted. Add a post-install onboarding tab that opens automatically on first install, explains what Commit does in 1-2 sentences, and points the user at where to go next.

## Problem Frame

The growth loop (CEO plan) depends on this step:

```
... → CWS install → [BLANK] → user eventually returns to GitHub → sees card → endorses
                       ▲
                       └── this plan fills this blank
```

Without an onboarding tab, the user's next action is entirely unguided. With one, we get 1-2 sentences of product explanation and a direct link back to either the trust page they came from or to a GitHub repo where they can see their first score.

The onboarding page is packaged with the extension (local HTML served via `chrome-extension://{id}/onboarding.html`) so it loads instantly, works offline, and can't be AB-tested server-side (fine — the content is stable).

## Requirements Trace

- R1. On `chrome.runtime.onInstalled` firing with `details.reason === "install"` (not `update`, not `chrome_update`), the service worker opens a new tab at `chrome-extension://{id}/onboarding.html`
- R2. The onboarding page explains Commit in 1-2 sentences, per DESIGN.md (Geist, `#f5f5f0` paper, `#1a1a2e` ink, 680px max width mirroring the trust page)
- R3. The page shows a primary CTA: "Visit a GitHub repo to see your first score" (link to `https://github.com/`) — always present, always visible
- R4. Existing `onInstalled` behavior (keypair generation) is preserved exactly — onboarding logic is additive, not a rewrite
- R5. Upgrading the extension (`reason === "update"`) must NOT open the onboarding tab
- R6. Webpack build includes `onboarding.html` and `onboarding.ts` in the output; the bundle size increase is minimal (one HTML, one small TS, no new runtime deps)
- R7. Extension's Playwright smoke test still passes; extension loads without console errors

## Scope Boundaries

- Not persisting "has seen onboarding" state — `onInstalled` fires exactly once per install, which is the correct lifecycle
- Not showing a tutorial or multi-step walkthrough — 1-2 sentences max, per CEO plan "minimal, not empty"
- Not detecting or redirecting back to a specific trust page the user came from. The CEO plan notes this would require storing the referrer before install, which Chrome Web Store does not reliably support. The generic "visit a GitHub repo" CTA is the planned fallback and meets the CEO's "If no referrer is available, show..." branch.
- Not adding telemetry

### Deferred to Separate Tasks

- Trust-page-referrer detection via a postMessage bridge or similar. Would require the trust page to set up a handshake before the user clicks "Get the extension", plus a web-accessible content script on the trust page origin. Out of scope; meaningful uplift but substantially more moving parts.

## Context & Research

### Relevant Code and Patterns

- `extension/src/background.ts:21-41` — existing `chrome.runtime.onInstalled` listener; currently doesn't use the `details` argument (which carries `reason`)
- `extension/src/popup.html` + `extension/src/popup.css` — existing pattern for an extension-local HTML page + CSS (follow the same `.popup-container` spacing cadence; the onboarding page is larger but the CSS approach is the same)
- `extension/webpack.config.js:9-18` — entry point declaration pattern (one TS file per entry)
- `extension/webpack.config.js:50-69` — `CopyWebpackPlugin` pattern used for `popup.html`, `offscreen.html`, `manifest.json`
- `src/routes/trust_page.rs` — DESIGN.md-compliant SSR page; matching its color palette and type scale on the onboarding page keeps the visual system consistent
- `DESIGN.md` — Geist stack, `#f5f5f0` paper background, primary button `#1a1a2e`, 680px max width for marketing-like pages

### Institutional Learnings

None directly applicable.

### External References

- `chrome.runtime.onInstalled` fires in three scenarios (`install`, `update`, `chrome_update`). Only `install` should trigger the tab open. Source: [Chrome docs — runtime.onInstalled](https://developer.chrome.com/docs/extensions/reference/api/runtime#event-onInstalled).
- `chrome.tabs.create` requires no additional permission when called from a service worker that owns the target URL (`chrome-extension://...`). No manifest change needed.

## Key Technical Decisions

- **Onboarding page is a plain static HTML file, no webpack bundling required for the HTML itself.** `onboarding.ts` is trivial (just a tiny handler for the CTA click if any interactivity is needed; otherwise the page can be static HTML with no script). Simplest approach: ship `onboarding.html` via `CopyWebpackPlugin` and either (a) skip `onboarding.ts` entirely if no interactivity is needed, or (b) include a tiny `onboarding.ts` entry for future extensibility. Choose (a) — less moving parts — unless the implementer has a reason to wire interactivity.
- **Inline CSS on the onboarding page.** Matches the trust page SSR pattern; avoids a separate CSS entry and a new MiniCssExtractPlugin chunk for a one-page surface.
- **CTA opens GitHub in the same tab.** The user is already looking at the onboarding page; we want them to move forward, not accumulate tabs.
- **No referrer detection in v1.** CEO plan explicitly allows the generic fallback. Deferred.

## Open Questions

### Resolved During Planning

- *Does this need a manifest permission change?* — No. `chrome.tabs.create` to an extension-owned URL requires no permission; `onInstalled` is always available.
- *Does the onboarding page need its own webpack entry point?* — Only if we want interactivity. For v1, static HTML via `CopyWebpackPlugin` is sufficient.
- *What happens if the user had a pre-existing keypair before upgrading?* — Unchanged. The existing `onInstalled` listener already has an idempotent "generate keypair if missing" check; this plan's onboarding branch runs *in addition to*, not instead of, that check.

### Deferred to Implementation

- Final copy for the two sentences of explanation. Short draft in the approach section below; implementer may refine after reading the final text aloud.

## Implementation Units

- [ ] **Unit 1: Create `onboarding.html` static page**

**Goal:** A self-contained, DESIGN.md-compliant welcome page packaged with the extension, reachable at `chrome-extension://{id}/onboarding.html`.

**Requirements:** R2, R3

**Dependencies:** None.

**Files:**
- Create: `extension/src/onboarding.html`

**Approach:**
- Single HTML file with inline CSS mirroring the trust page aesthetic (680px max width, `#f5f5f0` background, Geist web font from Google Fonts, `#1a1a2e` ink, `#e5e5e0` borders)
- Structure:
  - Top: small "Commit" wordmark in Geist 800, 16px
  - Hero block: score-circle brand mark (reuse the gradient pattern — the score circle is the brand mark per DESIGN.md) with a placeholder number, e.g., "—", as a decorative visual
  - Heading: "Commit is installed." (Geist 700, 24-28px)
  - Body: 1-2 sentences. Draft: *"See verifiable trust signals for any GitHub repo — right where you already make decisions. Your first score is one click away."*
  - Primary CTA: `<a class="cta" href="https://github.com/">Visit a GitHub repo</a>` styled as a primary button (`#1a1a2e` background, white text, 6px radius, 12px 24px padding, 14-16px Geist 600)
  - Secondary link (muted): `<a>` to `API_BASE` equivalent `https://commit-backend.fly.dev/` — "Learn more"
- No external JS dependencies. No tracking pixels. No service-worker-dependent behavior on the page itself.
- Accessibility: `<html lang="en">`, a single `<h1>`, sufficient contrast, focus-visible styles on the CTA

**Patterns to follow:**
- `src/routes/trust_page.rs` inline `<style>` block — colors, type scale, max-width
- `extension/src/popup.html` — HTML boilerplate shape (doctype, charset, viewport)

**Test scenarios:**
- Test expectation: none — static HTML with no logic. Unit 3's Playwright integration test asserts the page renders and the CTA is present.

**Verification:**
- File exists at `extension/src/onboarding.html`
- Manual browser open of the file (file://) renders the page with correct fonts and colors (Geist may fall back to system-sans-serif when opening via `file://` due to Google Fonts CORS — acceptable for spot-check; real verification is loading via `chrome-extension://`)

- [ ] **Unit 2: Wire webpack to copy `onboarding.html` into the build**

**Goal:** Ensure `npm run build` emits `onboarding.html` into `extension/build/`.

**Requirements:** R6

**Dependencies:** Unit 1 (file must exist).

**Files:**
- Modify: `extension/webpack.config.js`

**Approach:**
- Add `{ from: "src/onboarding.html", to: "onboarding.html" }` to the `CopyWebpackPlugin` patterns array (around line 55, alongside `popup.html` and `offscreen.html`)
- No new entry point; onboarding has no script in v1

**Patterns to follow:**
- Existing `{ from: "src/popup.html", to: "popup.html" }` entry at line 55

**Test scenarios:**
- Happy path: after `npm run build`, `extension/build/onboarding.html` exists and matches the source file byte-for-byte
- Edge case: adding the file does not increase the WASM/entrypoint size warnings (the existing `performance.maxEntrypointSize` should absorb a tiny static HTML)

**Verification:**
- `npm run build` succeeds
- `ls extension/build/onboarding.html` returns the file

- [ ] **Unit 3: Open onboarding tab on first install**

**Goal:** In the service worker, open the onboarding tab when `onInstalled.reason === "install"`. Preserve existing keypair-generation behavior.

**Requirements:** R1, R4, R5, R7

**Dependencies:** Units 1 and 2 (the HTML must be buildable; otherwise the new tab opens a 404).

**Files:**
- Modify: `extension/src/background.ts`
- Test: `extension/test/extension-smoke.spec.ts` (extend existing test — see scenarios)

**Approach:**
- Change the existing listener signature to accept `details`:
  ```
  chrome.runtime.onInstalled.addListener(async (details) => {
    // existing keypair-generation code, unchanged
    const existing = await chrome.storage.local.get("keypair");
    if (!existing.keypair) { /* ... unchanged ... */ }

    // NEW: open onboarding only on fresh install
    if (details.reason === "install") {
      await chrome.tabs.create({ url: chrome.runtime.getURL("onboarding.html") });
    }
  });
  ```
- Keep the two concerns distinct and well-commented. Keypair generation is guarded by `if (!existing.keypair)` already; onboarding is guarded by `reason === "install"`. Both are idempotent in intent.

**Coordination note:** Plan A (keyring removal) modifies the message-dispatch + helper sections of `background.ts` but does NOT touch `onInstalled`. Mergeable in any order.

**Patterns to follow:**
- Existing listener shape; follow it exactly plus the new branch

**Test scenarios:**
- Happy path (Playwright): launching Chromium with the unpacked extension fires `onInstalled({reason: "install"})`; assert that within 3 seconds a new tab exists whose URL is `chrome-extension://{id}/onboarding.html`. Extend `extension-smoke.spec.ts` — the existing test already has a `BrowserContext` handle, so `context.pages()` or `context.waitForEvent("page")` can catch the onboarding tab.
- Happy path (Playwright): on the onboarding tab, an element with the literal text "Visit a GitHub repo" is present and is an `<a>` with `href="https://github.com/"`.
- Edge case (Playwright or unit): simulating a second load of the same extension context does NOT open a second onboarding tab. (Playwright's `launchPersistentContext` with the same user-data-dir on a second call would exercise this — optional, skip if implementation cost is high.)
- Edge case: if `chrome.tabs.create` rejects for any reason, the error must not block the keypair-generation path — both should be independent `await` statements, not chained.
- Integration: existing "extension loads without errors" smoke assertion (`sw` has no console errors) still passes after adding the listener.

**Verification:**
- `npm run build` + `npm run test` green
- Manual load-unpacked: uninstall the extension, reinstall from the unpacked `extension/build/` folder, confirm the onboarding tab opens with the CTA visible
- Manual: disable + re-enable (which fires `reason: "update"`) does NOT open an onboarding tab

## System-Wide Impact

- **Interaction graph:** One new `chrome.tabs.create` call on install. No ongoing message passing.
- **Error propagation:** `tabs.create` failures logged, don't abort keypair generation. Acceptable degradation (user just doesn't see the onboarding tab; no functional impact).
- **State lifecycle risks:** None — `onInstalled` is the canonical single-fire event. No state persistence required.
- **API surface parity:** No backend contract changes.
- **Integration coverage:** Playwright smoke extended to assert the tab + CTA.
- **Unchanged invariants:** Keypair generation on install, cache cleanup alarm, offscreen-document creation, message-handling behavior, manifest permissions.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `onInstalled` fires for `reason: "update"` on every version bump | Guarded by `reason === "install"` check. R5 explicit. |
| Onboarding HTML drifts visually from the trust page / main product | Shared DESIGN.md colors and type scale; visual consistency is a manual review item before merge. |
| The tab opens but is blocked by a popup blocker | `chrome.tabs.create` is not subject to popup blockers (it's an extension API, not a `window.open` call). Not a concern. |
| User reinstalls extension repeatedly (dev loop) and finds the tab annoying | Dev flow only. The `--load-extension` flag in Playwright fires `onInstalled` once per launched context; development reload via `chrome://extensions` refresh fires `reason: "update"`, not `install`. Real-world friction is nil. |
| Plan A's deletions in `background.ts` collide with this plan's additions | Different regions: Plan A touches message dispatch + helpers; this plan touches the top-of-file `onInstalled` listener. Trivial merge. |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "Post-install onboarding page" as complete
- Update `extension/STORE_LISTING.md` if desired: mention onboarding in the description ("First-time users get a welcome page pointing them to their first score") — optional
- No telemetry, no privacy-policy changes

## Sources & References

- **Origin document:** [ceo-plans/2026-04-12-phase3-one-network-endorsements.md](~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md) (§Scope Decisions row 12, §Accepted Scope §12, §Growth Loop)
- Related plan (shares no files): `docs/plans/2026-04-13-002-feat-trust-page-get-extension-cta-plan.md` — the step *before* this in the growth loop
- Coordination note: `docs/plans/2026-04-13-001-refactor-remove-dead-keyring-code-plan.md` — touches different regions of `background.ts`; trivial merge
