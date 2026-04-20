---
title: "SERP content script endorsement button: review-caught anti-patterns"
date: 2026-04-19
category: best-practices
module: extension
problem_type: best_practice
component: tooling
severity: medium
applies_when:
  - Adding an endorsement button to a content script without a sentiment flip (SERP vs GitHub card)
  - Sending chrome.runtime.sendMessage and consuming the result in TypeScript
  - Writing to or clearing chrome.storage.local inside an async success path
  - Multiple UI states share structurally identical reset/cleanup logic
  - Two content scripts share endorsed-cache state for the same subject
tags:
  - chrome-extension
  - content-script
  - endorsement
  - typescript
  - serp
  - button-state
  - typed-messaging
  - storage-error-handling
---

# SERP content script endorsement button: review-caught anti-patterns

## Context

When adding endorsement count display and a compact Endorse button to the Google SERP content script (`extension/src/content-google.ts`), the initial implementation was modeled on the GitHub card's `startEndorsement()` in `content-github.ts`. A 9-reviewer parallel code review caught five issues — all rooted in copying patterns between surfaces without accounting for surface-specific constraints.

The SERP surface differs from the GitHub card surface in one critical way: SERP has no "Not for me" button (CEO decision: too little context on SERP for negative signals). This eliminates the sentiment flip use case, which changes button lifecycle assumptions. The GitHub card's 3-second reset pattern exists specifically to allow Endorse <-> Not for me flipping — copying it to SERP was the root mistake.

The `chrome.runtime.sendMessage` untyped-`any` return is a recurring issue in this codebase (session history). The same gap appeared when implementing the GitHub card endorsement flow and was flagged again here.

## Guidance

### 1. Surface-specific button lifecycle: permanent disable vs timed reset

On surfaces with a sentiment flip (GitHub card: Endorse / Not for me), the button must reset after ~3 seconds so the user can switch sentiment. On surfaces with no sentiment flip (SERP), success should permanently disable the button. Never copy a timed-reset pattern between surfaces without checking whether a flip action exists.

```ts
// SERP: permanent disable on success — no sentiment flip
btn.textContent = "Endorsed \u2713";
btn.style.color = "#888";
btn.style.cursor = "default";
btn.classList.add("commit-endorse-indicator");
// No setTimeout reset

// GitHub card: timed reset to allow sentiment flip
btn.textContent = "Endorsed";
setTimeout(() => {
  btn.textContent = "Endorse";
  btn.disabled = false;
}, 3000);
```

### 2. Type the sendMessage result before accessing fields

`chrome.runtime.sendMessage` returns `Promise<any>`. Accessing `.success` or `.errorCode` without a type guard silently passes at compile time but fails unpredictably at runtime when the response shape is unexpected (e.g., service worker terminated mid-flight returning `undefined`).

```ts
interface EndorsementResult {
  success: boolean;
  errorCode?: string;
  error?: string;
}

const result = (await chrome.runtime.sendMessage({
  type: "START_ENDORSEMENT",
  repoOwner: owner,
  repoName: name,
  sentiment: "positive",
})) as EndorsementResult | undefined;

if (result?.success) { /* ... */ }
const label = errorCodeToLabel(result?.errorCode);
```

### 3. Isolate storage errors from endorsement success

Cache invalidation after endorsement (removing the stale trust-card entry) is best-effort. If `chrome.storage.local.remove()` throws — quota exceeded, extension context invalidated — the error must not propagate to the outer catch block where it would display "Offline" despite the endorsement having succeeded.

```ts
if (result?.success) {
  btn.textContent = "Endorsed \u2713";
  // Cache removal is best-effort; TTL expiry is the fallback
  try {
    await chrome.storage.local.remove(`trust-card:github:${repoId}`);
  } catch {
    // Non-fatal: stale cache expires via 1-hour TTL
  }
}
```

### 4. Extract a reset helper with a DOM-connected guard

When the same reset logic appears in error and catch branches, extract a helper. Include a `btn.isConnected` guard: content scripts run in live pages, and the card element may have been removed from the DOM (navigation, SPA route change) by the time the setTimeout fires.

```ts
function resetSerpBtn(btn: HTMLButtonElement, label: string): void {
  btn.textContent = label;
  btn.style.color = "#dc2626";
  setTimeout(() => {
    if (!btn.isConnected) return;
    btn.textContent = "Endorse";
    btn.style.color = "#7c3aed";
    btn.disabled = false;
  }, 3000);
}
```

### 5. Document cross-surface cache provenance

The endorsed-cache (`chrome.storage.local`) is shared across all content scripts via `endorsed-cache.ts`. An endorsement made on the GitHub card surfaces as a read-only indicator on the SERP card for the same subject. Any branch that reads from the shared cache must document this cross-surface coupling — especially the negative sentiment branch, which is reachable on SERP only via the GitHub card.

```ts
// Read-only indicator: negative sentiment is set from the GitHub card
// (SERP has no "Not for me" button per CEO decision). The shared
// endorsed-cache means a negative endorsement made on GitHub surfaces
// here as a muted indicator.
if (cachedEndorsement?.sentiment === "negative") {
  endorseBtn.textContent = "Not for me \u2713";
  endorseBtn.disabled = true;
}
```

Note: the background service worker calls `setEndorsement()` before returning the result to the content script, so by the time `result?.success` is true, the endorsed-cache already has the entry. Content scripts only read from the cache; they never write to it directly. (session history)

## Why This Matters

- **Permanent disable vs reset**: The 3s reset on SERP creates a window where users click "Endorse" again on already-endorsed repos, triggering a wasted TLSNotary proof and a 409 "Already endorsed" error — contradictory feedback after seeing "Endorsed" moments before.
- **Untyped sendMessage**: Silent `undefined` property access is a recurring source of errors in extension code. Chrome's messaging APIs are stringly-typed; the TypeScript compiler provides zero safety unless you add it explicitly.
- **Storage error propagation**: The outer catch block is the user-visible error path. Non-fatal cleanup operations must never escape into it.
- **DOM detachment**: Google SERP is a live-updating page. Card elements can be removed during async operations. The `isConnected` guard makes the detachment case explicit.
- **Cache provenance**: The shared endorsed-cache creates invisible cross-surface coupling. Without comments, removing or refactoring one surface's code unknowingly breaks another surface's indicator path.

## When to Apply

- Adding endorsement (or any interactive) button to a new Chrome extension surface
- Copying button state logic from one surface to another — verify the flip assumption
- Consuming `chrome.runtime.sendMessage` results in TypeScript content scripts
- Performing `chrome.storage.local` operations inside try/catch that also controls user-visible error UI
- Any content script async operation that writes back to DOM elements that may be removed by SPA navigation
- Two or more content scripts sharing state via `chrome.storage.local`

## Examples

**Before** (initial implementation with issues):

```ts
async function startSerpEndorsement(repoId: string, btn: HTMLButtonElement) {
  const [owner, name] = repoId.split("/");
  btn.disabled = true;
  btn.textContent = "Proving...";
  try {
    const result = await chrome.runtime.sendMessage({
      type: "START_ENDORSEMENT", repoOwner: owner, repoName: name, sentiment: "positive",
    });
    if (result.success) {
      btn.textContent = "Endorsed \u2713";
      await chrome.storage.local.remove(cacheKey);  // throws → shows "Offline"
      setTimeout(() => {  // wrong: SERP has no flip action
        btn.textContent = "Endorse"; btn.disabled = false;
      }, 3000);
    } else {
      btn.textContent = errorCodeToLabel(result.errorCode);
      setTimeout(() => { btn.textContent = "Endorse"; btn.disabled = false; }, 3000);  // duplicated
    }
  } catch {
    btn.textContent = "Offline";
    setTimeout(() => { btn.textContent = "Endorse"; btn.disabled = false; }, 3000);  // duplicated
  }
}
```

**After** (review-corrected):

```ts
interface EndorsementResult {
  success: boolean;
  errorCode?: string;
  error?: string;
}

function resetSerpBtn(btn: HTMLButtonElement, label: string): void {
  btn.textContent = label;
  btn.style.color = "#dc2626";
  setTimeout(() => {
    if (!btn.isConnected) return;
    btn.textContent = "Endorse";
    btn.style.color = "#7c3aed";
    btn.disabled = false;
  }, 3000);
}

async function startSerpEndorsement(repoId: string, btn: HTMLButtonElement) {
  // Caller guarantees owner/repo format (from API response or extractGithubRepo)
  const [owner, name] = repoId.split("/");
  btn.disabled = true;
  btn.textContent = "Proving...";
  try {
    // Cast is best-effort — no runtime validation. For tighter contracts, use a
    // manual typeof check or a schema validator (e.g., zod) on the result.
    const result = (await chrome.runtime.sendMessage({
      type: "START_ENDORSEMENT", repoOwner: owner, repoName: name, sentiment: "positive",
    })) as EndorsementResult | undefined;

    if (result?.success) {
      btn.textContent = "Endorsed \u2713";
      btn.style.color = "#888";
      btn.style.cursor = "default";
      btn.classList.add("commit-endorse-indicator");
      try {
        await chrome.storage.local.remove(`trust-card:github:${repoId}`);
      } catch { /* TTL expiry handles stale cache */ }
    } else {
      resetSerpBtn(btn, errorCodeToLabel(result?.errorCode));
    }
  } catch {
    resetSerpBtn(btn, "Offline");
  }
}
```

## Related

- `docs/solutions/logic-errors/promise-race-ghost-endorsement-after-timeout-2026-04-12.md` — sibling problem in the same endorsement flow: background.ts cancellation pattern for `Promise.race` timeout
- `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md` — foundational architecture for the three-layer message relay (content script -> background -> offscreen WASM) that these patterns sit on top of
