---
title: "feat: End-to-end ZK endorsement flow"
type: feat
status: active
date: 2026-04-12
---

# feat: End-to-end ZK endorsement flow

## Overview

Wire up the complete endorsement path so a user on a GitHub repo page can click "Endorse", watch a ZK proof generate via TLSNotary WASM in the offscreen document, have the proof submitted to the backend, verified, stored, and then see the endorsement reflected in the trust card UI. The individual pieces (extension WASM proving, backend attestation verification, content script injection, trust card SSR) exist and are tested. This plan connects them into a working user-visible flow.

## Problem Frame

The endorsement flow is built in isolation pieces that have never executed end-to-end:

1. The content script already has an Endorse button and wired `START_ENDORSEMENT` message handling, but no user has successfully completed the flow because the proving + submission chain hasn't been verified against the deployed notary server.
2. The trust card API (`GET /trust-card`) returns `Subject` with an `endorsement_count` field, but it's always 0 -- endorsements are never counted back into the response.
3. The trust card SSR page shows "Endorse this repo to improve accuracy" in the footer but displays no endorsement data.
4. The extension shows "Endorsed" or "Failed" text on the button but gives no persistent indication that the repo has endorsements (the card shows the same data on next page load).
5. The `POST /endorsements` path creates `pending_attestation` status. There is no follow-up process that transitions it to `verified` via this path. The webhook path (`POST /webhook/endorsement`) creates endorsements as `verified`, but nothing currently triggers it. The direct-submission path is the active one.
6. The `PROXY_BASE` still points to `wss://notary.pse.dev/proxy` -- adequate for now, but needs to be confirmed working with the own notary server at `commit-verifier.fly.dev`.

## Requirements Trace

- R1. A user visiting a GitHub repo page with the extension installed can click "Endorse" and complete a ZK endorsement within ~10s
- R2. The extension provides clear visual feedback during proving (loading), on success, and on failure
- R3. The backend trust card API includes endorsement count and endorsement data so the extension can display it
- R4. The extension content script and trust card SSR page both display endorsement count when endorsements exist
- R5. The endorsement status model is coherent -- direct submissions that pass attestation signature verification should be `verified`, not `pending_attestation`
- R6. The flow works against the deployed `commit-verifier.fly.dev` notary and `commit-backend.fly.dev` backend

## Scope Boundaries

- The WebSocket proxy (`wss://notary.pse.dev/proxy`) is a stateless byte bridge and remains on PSE's infrastructure. Deploying our own proxy is deferred.
- Google SERP content script does not get an Endorse button (GitHub only for Phase 2).
- No new proof types (email, ci_logs). Only `git_history` is supported.
- No Ed25519 signing of endorsement submissions (deferred to score integrity follow-up).
- No network keyring / "N in your network" display (Phase 3).
- No L2 on-chain attestation submission (the `attestations` table stays in `pending` state).

### Deferred to Separate Tasks

- Own WebSocket proxy deployment: when PSE proxy becomes unreliable
- Score v2 (Layer 2 integration): requires more endorsement volume to be meaningful
- "N in your network endorse this" display: Phase 3 feature
- Nonce-based replay prevention: separate security follow-up

## Context & Research

### Relevant Code and Patterns

**Extension entry points:**
- `extension/src/content-github.ts` -- injects trust card, has Endorse button + `startEndorsement()` handler
- `extension/src/background.ts` -- `handleStartEndorsement()` orchestrates offscreen + API submission
- `extension/src/prove-worker.ts` -- Web Worker running TLSNotary WASM via `tlsn-js`
- `extension/src/offscreen-bundle.js` -- message relay between background and prove worker
- `extension/src/offscreen.html` -- offscreen document hosting the worker
- `extension/src/config.ts` -- `API_BASE`, `NOTARY_URL`, `PROXY_BASE` constants
- `extension/src/trust-card.css` -- styles for trust card, score circle, endorse button (existing)
- `extension/src/manifest.json` -- Manifest V3, permissions for commit-verifier.fly.dev and notary.pse.dev

**Backend routes:**
- `src/routes/endorsement.rs` -- `POST /endorsements` (direct submission), `GET /endorsements` (query by subject)
- `src/routes/webhook.rs` -- `POST /webhook/endorsement` (verifier callback, creates `verified` status)
- `src/routes/trust_card.rs` -- `GET /trust-card` returns `TrustCardResponse { subject, signals, score }`
- `src/routes/trust_page.rs` -- SSR trust card page at `/trust/{kind}/{id}`

**Backend services:**
- `src/services/db.rs` -- `get_endorsement_count()` exists but is never called, `get_endorsements_for_subject()` works
- `src/validation.rs` -- `validate_transcript_subject()`, `verify_attestation_signature()`
- `src/services/score.rs` -- `score_github_repo()` (Layer 1 only, does not use endorsement data)

**Models:**
- `src/models/subject.rs` -- `Subject` has `endorsement_count: u32` field, always set to 0
- `src/models/endorsement.rs` -- `ProofType`, `EndorsementCategory`, `AttestationStatus`
- `src/models/signal.rs` -- `CommitScore`, `ScoreBreakdown` with Layer 2 fields (currently zeroed)

**Verifier server:**
- `verifier/Dockerfile` -- `tlsn/notary-server:v0.1.0-alpha.12`
- `verifier/config.yaml` -- port 7047, secp256k1 signing, auth disabled
- `verifier/fly.toml` -- deployed as `commit-verifier`, auto-stop enabled

### Flow Architecture

The E2E flow has two possible paths. This plan uses the **direct submission path** (Path A) because it's simpler and already has attestation signature verification:

**Path A (direct submission) -- chosen:**
1. User clicks Endorse in content script
2. Background sends `PROVE_ENDORSEMENT` to offscreen document
3. Offscreen relays to prove-worker Web Worker
4. Worker runs TLSNotary WASM: connects to `commit-verifier.fly.dev` for notarization, proxies through `notary.pse.dev/proxy` to reach `api.github.com`
5. Worker returns attestation + transcript to background
6. Background POSTs to `commit-backend.fly.dev/endorsements` with attestation + transcript
7. Backend verifies transcript binding, verifies attestation signature (notary public key), hashes attestation into proof_hash, stores endorsement
8. Backend returns endorsement ID + status

**Path B (webhook) -- not used for E2E:**
The webhook path (`POST /webhook/endorsement`) was designed for a verifier server that actively calls back after verification. The TLSNotary notary server does not have this behavior -- it's a notarization server, not a verifier. The webhook path remains available for future use (e.g., batch re-verification) but is not part of the E2E flow.

### Status Model Decision

The direct submission path currently hardcodes `pending_attestation` status. With attestation signature verification enabled (notary public key configured), a successfully-verified direct submission has the same cryptographic assurance as the webhook path. The status should reflect this: when `verify_attestation_signature()` passes, the endorsement should be `verified`.

## Key Technical Decisions

- **Direct submission path, not webhook:** The own notary server is a notarization server (it signs attestations), not a verifier that calls back. The direct path (`POST /endorsements`) already verifies the attestation signature server-side, making it equivalent in trust level.
- **Status promotion on signature verification:** When `verify_attestation_signature()` succeeds, set status to `verified` instead of `pending_attestation`. When the notary public key is not configured (dev mode), keep `pending_attestation` as the conservative default.
- **Endorsement count from DB, not Subject field:** The `Subject.endorsement_count` field is always 0 because `upsert_subject()` doesn't maintain it. Instead of adding trigger/update complexity, query `get_endorsement_count()` when building the trust card response. The `endorsement_count` on `Subject` becomes a convenience field populated at query time.
- **Endorsement data in trust card API response:** Add an `endorsement_count` field and a `recent_endorsements` array to `TrustCardResponse`. The extension can display "3 ZK endorsements" without a separate API call.
- **No separate endorsement fetch from extension:** Avoid adding a second API call from the content script. The trust card API response will include endorsement data, keeping the extension's network requests minimal.
- **PSE proxy retained:** `wss://notary.pse.dev/proxy` is a stateless TCP-over-WebSocket bridge. No secrets or attestation data flow through it. Deploying our own proxy is not blocking.

## Implementation Units

- [ ] **Unit 1: Backend -- Promote endorsement status on attestation verification**

  **Goal:** When `verify_attestation_signature()` succeeds in the direct submission path, set the endorsement status to `verified` instead of `pending_attestation`.

  **Approach:**
  - In `src/routes/endorsement.rs`, after the attestation signature verification block, track whether verification passed.
  - If the notary public key is configured and verification succeeded, call `db.update_endorsement_status(&endorsement_id, "verified")` after `create_endorsement()`.
  - If the notary public key is not configured (dev/test), keep the existing `pending_attestation` default.
  - Update the `EndorsementResponse` to reflect the actual status (`verified` or `pending_attestation`).

  **Files:** `src/routes/endorsement.rs`

  **Tests:** Existing integration tests cover the route. Add a unit test that verifies status is `pending_attestation` when no key is configured vs. `verified` when it is (mock-based or using the existing test helpers).

- [ ] **Unit 2: Backend -- Surface endorsement data in trust card API**

  **Goal:** Include endorsement count and recent endorsements in `TrustCardResponse` so the extension and trust card page can display them without extra API calls.

  **Approach:**
  - Add `endorsement_count: u32` and `recent_endorsements: Vec<EndorsementSummary>` fields to `TrustCardResponse` in `src/routes/trust_card.rs`.
  - After fetching/caching the subject, call `db.get_endorsement_count(&subject.id)` and `db.get_endorsements_for_subject(&subject.id)` (limited to 5 most recent).
  - Reuse the `EndorsementSummary` struct from `src/routes/endorsement.rs` (move it to a shared location or re-export).
  - Populate `Subject.endorsement_count` with the actual count before returning.
  - Add a `get_recent_endorsements()` method to `Database` that returns the N most recent endorsements for a subject (SELECT with LIMIT), to avoid returning unbounded data.

  **Files:** `src/routes/trust_card.rs`, `src/services/db.rs`, `src/routes/endorsement.rs` (for shared struct)

  **Tests:** Test that `GET /trust-card` includes endorsement data when endorsements exist for a subject.

- [ ] **Unit 3: Backend -- Display endorsements on trust card SSR page**

  **Goal:** The server-rendered trust card page at `/trust/{kind}/{id}` shows endorsement count and a list of recent endorsements with ZK verification badges.

  **Approach:**
  - In `src/routes/trust_page.rs`, after fetching the subject, query endorsement count and recent endorsements (same as Unit 2).
  - Add a new card section in `render_html()` between the score breakdown and badge section: "Endorsements" card with count header and a list of recent endorsements.
  - Each endorsement row shows: category, proof type, status badge (verified = ZK violet tag, pending = gray tag), and relative timestamp.
  - Use the ZK accent color (`#7c3aed`) and the `.layer-badge-zk` pattern already defined in the page CSS for verified endorsements.
  - If zero endorsements, show a subtle CTA: "No endorsements yet. Install the Commit extension to endorse this repo."
  - Pass endorsement count to `render_html()` as an additional parameter.

  **Files:** `src/routes/trust_page.rs`

  **Design notes:** Follow DESIGN.md -- card with `.card` class, `.card-title` uppercase label, ZK violet tag for verified status. No new decorative elements.

- [ ] **Unit 4: Extension -- Display endorsement data in GitHub content script trust card**

  **Goal:** The injected trust card on GitHub repo pages shows endorsement count with a ZK verification badge when endorsements exist.

  **Approach:**
  - Update the `TrustCardData` interface in `extension/src/content-github.ts` to include `endorsement_count` and optionally `recent_endorsements` from the API response.
  - In `createTrustCard()`, after the signals line, add a network/endorsement line when `endorsement_count > 0`: show "{N} ZK endorsements" using the existing `.commit-card-network` class (violet text) and a `.commit-zk-tag` inline badge.
  - When endorsements are present, the Endorse button should still be shown (users can endorse multiple times -- the unique constraint is on `proof_hash`, preventing duplicate attestations but not repeat endorsements from different proving sessions).
  - After a successful endorsement (`btn.textContent = "Endorsed"`), optimistically increment the displayed endorsement count by 1 without refetching.
  - Clear the trust card cache key for this repo after successful endorsement so the next page load fetches fresh data.

  **Files:** `extension/src/content-github.ts`, `extension/src/trust-card.css` (if any new styles needed, though existing `.commit-card-network` and `.commit-zk-tag` should suffice)

  **Design notes:** Follow DESIGN.md -- ZK accent `#7c3aed`, 9px tag with violet-10% background. The endorsement line sits below the signals line in the card details block.

- [ ] **Unit 5: Extension -- Verify and harden endorsement flow UX**

  **Goal:** The endorsement flow provides clear, trustworthy feedback at every stage and handles all failure modes gracefully.

  **Approach:**
  - **Loading state:** The existing "Proving..." text + shimmer animation on the button is adequate for the ~5-10s proving time. No spinner or progress bar needed -- the button animation is visible and the timeframe is short.
  - **Success state:** After "Endorsed" is shown, add a brief checkmark or transition. After 3 seconds, revert to showing "Endorse" (allowing re-endorsement). Clear the cache for this subject.
  - **Failure states to handle:**
    - Notary server unreachable: show "Notary offline" for 3s, then reset to "Endorse". Log the actual error to console.
    - Proof generation timeout (>60s): show "Timed out" for 3s. The offscreen-bundle.js already has a 60s timeout.
    - Backend submission 400/401/409: show "Failed" for 3s. 409 (duplicate proof_hash) means the user already endorsed with this exact proof -- show "Already endorsed" instead.
    - Backend submission 404: subject not found. Unlikely since the trust card loaded, but show "Not found" for 3s.
    - Network error: show "Offline" for 3s.
  - **Re-endorsement:** The button resets to "Endorse" after success or failure. Users can endorse the same repo multiple times (each proving session produces a unique attestation). Duplicate attestations are rejected by the unique proof_hash constraint.
  - **Debounce:** Disable the button during the entire flow (already implemented via `btn.disabled = true`). No additional debounce needed.

  **Files:** `extension/src/content-github.ts`, `extension/src/background.ts`

  **Testing:** Manual testing against the deployed stack. The key failure scenarios can be tested by temporarily misconfiguring `NOTARY_URL` or `API_BASE` in dev builds.

- [ ] **Unit 6: Integration verification and deployment**

  **Goal:** Verify the complete E2E flow works against deployed infrastructure and document the manual test plan.

  **Approach:**
  - **Config verification:** Confirm `extension/src/config.ts` values:
    - `API_BASE`: `https://commit-backend.fly.dev` (correct)
    - `NOTARY_URL`: `https://commit-verifier.fly.dev` (correct)
    - `PROXY_BASE`: `wss://notary.pse.dev/proxy` (retained, adequate)
  - **Notary server health check:** Verify `https://commit-verifier.fly.dev/healthcheck` responds. Verify the notary's public key is set in the backend via `NOTARY_PUBLIC_KEY` env var on Fly.io.
  - **Manual E2E test steps:**
    1. Load the extension in Chrome (dev build or Chrome Web Store).
    2. Navigate to a known repo (e.g., `github.com/nickel-org/nickel.rs`).
    3. Verify the trust card loads with a Commit Score.
    4. Click "Endorse". Verify button shows "Proving..." with shimmer.
    5. Wait for proof completion (~5-10s). Verify button shows "Endorsed".
    6. Verify the endorsement count updates in the trust card (optimistic +1).
    7. Reload the page. Verify the trust card shows the endorsement count from the API.
    8. Visit the trust card page (`commit-backend.fly.dev/trust/github/nickel-org/nickel.rs`). Verify the endorsement appears in the endorsement section with a "ZK Verified" badge.
    9. Click "Endorse" again. Verify a second endorsement is created (different proof_hash).
    10. Check error handling: disconnect network, verify "Offline" message. Restore network, verify endorsement works again.
  - **Automated test coverage:** Existing `cargo test` covers transcript validation and attestation signature verification. Add an integration test that creates an endorsement via `POST /endorsements` and then verifies it appears in `GET /trust-card` response for the same subject.
  - **Deploy sequence:** Backend first (new trust card response fields are additive, extension keeps working), then extension (reads new fields when available, gracefully ignores when absent).

  **Files:** No code changes. This unit is verification and test documentation.

## System-Wide Impact

- **Trust card API response shape changes:** `TrustCardResponse` gains `endorsement_count` and `recent_endorsements`. This is additive -- the existing extension will ignore unknown fields until updated. No breaking change.
- **Endorsement status semantics shift:** Direct submissions can now be `verified` (not just `pending_attestation`). Any code that filters on status should use `status != 'failed'` rather than `status == 'verified'` for counting. The existing `get_endorsement_count()` already uses this pattern.
- **Score not yet affected:** This plan does not activate Layer 2 scoring. The score remains Layer 1 only. Endorsement data is displayed alongside the score but does not influence it. Score v2 is a separate follow-up.
- **Extension update required:** The content script changes require a new Chrome Web Store submission. The backend changes are backward-compatible and can deploy independently.

## Risks & Dependencies

| Risk | Impact | Mitigation |
|------|--------|------------|
| Notary server cold start on Fly.io (auto-stop enabled) | First endorsement after idle period takes 10-20s extra | The "Proving..." state already handles variable timing. Consider raising `min_machines_running` to 1 if user reports of slowness increase. |
| PSE proxy (`notary.pse.dev/proxy`) goes down | All endorsements fail until proxy is restored | Deploy own proxy as a fast follow-up. The proxy is a simple WebSocket-to-TCP bridge. |
| TLSNotary WASM proof generation fails on certain browser versions | Endorsement flow breaks silently | The prove-worker already has error handling. Add browser version to error telemetry (console.log for now). |
| Chrome Web Store review delays | Extension update with UI changes takes 1-5 days to review | Backend changes deploy independently. Extension works with old API (missing fields default gracefully). |
| `get_endorsement_count()` adds a DB query per trust card request | Slight latency increase on trust card API | The query is a simple COUNT with an indexed column. Profile if trust card latency increases noticeably. |
| Duplicate endorsement attempts (user clicks fast) | Button disabled during flow, but race condition possible if page reloads | The unique `proof_hash` constraint in the DB prevents actual duplicates. A 409 response is handled in the UI. |
