---
title: Trust page design audit batch fixes (8 findings)
type: fix
status: active
date: 2026-04-13
origin: ~/.gstack/projects/commit/designs/design-audit-20260412/design-audit-commit-backend.md
---

# Trust page design audit batch fixes (8 findings)

## Overview

Resolve all 8 findings from the 2026-04-12 design audit of the SSR trust page at `commit-backend.fly.dev/trust/{kind}/{id}`. Two HIGH (install CTA + absolute badge URLs), four MEDIUM (focus-visible, footer hardcode, score animation, badge user-select), two POLISH (root breadcrumb, mobile breakpoint). All changes live in `src/routes/trust_page.rs` and the inline `<style>` block within it.

## Problem Frame

The trust page is one of two growth-loop pickup points (CWS install is the other). It scored A on visual hierarchy and typography but C on interaction states and motion, and the HIGH findings actively break the workflow: badge markdown 404s when pasted into a real GitHub README, and the install CTA in the empty state is plain text where it should be the most prominent button on the page. The audit deferred all 8 to Phase 3 — this plan ships them as a single batch because they share one file and one CSS block.

## Requirements Trace

- R1 (FINDING-002, HIGH). Badge markdown uses absolute URLs so it works when pasted into any GitHub README.
- R2 (FINDING-001, HIGH). The endorsements empty state has a real `<a class="install-cta-btn">` button (matching the existing install CTA pattern) instead of plain text.
- R3 (FINDING-003, MEDIUM). All interactive elements (links, buttons, the new copy button) carry `:focus-visible` styles meeting WCAG AA contrast.
- R4 (FINDING-004, MEDIUM). Footer GitHub link is generic (commit org or product link), not a personal handle hardcode.
- R5 (FINDING-005, MEDIUM). Score circle animates a 400ms ease-out fill on first load per DESIGN.md.
- R6 (FINDING-006, MEDIUM). Badge code block has a Copy button; `user-select: all` is removed in favor of the explicit copy action.
- R7 (FINDING-007, POLISH). Root breadcrumb does not point to a 404 — either remove the link or land at a real route.
- R8 (FINDING-008, POLISH). Mobile breakpoint is 375px per DESIGN.md, not 480px.

## Scope Boundaries

- The trust page's content structure, copy, and information architecture are out of scope. This is polish + bug fix.
- The Chrome extension cards are not touched — separate work surfaces.
- No new server routes (the breadcrumb decision proposes removing the link, not adding a `/` handler — see Open Questions).
- No analytics, no A/B testing, no copy variations.

### Deferred to Separate Tasks

- A real landing page at `/`: deferred indefinitely. Resolution for FINDING-007 is to remove the `/` link from the breadcrumb, not to build a marketing page.
- Templating engine migration (askama/maud) for the trust page: out of scope. The inline HTML string approach stays for now; if it grows further it can be revisited.

## Context & Research

### Relevant Code and Patterns

- `src/routes/trust_page.rs:19–186` — `render_github_trust_page` orchestrates the page render.
- `src/routes/trust_page.rs:202–650` — `render_html()` builds the page as a single string. All HTML and inline `<style>` live here.
- `src/routes/trust_page.rs:258` — OG meta tag already hardcodes `https://commit-backend.fly.dev`. This is the only existing absolute-URL usage and the natural reference point for R1.
- `src/routes/trust_page.rs:299–308` — `.score-circle` styles (no animation today).
- `src/routes/trust_page.rs:526–544` — `.install-cta-btn` styles, including the only existing `:focus-visible` rule (line 541–544). The pattern is good; just needs to be applied to more elements.
- `src/routes/trust_page.rs:556–565` — `.badge-code` styles, with `user-select: all` on line 564.
- `src/routes/trust_page.rs:580–589` — `@media (max-width: 480px)` block.
- `src/routes/trust_page.rs:639` — badge markdown snippet generation (currently relative URLs).
- `src/routes/trust_page.rs:644` — footer GitHub link hardcoded to `hawkaa/commit`.
- `src/routes/trust_page.rs:660` — empty endorsements text.
- `src/routes/trust_page.rs:595` — root breadcrumb `<a href="/">`.
- `tests/api.rs:124–171+` — existing trust page tests; HTTP-level only, including `trust_page_includes_install_cta()` which is the obvious extension point for asserting the new empty-state button.
- `DESIGN.md` — animation spec for score fill at "400ms ease-out fill animation on first load." Mobile breakpoint inferred as 375px from the audit and DESIGN.md's responsive notes.

### Institutional Learnings

- CI gate parity learning (2026-04-12) — every push must clear `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`. Inline HTML strings are particularly easy to leave with stray whitespace; run `cargo fmt` after touching `render_html()`.

### External References

- WCAG 2.1 Success Criterion 2.4.7 (Focus Visible) — keyboard focus indicator must be visible. The existing `.install-cta-btn:focus-visible` rule (2px outline with offset) is a reasonable template.
- MV3 clipboard API: `navigator.clipboard.writeText()` works from a same-origin user-gesture handler in a regular page (no special permission needed). The fallback for older or restricted browsers is a hidden `<textarea>` + `document.execCommand('copy')`.

## Key Technical Decisions

- **Introduce a `PUBLIC_URL` const (with optional env var override).** Define `const DEFAULT_PUBLIC_URL: &str = "https://commit-backend.fly.dev";` near `CHROME_WEBSTORE_URL` at line 16. Read `std::env::var("PUBLIC_URL").unwrap_or_else(|_| DEFAULT_PUBLIC_URL.to_string())` once in `render_html()` and thread it where needed (badge markdown + OG image). Single source of truth, and dev environments can override.
- **Resolve FINDING-007 by removing the `/` link, not adding a route.** The breadcrumb stays as plain text "Commit › trust › github › owner/repo" with only the trailing item being a link if any. This is the smallest change consistent with reality (no landing page exists, building one is out of scope).
- **Score animation via CSS keyframes, not JS.** A single `@keyframes score-fill { from { stroke-dashoffset: <full-circle> } to { stroke-dashoffset: <target> } }` plus `animation: score-fill 400ms ease-out forwards` on the score arc. SSR can compute the target offset from the score value. No JS required; works without hydration. The numeric score readout fades in via `animation: fade-in 400ms ease-out` for cohesion.
- **Copy button uses `navigator.clipboard.writeText()` with a hidden-textarea fallback.** Tiny inline `<script>` block (the page already runs no JS today, so this is the first script — accept it as the price of the feature). The button toggles to "Copied!" for 1.5s on success.
- **Footer link points to the project repo, not a personal account.** Use `https://github.com/getcommit-dev/commit` if that's the organizational repo, otherwise drop the link entirely and replace with a generic "Made with Rust" or similar. The implementer should confirm the canonical repo URL with the founder before merging — flagged as Open Question.
- **`:focus-visible` applied via a single shared rule** with element selectors enumerated (links, buttons, the new copy button, the install CTA). Reuse the existing 2px outline with 2px offset and the brand violet (or whatever the existing CTA uses).
- **Mobile breakpoint changes from `max-width: 480px` to `max-width: 375px`.** This narrows the mobile-specific styles; verify that the current mobile rules still make sense at 375px (they should — they were already conservative).

## Open Questions

### Resolved During Planning

- **Remove `/` breadcrumb link or build a landing page?** Remove the link. Building a landing page is out of scope and the link currently leads to a broken experience.
- **Hardcoded base URL or env var?** Const with optional env override. Hardcoded value matches today's behavior; env var unblocks local dev.

### Deferred to Implementation

- **Canonical footer GitHub URL.** The audit calls out `hawkaa/commit` as personal. The implementer should confirm whether `getcommit-dev/commit` (or some other org) is the canonical project repo and use that URL; if no org repo exists, drop the link entirely and use a non-link footer.
- **Exact stroke-dashoffset math for the animation.** Depends on the SVG circle radius used today. The implementer should compute `circumference = 2 * π * r`, set the from/to values inline per render, and verify the start state is fully empty (full offset).

## Implementation Units

- [ ] **Unit 1: Public-URL helper + absolute badge markdown + OG image refactor**

**Goal:** Single source of truth for the public URL; badge markdown snippet uses absolute URLs.

**Requirements:** R1

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (add `DEFAULT_PUBLIC_URL` const, resolve `public_url` once in `render_html`, thread to badge generation at line 639 and OG image at line 258)
- Test: `tests/api.rs` (add `trust_page_badge_markdown_uses_absolute_urls`)

**Approach:**
- Read env var once at the top of `render_html` (or inside the helper that builds the badge snippet).
- Replace `[![Commit Score](/badge/github/{owner}/{repo}.svg)](/trust/github/{owner}/{repo})` with `[![Commit Score]({public_url}/badge/github/{owner}/{repo}.svg)]({public_url}/trust/github/{owner}/{repo})`.
- OG image already absolute — switch its hardcoded host to `{public_url}` for consistency.

**Patterns to follow:**
- Existing `CHROME_WEBSTORE_URL` const at line 16.

**Test scenarios:**
- Happy path — trust page response body contains the absolute badge markdown including `https://commit-backend.fly.dev/badge/...` and `https://commit-backend.fly.dev/trust/...`.
- Edge case — when `PUBLIC_URL` env var is set, the rendered markdown reflects that value (test sets the env var via `std::env::set_var` and resets afterward, or constructs the helper directly).

**Verification:**
- `cargo test` passes; manual paste of the badge snippet from a deployed page into a scratch GitHub README renders the badge without 404.

- [ ] **Unit 2: Real install button in endorsements empty state**

**Goal:** Replace plain "Install the Commit extension" text with a styled `<a class="install-cta-btn">` button that links to `CHROME_WEBSTORE_URL`.

**Requirements:** R2

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (line 660, the empty endorsements block)
- Test: `tests/api.rs` (extend `trust_page_includes_install_cta` or add a peer test asserting the empty-state button is present when the repo has no endorsements)

**Approach:**
- Render an `<a class="install-cta-btn install-cta-empty" href="{cta_url}">Install the Commit extension to endorse</a>` inside the empty state div, with a short supporting line above it.
- Reuse `.install-cta-btn` styling (lines 526–544); add a `.install-cta-empty` modifier for centering or spacing if needed inside `.endorsement-empty`.

**Patterns to follow:**
- The existing install CTA already on the page (lines 626–632) — this unit clones its treatment.

**Test scenarios:**
- Happy path — for a subject with zero endorsements, the response body contains a link with class `install-cta-btn` inside the endorsements empty state.
- Edge case — for a subject with at least one endorsement, the empty-state button is not rendered (the existing endorsement list shows instead).

**Verification:**
- `cargo test` passes; manual visual check on a no-endorsement repo shows the prominent button.

- [ ] **Unit 3: `:focus-visible` styles for all interactive elements**

**Goal:** Every link, button, and copy control has a visible focus ring meeting WCAG AA.

**Requirements:** R3

**Dependencies:** Unit 5 (Copy button must exist before its focus rule does; or land focus rules first and just include the selector in advance)

**Files:**
- Modify: `src/routes/trust_page.rs` (CSS block at lines 265–590)

**Approach:**
- Add a single shared rule: `a:focus-visible, button:focus-visible, .copy-btn:focus-visible { outline: 2px solid var(--accent-violet, #6E56CF); outline-offset: 2px; border-radius: 2px; }`.
- Drop the duplicate rule at lines 541–544 once the shared rule covers it (or keep it if `.install-cta-btn` needs a stronger treatment).

**Patterns to follow:**
- Existing `.install-cta-btn:focus-visible` at lines 541–544.

**Test scenarios:**
- This is a CSS-only change; verify via response-body substring presence of `:focus-visible` rule covering `a` and `button`.
- Manual: tabbing through the page in Chrome and Firefox shows visible outlines on the breadcrumb link, install CTA, badge link, footer link, and copy button.

**Verification:**
- `cargo test` passes; manual keyboard-only walkthrough confirms focus is visible at every stop.

- [ ] **Unit 4: Score circle 400ms ease-out fill animation**

**Goal:** Score arc animates from empty to its target value over 400ms with ease-out timing on first paint, per DESIGN.md.

**Requirements:** R5

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (CSS for `.score-circle` and the SVG arc; the SVG `stroke-dasharray`/`stroke-dashoffset` may need a dynamic value computed from the score)

**Approach:**
- Compute `circumference = 2.0 * std::f64::consts::PI * radius` (use the radius the SVG already uses).
- Set `stroke-dasharray = circumference`. Set `stroke-dashoffset = circumference * (1.0 - score / 100.0)` as the target.
- Add `@keyframes score-fill { from { stroke-dashoffset: {circumference}; } to { stroke-dashoffset: {target}; } }` and apply `animation: score-fill 400ms ease-out forwards;` to the arc element.
- Numeric score readout: `animation: fade-in 400ms ease-out;` for cohesion.

**Patterns to follow:**
- Existing `.score-circle` styles at lines 299–308 — extend, don't replace.

**Test scenarios:**
- Response body contains an `@keyframes score-fill` rule and the arc element references it via `animation:`.
- Edge case — score 0 still animates (ends at full offset, not crashing on division).
- Edge case — score 100 animates to offset 0.
- Manual: load the page; the score arc fills in 400ms with the easing profile DESIGN.md describes. No motion if the user has `prefers-reduced-motion: reduce` set (add a `@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }` guard).

**Verification:**
- `cargo test` passes; manual visual check across two browsers confirms the animation feels right.

- [ ] **Unit 5: Copy button replaces `user-select: all` on badge code**

**Goal:** Add a "Copy" button next to the badge markdown block; remove `user-select: all`. Button toggles to "Copied!" briefly on success.

**Requirements:** R6

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (badge code block markup around the existing `.badge-code` element; add `.copy-btn` styles and a small inline `<script>` for the click handler)

**Approach:**
- Markup: wrap badge code in a flex container with the `<pre class="badge-code">` and a `<button class="copy-btn" type="button" data-copy-target="badge-code">Copy</button>`.
- Remove `user-select: all` from `.badge-code`. Default user selection behavior is fine.
- Inline `<script>` (one block at end of body): selects all `[data-copy-target]` buttons, on click reads the target element's text content, calls `navigator.clipboard.writeText(...)`, sets button text to "Copied!" for 1.5s, then restores.
- Fallback: if `navigator.clipboard` is unavailable or rejects, create a hidden `<textarea>`, populate, select, `document.execCommand('copy')`, remove.

**Patterns to follow:**
- The page has no JS today; this introduces it. Keep the script under 30 lines, no dependencies.

**Test scenarios:**
- Response body contains a `.copy-btn` element adjacent to the badge code.
- Response body does NOT contain `user-select: all` in the `.badge-code` rule.
- Response body contains the inline `<script>` with a `data-copy-target` selector.
- Manual: click Copy, paste into another app, verify the markdown matches the rendered snippet. Click again with focus — focus ring appears (from Unit 3).

**Verification:**
- `cargo test` passes; manual click-and-paste confirms.

- [ ] **Unit 6: Footer link contextualization**

**Goal:** Replace the personal `hawkaa/commit` link with a project-canonical link or remove the link entirely if no canonical org URL exists.

**Requirements:** R4

**Dependencies:** None (but blocked on Open Question — confirm canonical URL with the founder)

**Files:**
- Modify: `src/routes/trust_page.rs` (line 644)

**Approach:**
- If a project org repo URL is confirmed: replace `https://github.com/hawkaa/commit` with that URL.
- Otherwise: drop the link, render the footer text without the GitHub anchor, and leave a TODO comment to add the link when the canonical URL is established.

**Patterns to follow:**
- Surrounding footer markup at line 644.

**Test scenarios:**
- Response body does NOT contain `hawkaa/commit`.
- If a link is present, response body contains the canonical URL.

**Verification:**
- `cargo test` passes; manual click in browser navigates to the right place (or no link is present, which is intentional).

- [ ] **Unit 7: Mobile breakpoint 480 → 375**

**Goal:** Mobile media query targets 375px per DESIGN.md.

**Requirements:** R8

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (line 580, `@media (max-width: 480px)`)

**Approach:**
- Change to `@media (max-width: 375px)`.
- Audit the rules inside the block — they should still make sense at 375px (probably more conservative than needed, which is fine). If any rule was specifically tuned for the 376–480px range, decide whether to lift it to a default style or drop it.

**Patterns to follow:**
- DESIGN.md responsive guidance.

**Test scenarios:**
- Response body contains `@media (max-width: 375px)` and not `@media (max-width: 480px)`.
- Manual: resize browser to 375px-wide simulator; layout still works.

**Verification:**
- `cargo test` passes; manual responsive check at 375 / 414 / 768 widths.

- [ ] **Unit 8: Root breadcrumb — remove the dead link**

**Goal:** First breadcrumb segment is no longer a link to `/` (which 404s today).

**Requirements:** R7

**Dependencies:** None

**Files:**
- Modify: `src/routes/trust_page.rs` (line 595, breadcrumb markup)

**Approach:**
- Replace `<a href="/">Commit</a>` (or similar) with `<span class="breadcrumb-root">Commit</span>` or just static text. Keep the visual treatment consistent with the breadcrumb font/color.
- No new route is added.

**Patterns to follow:**
- Existing breadcrumb markup at line 595.

**Test scenarios:**
- Response body's breadcrumb does not contain `href="/"`.
- Response body still contains the word `Commit` in the breadcrumb position.

**Verification:**
- `cargo test` passes; manual click on breadcrumb root no longer navigates anywhere broken.

## System-Wide Impact

- **Interaction graph:** All changes are confined to `src/routes/trust_page.rs` and its inline CSS/JS. No other routes, services, or extension surfaces are affected.
- **Error propagation:** N/A — UI polish, no new failure modes. The copy button handler must catch `clipboard.writeText` rejections and fall through to the textarea path.
- **State lifecycle risks:** None. All changes are render-time.
- **API surface parity:** The page response shape (HTTP status, headers) is unchanged. Only the body HTML changes.
- **Integration coverage:** Existing trust page tests continue to assert structure; new tests assert the specific findings (absolute URLs, empty-state button, copy button presence).
- **Unchanged invariants:** Score number, endorsement counts, OG image generation, signal grid layout, color palette — all unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Animation feels janky on slow devices | `prefers-reduced-motion` guard disables animation; 400ms ease-out is conservative and well within the perception budget |
| Copy button breaks in Firefox or older Chromium | Fallback path via hidden textarea + `execCommand('copy')` covers both |
| Removing `user-select: all` regresses the "select all by clicking" affordance some users like | Acceptable trade — Copy button is a stronger affordance, and click-to-select-all on an entire snippet is a non-standard Web pattern users don't expect |
| Footer link removal lands without a confirmed canonical URL | Implementer confirms with founder before merging Unit 6; otherwise drops the link and leaves a TODO |
| `:focus-visible` selector not supported in older browsers | All target browsers (modern Chrome, Firefox, Safari, Edge) support it; older browsers degrade to default focus rings, which is acceptable |

## Documentation / Operational Notes

- Update `CLAUDE.md` Phase 3 checklist: mark "Design fixes: absolute badge URLs, install CTA, focus-visible, score animation (8 findings from design audit)" complete.
- No new env vars required for production (PUBLIC_URL has a default). For local dev, `PUBLIC_URL=http://localhost:3000` produces correct local badge markdown.
- Verify after deploy: the canary trust page (`commit-backend.fly.dev/trust/github/tokio-rs/tokio`) renders the new layout, the badge snippet copies cleanly, and the score circle animates.

## Sources & References

- **Origin document:** `~/.gstack/projects/commit/designs/design-audit-20260412/design-audit-commit-backend.md`
- Related code: `src/routes/trust_page.rs:16,202–650`
- Related plans: `docs/plans/2026-04-13-002-feat-trust-page-get-extension-cta-plan.md` (the existing CTA whose pattern Unit 2 reuses), `docs/plans/2026-04-13-003-feat-github-card-badge-cta-plan.md` (badge clipboard pattern reference)
- DESIGN.md: animation spec line 58, mobile breakpoint at 375px (per audit)
