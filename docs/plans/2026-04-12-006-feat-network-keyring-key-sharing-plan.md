---
title: "feat: Network keyring and key sharing"
type: feat
status: active
date: 2026-04-12
---

# feat: Network keyring and key sharing

## Overview

Add the ability for users to share their Ed25519 public keys and build trust networks. The extension already generates a keypair on install — this plan adds a backend public key registry, a sharing flow (copy/paste or URL), and a `POST /network-query` endpoint that returns endorsement counts scoped to a user's network. This is the Phase 2 infrastructure that enables the Phase 3 "N in your network endorse this" display.

## Problem Frame

The Commit extension generates an Ed25519 keypair on install (background.ts), but the public key is never shared, registered, or used. Endorsements are anonymous — there is no way to answer "did anyone I know endorse this?" The design doc specifies a `NetworkMembership` model where users share keys out-of-band and the extension holds a local keyring. The CEO plan calls for "network keyring + key sharing" in Phase 2 and "N in your network endorse this" in Phase 3.

The design doc describes an ideal OPRF-based private set intersection query where the server never learns the network graph. For the current scale (< 50 users), the MVP fallback is acceptable: extension sends hashed public keys, server matches against endorser key hashes. Privacy-preserving queries are deferred.

## Requirements Trace

- R1. Extension keypair's public key must be registered with the backend on first use
- R2. Users must be able to share their public key via a copyable hex string or shareable URL
- R3. Users must be able to add another user's public key to their local keyring
- R4. Endorsements must be associated with the submitting public key so network queries can filter by endorser
- R5. `POST /network-query` must accept a list of key hashes and a subject, returning endorsement count from matching endorsers
- R6. MVP uses hashed public keys (SHA-256) — not raw keys — to limit server-side linkability
- R7. Existing endorsement flow must not break for users who haven't registered a key yet

## Scope Boundaries

- OPRF / private set intersection for network queries — deferred to when user count justifies the complexity
- "N in your network endorse this" UI display — Phase 3, separate plan (consumes the API this plan builds)
- Ed25519 request signing for endorsement authentication — separate follow-up
- Key rotation or revocation — not needed at current scale, add later
- Firefox extension support

### Deferred to Separate Tasks

- Phase 3 "N in your network endorse this" display: consumes `POST /network-query` from this plan
- Privacy-preserving network queries (OPRF/PSI): replaces the MVP hashed-key approach at scale
- Ed25519 endorsement signing: uses the same keypair but is a separate auth concern

## Context & Research

### Relevant Code and Patterns

- `extension/src/background.ts:21-41` — Ed25519 keypair generation on install, stored in `chrome.storage.local` as `keypair: { publicKey: number[], privateKey: number[], createdAt: string }`
- `src/services/db.rs` — SQLite with `CREATE TABLE IF NOT EXISTS` migration pattern, `map_db_error()` for constraint violations
- `src/routes/endorsement.rs` — `POST /endorsements` handler, `SubmitEndorsementRequest` struct
- `src/routes/webhook.rs` — `POST /webhook/endorsement`, creates endorsements with `verified` status
- `src/main.rs` — axum Router, AppState with `db`, `github`, `notary_public_key`
- `extension/src/config.ts` — `API_BASE` constant for backend URL
- `extension/src/content-github.ts` — Trust card injection, endorse button handler

### Database Schema Relevant to This Plan

- `endorsements` table: has `id`, `subject_id`, `proof_hash`, `status` — currently no `endorser_key_hash` column
- `subjects` table: keyed by `(kind, identifier)`, has `id` UUID
- No `peers` or `network` tables exist yet

## Key Technical Decisions

- **Public key registration is implicit on first endorsement, not explicit.** Adding a separate "register key" step creates friction. Instead, the extension sends its public key hash with every endorsement. The backend stores the key hash in the endorsements table. A peer shows up in the network when they've made at least one endorsement. This avoids a separate `peers` table — the endorser graph is derived from endorsements.

- **Key hash stored per endorsement, not in a separate table.** Each endorsement gets an `endorser_key_hash` column (SHA-256 of the raw Ed25519 public key, hex-encoded). This is simpler than a normalized peers table and avoids foreign key complexity. The network query joins endorsements by key hash. At current scale (< 1000 endorsements), this is efficient.

- **Extension sends key hash, not raw public key.** The backend never sees the actual public key — only the SHA-256 hash. This limits linkability: the server can tell "same device endorsed X and Y" but cannot derive the public key to correlate with other systems. The hash is deterministic so the same device always produces the same hash.

- **Network query is a simple POST, not a blinded proof.** `POST /network-query` accepts `{ subject_kind, subject_id, key_hashes: [hex_string] }` and returns `{ count: u32, total_endorsements: u32 }`. The server learns which key hashes the caller is interested in — this is the MVP privacy tradeoff acceptable at < 50 users.

- **Keyring stored in chrome.storage.local.** The keyring is an array of `{ publicKeyHex: string, label: string, addedAt: string }` under the key `"keyring"`. Labels are user-provided nicknames ("Alice", "Work laptop"). The extension hashes each key before sending to the backend.

- **Sharing via hex string.** The user's public key is displayed as a hex string in the extension popup. Share by copy/paste, messaging, or QR code (QR is a follow-up). Adding a key: paste the hex string into the extension popup's "Add to network" field. Simple, no server-mediated key exchange needed.

## Open Questions

### Resolved During Planning

- **Should we create a `peers` table?** No — store `endorser_key_hash` per endorsement. Simpler, avoids schema complexity. Network is derived from endorsement data.
- **Should we register the key proactively?** No — implicit on first endorsement. Avoids a registration step that adds friction.
- **What about users who endorsed before this change?** Existing endorsements will have NULL `endorser_key_hash`. They won't appear in network queries. This is acceptable — legacy anonymous endorsements remain but aren't network-attributed.

### Deferred to Implementation

- Exact popup UI layout for key display and keyring management
- Whether to add a "Share via link" that encodes the key in a URL fragment (e.g., `commit.dev/join#key=<hex>`)

## Implementation Units

- [ ] **Unit 1: Backend — Add endorser_key_hash to endorsements**

**Goal:** Store the endorser's hashed public key with each endorsement so network queries can filter by endorser.

**Requirements:** R4, R7

**Dependencies:** None

**Files:**
- Modify: `src/services/db.rs` (migration + create_endorsement signature)
- Modify: `src/routes/endorsement.rs` (accept `endorser_key_hash` field)
- Modify: `src/routes/webhook.rs` (accept optional `endorser_key_hash`)
- Test: `tests/api.rs`

**Approach:**
- Add migration in `db.rs`: `ALTER TABLE endorsements ADD COLUMN endorser_key_hash TEXT;` (nullable — existing rows get NULL)
- Add index: `CREATE INDEX IF NOT EXISTS idx_endorsements_key_hash ON endorsements(endorser_key_hash);`
- Extend `create_endorsement()` to accept `endorser_key_hash: Option<&str>` parameter
- In `endorsement.rs`: add `endorser_key_hash: Option<String>` to `SubmitEndorsementRequest`. Pass through to `create_endorsement()`. The field is optional — clients that don't send it (old extension versions) still work.
- In `webhook.rs`: accept optional `endorser_key_hash` in webhook payload, pass through. Webhooks from the notary server won't have a key hash (server-to-server), so this stays `None`.
- Validate format when present: must be 64-character hex string (SHA-256 output). Return 400 if malformed.

**Patterns to follow:**
- `attestation_data` migration pattern in `db.rs:122-148` (nullable column addition with version check)
- `SubmitEndorsementRequest` struct pattern in `endorsement.rs`

**Test scenarios:**
- Happy path: endorsement with `endorser_key_hash` field stores the hash in DB
- Happy path: endorsement without `endorser_key_hash` (backward compat) succeeds, NULL in DB
- Error: `endorser_key_hash` is not 64-char hex → 400
- Regression: all existing endorsement tests pass (field is optional)

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- Existing endorsement flow works without the new field

---

- [ ] **Unit 2: Backend — Network query endpoint**

**Goal:** Add `POST /network-query` that accepts a list of endorser key hashes and a subject, returning how many endorsements came from matching endorsers.

**Requirements:** R5, R6

**Dependencies:** Unit 1

**Files:**
- Create: `src/routes/network.rs`
- Modify: `src/main.rs` (add route)
- Modify: `src/routes/mod.rs` (add `pub mod network`)
- Modify: `src/services/db.rs` (add `count_network_endorsements` query)
- Test: `tests/api.rs`

**Approach:**
- New route: `POST /network-query` accepting `NetworkQueryRequest { kind: String, id: String, key_hashes: Vec<String> }`. Returns `NetworkQueryResponse { network_endorsement_count: u32, total_endorsement_count: u32 }`.
- In `db.rs`: add `count_network_endorsements(subject_id: &Uuid, key_hashes: &[String]) -> Result<u32>`. Query: build a `WHERE endorser_key_hash IN (?, ?, ...)` clause dynamically. Count non-failed endorsements matching both the subject and any of the provided key hashes.
- Validate: `key_hashes` must be non-empty, each must be 64-char hex. Cap at 200 key hashes per request to prevent abuse. Return 400 if empty or too many.
- Return `total_endorsement_count` alongside `network_endorsement_count` so the caller can display "3 of 7 endorsements are from your network."
- The endpoint is unauthenticated (consistent with all other Commit endpoints).

**Patterns to follow:**
- `get_trust_card` route pattern for request parsing and subject lookup
- `get_endorsement_count` query pattern in `db.rs`
- Route registration in `main.rs`

**Test scenarios:**
- Happy path: subject has 3 endorsements, 2 match provided key hashes → `{ network: 2, total: 3 }`
- Happy path: no matching key hashes → `{ network: 0, total: 3 }`
- Happy path: no endorsements for subject → `{ network: 0, total: 0 }`
- Edge case: endorsements with NULL `endorser_key_hash` don't match any key hash
- Error: empty `key_hashes` array → 400
- Error: key_hashes exceeding 200 → 400
- Error: invalid hex in key_hashes → 400
- Error: unknown subject kind → 400

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- Endpoint returns correct counts for test data

---

- [ ] **Unit 3: Extension — Send endorser key hash with endorsements**

**Goal:** The extension sends its public key hash alongside every endorsement submission, linking the endorsement to the device's identity.

**Requirements:** R1, R4

**Dependencies:** Unit 1

**Files:**
- Modify: `extension/src/background.ts` (add key hash to endorsement submission)

**Approach:**
- In `handleStartEndorsement()`: before POSTing to `/endorsements`, read the keypair from `chrome.storage.local`. Compute SHA-256 of the raw public key bytes. Hex-encode the hash. Include as `endorser_key_hash` field in the POST body.
- Use `crypto.subtle.digest("SHA-256", new Uint8Array(keypair.publicKey))` to compute the hash.
- If the keypair doesn't exist (shouldn't happen — generated on install), proceed without the field (backward compat).

**Patterns to follow:**
- Existing `chrome.storage.local.get("keypair")` pattern in background.ts
- `crypto.subtle` usage already present for key generation

**Test scenarios:**
Test expectation: none — extension has no unit test infrastructure. Verified by backend integration tests in Unit 1 and manual testing.

**Verification:**
- Extension builds without errors (`npm run build` in extension/)
- Manual: endorse a repo, check backend logs/DB to confirm `endorser_key_hash` is stored
- The hash is deterministic: endorsing two different repos produces the same key hash

---

- [ ] **Unit 4: Extension — Keyring management and key sharing**

**Goal:** Users can view their public key, copy it to share, and add other users' keys to their local keyring.

**Requirements:** R2, R3

**Dependencies:** Unit 3

**Files:**
- Modify: `extension/src/popup.html` (add keyring UI section)
- Modify: `extension/src/popup.ts` (or create if not existing — keyring management logic)
- Modify: `extension/src/background.ts` (add message handlers for keyring operations)

**Approach:**
- **Popup UI additions:**
  - "Your key" section: display the hex-encoded public key (first 8 + last 8 chars visible, full key on click/expand). "Copy" button.
  - "Your network" section: list of keyring entries with label and truncated key. "Add key" input field + button. "Remove" button per entry.
- **Keyring storage:** `chrome.storage.local` key `"keyring"` containing `Array<{ publicKeyHex: string, label: string, addedAt: string }>`.
- **Message protocol:** Popup sends messages to background for add/remove operations. Background validates the hex format (64-char hex for raw Ed25519 public key = 32 bytes) and persists to storage.
- **Public key display:** Read `keypair.publicKey` from storage, hex-encode the raw bytes (not the hash — the actual key, so recipients can independently hash it). The 32-byte Ed25519 public key encodes as 64 hex characters.

**Patterns to follow:**
- Existing `chrome.storage.local` patterns in background.ts
- `chrome.runtime.sendMessage` / `onMessage` pattern for popup↔background communication

**Test scenarios:**
Test expectation: none — extension popup has no test infrastructure. Manual verification.

**Verification:**
- Extension builds without errors
- Manual: open popup, see own public key, copy it, paste into another instance's "Add key" field, confirm it appears in the keyring list
- Keyring persists across browser restarts

---

- [ ] **Unit 5: Extension — Network query integration**

**Goal:** The extension queries the backend for network endorsement counts and makes the data available for the Phase 3 "N in your network" display.

**Requirements:** R5

**Dependencies:** Units 2, 4

**Files:**
- Modify: `extension/src/content-github.ts` (add network query call)
- Modify: `extension/src/background.ts` (add network query helper)

**Approach:**
- In `background.ts`: add `queryNetwork(subjectKind: string, subjectId: string): Promise<{ network: number, total: number }>` function. Reads the keyring from storage, computes SHA-256 of each key, sends `POST /network-query` with the hashes.
- In `content-github.ts`: after fetching the trust card data, call `queryNetwork()` via message to background. Store the result alongside the trust card cache entry.
- For now, include the network data in the cached trust card data but do NOT render it visually — the "N in your network" display is Phase 3. This unit wires the data pipeline so Phase 3 only needs UI work.
- If the keyring is empty (no network contacts), skip the query entirely.

**Patterns to follow:**
- `fetch(API_BASE + "/trust-card?...")` pattern in content-github.ts
- `chrome.runtime.sendMessage` for content script → background communication

**Test scenarios:**
Test expectation: none — manual verification only.

**Verification:**
- Extension builds without errors
- Manual: add a contact's key to keyring, visit a repo they endorsed, check console/network tab for the `/network-query` request and response
- Empty keyring: no network query is made

## System-Wide Impact

- **Endorsement API contract change:** `POST /endorsements` gains an optional `endorser_key_hash` field. Existing clients that omit it continue to work (backward compat). New extension versions include it.
- **New route added:** `POST /network-query` is a new endpoint. No existing routes are modified in behavior.
- **Database schema:** `endorsements` table gains a nullable `endorser_key_hash TEXT` column. Existing rows retain NULL. No data migration needed beyond the ALTER TABLE.
- **Privacy model:** The server learns which key hashes a user asks about in network queries. At < 50 users this is acceptable. The design doc's OPRF approach replaces this at scale.
- **Extension storage:** Two new keys in `chrome.storage.local`: `keyring` (array of contacts) and network query cache entries. Storage usage is minimal (< 10KB for 200 contacts).
- **Unchanged invariants:** Trust card API, badge API, trust page SSR, endorsement verification — all unchanged. Score computation is not affected (Score v2 plan handles `network_density`).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Key hash linkability: server can correlate endorsements from same device | Acceptable at MVP scale (< 50 users who trust the system). OPRF/PSI replaces this at scale. |
| Lost keypair means lost network identity | No recovery mechanism yet. User generates a new key, asks contacts to re-add them. Key backup/export is a follow-up. |
| Extension popup may not exist yet | Check during implementation. If no popup.html exists, create a minimal one. The popup is a lightweight UI — not a complex SPA. |
| IN clause performance with 200 key hashes | SQLite handles IN with hundreds of values efficiently. The endorsements table is small (< 10K rows in Phase 2). Add EXPLAIN QUERY PLAN check during implementation if concerned. |
| Old extension versions don't send key hash | `endorser_key_hash` is optional. Old endorsements are invisible to network queries but otherwise work fine. |

## Sources & References

- Design doc: `~/.gstack/projects/commit/hakon-unknown-design-20260410-131531.md` (NetworkMembership section)
- CEO plan: `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md` (Phase 2: network keyring)
- Extension keypair: `extension/src/background.ts:21-41`
- Existing endorsement route: `src/routes/endorsement.rs`
- Database patterns: `src/services/db.rs`
