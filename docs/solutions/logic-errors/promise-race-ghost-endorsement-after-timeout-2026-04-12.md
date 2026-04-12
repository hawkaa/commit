---
title: Promise.race timeout allows ghost endorsement via uncancelled background flow
date: 2026-04-12
category: logic-errors
module: extension
problem_type: logic_error
component: frontend_stimulus
symptoms:
  - Timeout fires and UI shows "Timed out" but endorsement flow continues in background
  - POST /endorsements called after user-visible timeout creating ghost endorsement
  - Retry after timeout may create duplicate endorsements with different proof hashes
root_cause: async_timing
resolution_type: code_fix
severity: high
tags:
  - promise-race
  - timeout
  - cancellation
  - chrome-extension
  - ghost-request
  - async-timing
  - tlsnotary
---

# Promise.race timeout allows ghost endorsement via uncancelled background flow

## Problem

The endorsement timeout in the Chrome extension's background service worker used `Promise.race` to race a 60-second timeout against the endorsement flow. When the timeout won, the UI showed "Timed out" but the flow continued executing, completing proof generation and POSTing to `/endorsements` — creating an endorsement the user never saw confirmation for.

## Symptoms

- User sees "Timed out" in the extension UI, but an endorsement record is silently created on the backend
- On retry, a second TLSNotary session generates a new attestation (different proof_hash), bypassing the unique constraint and creating a duplicate endorsement
- Wasted compute: ~5-10s of WASM proof generation completes and submits even though the result is discarded from the user's perspective

## What Didn't Work

N/A — bug was caught during structured pre-merge code review of 3 parallel worktree branches, not discovered through runtime debugging. The review examined ~1300 lines of changes across Rust backend and TypeScript extension code.

## Solution

Introduce a shared cancellation flag as a mutable object passed into the endorsement flow. The timeout callback sets `cancelled = true` before resolving. The flow checks the flag after proof generation, before the API call, and skips submission if the timeout already fired.

Before (buggy):

```typescript
const ENDORSEMENT_TIMEOUT_MS = 60000;

async function handleStartEndorsement(msg: EndorsementMessage): Promise<ProveResult> {
  const timeoutPromise = new Promise<ProveResult>((resolve) =>
    setTimeout(
      () => resolve({ success: false, error: "Timeout", errorCode: "timeout" }),
      ENDORSEMENT_TIMEOUT_MS
    )
  );
  const flowPromise = runEndorsementFlow(repoOwner, repoName);
  return Promise.race([flowPromise, timeoutPromise]);
}

async function runEndorsementFlow(
  repoOwner: string,
  repoName: string
): Promise<ProveResult> {
  // ... proof generation (~5-10s) ...
  // ... POST /endorsements ...
  // This continues running even after timeout wins the race!
}
```

After (fixed):

```typescript
async function handleStartEndorsement(msg: EndorsementMessage): Promise<ProveResult> {
  // Shared cancellation flag — must be an object, not a primitive
  const state = { cancelled: false };

  const timeoutPromise = new Promise<ProveResult>((resolve) =>
    setTimeout(() => {
      state.cancelled = true;
      resolve({ success: false, error: "Timeout", errorCode: "timeout" });
    }, ENDORSEMENT_TIMEOUT_MS)
  );

  const flowPromise = runEndorsementFlow(repoOwner, repoName, state);
  return Promise.race([flowPromise, timeoutPromise]);
}

async function runEndorsementFlow(
  repoOwner: string,
  repoName: string,
  state: { cancelled: boolean }
): Promise<ProveResult> {
  // ... proof generation ...

  // Skip the API call if the timeout already fired
  if (state.cancelled) {
    console.warn("[commit] Flow completed after timeout — skipping submission");
    return { success: false, error: "Timeout", errorCode: "timeout" };
  }

  // ... POST /endorsements ...
}
```

## Why This Works

`Promise.race` is a resolution race, not a cancellation mechanism — the losing promise's async work continues unconditionally. The fix inserts a cooperative cancellation checkpoint at the side-effectful operation (the POST). The check fires after the expensive WASM proof work (which cannot be cancelled mid-execution) but before any state is mutated on the server.

The cancellation state must be a mutable object (`{ cancelled: boolean }`) rather than a primitive because it is passed as a function argument to `runEndorsementFlow`. JavaScript passes primitives by value to function parameters — the function receives a copy of `false`, not a reference to the outer `let` binding. Passing an object shares the reference, so both the timeout callback and the flow function observe the same memory location. (`AbortController`/`AbortSignal` is the idiomatic Web API alternative for fetch-based cancellation, but cannot abort mid-WASM execution — the shared-flag pattern covers the checkpoint between proof generation and the API call.)

## Prevention

**Code patterns:**
- Never use `Promise.race` alone when the losing promise has side effects (network calls, database writes, state mutations). Always pair with a cooperative cancellation mechanism.
- For flows with expensive pre-work followed by a side-effectful commit step, add a cancellation check at each commit boundary: `if (state.cancelled) return earlyResult`
- Prefer the shared-object pattern (`{ cancelled: boolean }`) over a `let` boolean when sharing mutable cancellation state across closures
- For `fetch`-based flows, consider `AbortController`/`AbortSignal` for network-level cancellation: `fetch(url, { signal: controller.signal })`

**Review checklist:**
- Any `Promise.race` usage: ask "what does the losing promise do after it loses?" If it has side effects, require a cancellation guard
- Timeout patterns racing against multi-step async flows: verify cancellation is checked at every state-mutating step
- Background service worker code: no user-visible error boundary exists once a background task outlives the UI response — side effects must be defensively guarded

## Related Issues

- `extension/src/background.ts` — primary fix location (commit `750898e`)
- `extension/src/offscreen-bundle.js` — related timeout pattern (uses `setTimeout` rejection for worker messages, different mechanism)
- `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md` — documents timeout patterns in the extension but scoped to offscreen-bundle.js, does not cover Promise.race cancellation
