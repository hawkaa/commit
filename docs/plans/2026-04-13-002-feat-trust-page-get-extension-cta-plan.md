---
title: "feat: Add 'Get the extension' CTA to trust page"
type: feat
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md
---

# feat: Add "Get the extension" CTA to trust page

## Overview

The server-rendered trust page at `commit-backend.fly.dev/trust/github/{owner}/{repo}` currently shows a score, signals, and a static badge snippet — but no way forward for a visitor who wants to *act*. Add a prominent "Get the extension to endorse this" CTA that links to the Chrome Web Store listing, closing the growth loop step between "visitor sees trust card" and "visitor installs extension".

## Problem Frame

The growth loop (per the CEO plan) is:

```
GitHub repo → badge in README → trust page → CWS install → onboarding → endorse
                                       ▲
                                       └── missing: no CTA here today
```

The trust page is a marketing surface as much as a data surface: it is the page a maintainer shares from a README badge, and a link target from Google SERP score clicks. Without an install CTA it silently loses visitors.

The trust page is SSR Rust and has no way to detect whether the visitor has the extension installed. That's fine — we always show the CTA. If they already have it, they ignore it; if they don't, we just captured them.

## Requirements Trace

- R1. The trust page renders a visually prominent "Get the extension" CTA linking to the Chrome Web Store listing
- R2. The CTA is styled consistently with `DESIGN.md` (Geist, primary button `#1a1a2e` background / white text, 6px radius)
- R3. The CTA opens the CWS listing in a new tab (`target="_blank" rel="noopener"`)
- R4. `cargo test` and `cargo clippy -- -D warnings` pass; the existing `get_trust_page` tests remain green
- R5. The CTA is keyboard-accessible (standard `<a>`, visible focus state)

## Scope Boundaries

- Not changing the page's data model or cache behavior
- Not detecting whether the extension is installed (SSR can't)
- Not adding a "Get the extension" CTA to any other surface (GitHub card, SERP card, privacy page) — separate Phase 3 work

### Deferred to Separate Tasks

- Extension-installed detection (separate concern; requires a postMessage bridge or content script injection into trust page origin — out of scope here)
- Post-install onboarding page itself — see `docs/plans/2026-04-13-004-feat-post-install-onboarding-plan.md`

## Context & Research

### Relevant Code and Patterns

- `src/routes/trust_page.rs:200-596` — `render_html` function holds the entire SSR template including inline CSS
- `src/routes/trust_page.rs:580-586` — `badge-section` is the current natural anchor for a CTA block; the install CTA sits well *above* the badge section (maintainer-focused CTA → visitor-focused CTA → sharing widget)
- `src/routes/trust_page.rs:522-535` — `.footer` styling is the existing pattern for edge-of-page calls-to-action; this plan does not use the footer, but inherits color variables (`#1a1a2e`, `#666`, `#888`, `#e5e5e0`)
- `DESIGN.md` — primary button spec: `#1a1a2e` background, white text, 6px radius, Geist 600 weight

### Institutional Learnings

None directly applicable. Plan is Lightweight.

### External References

- Chrome Web Store extension URL format: `https://chromewebstore.google.com/detail/{slug}/{id}` — the exact slug and ID are assigned by the Web Store on publish. Pipe this in as a constant so the trust page can ship before publish is confirmed; a placeholder is acceptable during review and updated on publish.

## Key Technical Decisions

- **CTA is a hardcoded `<a>` link, not a config-driven component.** The trust page template is a monolithic `format!` string; staying consistent keeps the change to one file.
- **URL pipes through a `const` at the top of the module.** So that when the CWS listing URL changes (approval, re-slugging), it is a one-line edit.
- **Placement: between the endorsements card and the badge section.** The page narrative flows "what this repo is → what its signals are → what others say → how you can participate (CTA) → how you can share (badge)". That sequencing keeps the CTA in the visitor's eye path but below the data they came for.
- **Single primary CTA, no secondary copy.** A compact "Endorse this from GitHub or Google — Get the Commit extension" block. Avoid turning the trust page into a landing page; the extension install is one step in the loop, not the destination.

## Open Questions

### Resolved During Planning

- *Should the CTA only show for anonymous visitors?* — No. SSR can't tell. Always show.
- *Should the CTA be above or below the badge section?* — Above. Visitor action before maintainer action.

### Deferred to Implementation

- Final CWS listing URL. If the extension hasn't been assigned a permanent Web Store URL yet, use `https://chromewebstore.google.com/` as a fallback and file a follow-up to swap it after approval. This is a one-line change in the constant.

## Implementation Units

- [ ] **Unit 1: Add "Get the extension" CTA section to trust page SSR template**

**Goal:** Render a prominent, visitor-focused CTA card on the trust page that links to the Chrome Web Store listing.

**Requirements:** R1, R2, R3, R4, R5

**Dependencies:** None.

**Files:**
- Modify: `src/routes/trust_page.rs`
- Test: `tests/api.rs` (add CTA rendering assertions — new test, see scenarios)

**Approach:**
- Add a module-level `const CHROME_WEBSTORE_URL: &str = "...";` near the top of `trust_page.rs`. Use the actual CWS URL once known; placeholder during review.
- Inside `render_html`, add a new CTA block in the HTML body between `{endorsements_html}` and the `<div class="badge-section">` block. Shape:
  ```html
  <div class="install-cta">
    <div class="install-cta-text">
      <div class="install-cta-title">Endorse this repo</div>
      <div class="install-cta-subtitle">Add ZK-verified commitment signals with one click — on GitHub, Google, and everywhere you already browse.</div>
    </div>
    <a href="{CHROME_WEBSTORE_URL}" target="_blank" rel="noopener" class="install-cta-btn">Get the Commit extension</a>
  </div>
  ```
- Add corresponding CSS rules in the inline `<style>` block, following existing patterns:
  - `.install-cta`: white surface `#fff`, `#e5e5e0` border, 12px radius, 24px padding, 24px bottom margin, flex row on desktop (text block + button) with 16px gap, column on mobile
  - `.install-cta-title`: 16px Geist 700, `#1a1a2e`, 4px bottom margin
  - `.install-cta-subtitle`: 13px Geist 400, `#666`
  - `.install-cta-btn`: `#1a1a2e` background, `#fff` text, 10px 16px padding, 6px radius, 13px Geist 600, `text-decoration: none`, whitespace: nowrap. `:hover` opacity 0.9. `:focus-visible` 2px `#16a34a` outline with 2px offset (R5).
  - Mobile breakpoint (`@media (max-width: 480px)`): stack vertically, button full-width
- Keep the change surgical — no refactoring of surrounding render helpers.

**Patterns to follow:**
- `.badge-section` (lines 501-507) for the card shell
- `.endorsement-onchain` (lines 476-491) for compact link-styled-as-button patterns
- Existing `@media (max-width: 480px)` block at lines 536-543 for mobile stacking

**Test scenarios:**
- Happy path: GET `/trust/github/{owner}/{repo}` for a subject with endorsements returns HTML containing the string `Get the Commit extension` and the CWS URL (assert in a new `trust_page_includes_install_cta` test)
- Happy path: the CTA link has `target="_blank"` and `rel="noopener"` in the response HTML (single grep assertion)
- Edge case: the CTA is present even when the subject has zero endorsements (no conditional gating)
- Edge case: existing `trust_page_github_repo_*` tests continue to pass — this plan adds content, it does not change any preexisting assertion

**Verification:**
- `cargo test` green, new CTA test green
- `cargo clippy -- -D warnings` green
- Manual: `cargo run` + visit `http://localhost:3000/trust/github/nickel-org/nickel.rs` — CTA renders above the badge section, button contrasts with surrounding cards, Tab-focusing the link shows the green outline, mobile viewport stacks the button

## System-Wide Impact

- **Interaction graph:** No new runtime paths. One outbound link added.
- **Error propagation:** None — static HTML.
- **State lifecycle risks:** None — no state.
- **API surface parity:** None — trust page is a human-facing HTML endpoint, not a JSON API.
- **Integration coverage:** New assertion in `tests/api.rs` locks in CTA presence.
- **Unchanged invariants:** `GET /trust-card` JSON contract, `GET /trust/{kind}/{id}` response status, cache headers, OG meta tags, score rendering, endorsement list rendering, badge section.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| CWS listing URL not yet stable at merge time | Ship with placeholder; one-line swap in the `const` after publish. The test asserts `Get the Commit extension` text, not the exact URL host. |
| CTA styling competes with the score hero | Visual review in a browser before merge; the spec above keeps the CTA below the endorsements card, so hero remains top-of-fold. |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "Trust page: add 'Get extension' CTA (growth loop)" as complete
- If CWS URL changes post-approval, update the `CHROME_WEBSTORE_URL` constant (tracked as a TODO comment in the source)

## Sources & References

- **Origin document:** [ceo-plans/2026-04-12-phase3-one-network-endorsements.md](~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md) (§Scope Decisions row 2, §Growth Loop)
- Related file: `src/routes/trust_page.rs`
- Related plan (shares no files): `docs/plans/2026-04-13-004-feat-post-install-onboarding-plan.md` — the destination after install
