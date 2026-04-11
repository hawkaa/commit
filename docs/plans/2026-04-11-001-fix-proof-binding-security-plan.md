---
title: "fix: Bind endorsement proofs to cryptographic attestation and subject"
type: fix
status: active
date: 2026-04-11
deepened: 2026-04-11
---

# fix: Bind endorsement proofs to cryptographic attestation and subject

## Overview

Two P0 security vulnerabilities in the endorsement flow allow proof forgery and subject spoofing. The proof_hash is computed from attacker-controlled JSON fields rather than the TLSNotary cryptographic attestation, and the claimed subject (repo identity) is never validated against the proof transcript. This means a proof generated for repo A can currently endorse repo B, and the proof_hash has no cryptographic binding to the actual MPC-TLS session.

## Problem Frame

The endorsement flow has two code paths, both vulnerable:

1. **`POST /endorsements`** (extension path) — Accepts a client-provided hex `proof_hash` with no verification. The extension truncates the attestation to 64 characters (`notarization.attestation.substring(0, 64)`) and sends this as the hash. No transcript, no subject binding.

2. **`POST /webhook/endorsement`** (notary server path) — Computes `proof_hash` by hashing `server_name`, `session.id`, and `results` strings — all attacker-controlled JSON fields. The `transcript` field is Optional and never read. Subject comes from `session.data` (a HashMap) with no cross-check against the transcript.

Neither path validates that the proof actually corresponds to the claimed subject.

## Requirements Trace

- R1. proof_hash must be derived from the TLSNotary cryptographic attestation, not from client-supplied or JSON-extracted string fields
- R2. The claimed subject_id must be validated against the HTTP request URL in the proof transcript — reject if they don't match
- R3. Transcript data must be required (not optional) for any endorsement with a proof
- R4. Existing test coverage for auth, server_name validation, and error paths must not regress
- R5. Extension must send full attestation and transcript data instead of truncated substring
- R6. Subject identifiers must be canonicalized to lowercase on ingestion to prevent case-mismatch failures
- R7. Exact proof replays must be prevented via unique constraint on (subject_id, proof_hash)

## Scope Boundaries

- Ed25519 request signing from extension keypair (Phase 2 — separate work)
- Notary server deployment to Fly.io (separate Phase 2 item, uses `verifier/fly.toml`)
- Server-side TLSNotary attestation signature verification (requires notary public key infrastructure — planned for when own notary is deployed)
- Network keyring, L2 attestation, Commit Score v2

### Deferred to Separate Tasks

- Notary server deployment: tracked as separate Phase 2 checklist item
- Full attestation signature verification: requires own notary server with known public key
- Attestation nonce-based replay prevention: the unique constraint prevents exact replays; nonce tracking for more sophisticated replay attacks is a separate hardening pass
- Email proof type transcript binding: email proofs are blocked by this fix (see Key Technical Decisions) and need a separate design for how a mail server transcript binds to a GitHub repo subject
- `ci_logs` proof type transcript binding: similar to email, needs its own URL path pattern (e.g., `/repos/{owner}/{repo}/actions/runs/{id}/logs`)

## Context & Research

### Relevant Code and Patterns

- `src/routes/endorsement.rs` — `POST /endorsements` handler, accepts `SubmitEndorsementRequest` with client-provided `proof_hash`
- `src/routes/webhook.rs` — `POST /webhook/endorsement` handler, `hash_verification_results()` function is the P0 #1 vulnerability, transcript validation absence is P0 #2
- `extension/src/prove-worker.ts` — WASM worker that runs `prover.notarize(commit)`, currently truncates attestation to 64 chars
- `extension/src/background.ts` — Routes proof result to `POST /endorsements`, constructs the subject claim client-side
- `extension/src/config.ts` — API_BASE and NOTARY_URL constants
- `tests/api.rs` — Existing webhook tests (auth, server_name, happy path) and endorsement tests (404, 400)
- `src/database.rs` — `find_subject` uses exact `WHERE kind = ? AND identifier = ?` (case-sensitive)
- Error handling pattern: `Result<Json<T>, StatusCode>` — flat status codes, no structured error bodies

### Institutional Learnings

- TLSNotary WASM integration documented in `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md` — confirms the proving pipeline: offscreen document -> Web Worker -> WASM. The attestation is generated at `prover.notarize(commit)` in prove-worker.ts.

## Key Technical Decisions

- **Attestation replaces proof_hash as input**: The `POST /endorsements` endpoint will accept raw attestation data instead of a client-computed hex hash. The backend computes proof_hash as `SHA-256(attestation_bytes)`. This eliminates the client's ability to supply an arbitrary hash.

- **Structural HTTP request line parsing with character validation**: The `validate_transcript_subject` function must parse the HTTP request line (`GET /path HTTP/1.1`), extract the URL path, split on `/` to get the owner and repo components, and compare each component individually against the subject_id. A loose `contains` check would be bypassable with crafted paths like `/repos/victim/repo-evil/repos/attacker/repo`. Comparison is case-insensitive (GitHub owner/repo names are case-insensitive). Each extracted component must be validated as ASCII matching `[a-zA-Z0-9_.\-]+` — reject if any component contains `%`, null bytes, or non-printable characters. This closes a percent-encoding bypass where `GET /repos/victim%2Frepo HTTP/1.1` could confuse the parser.

- **Transcript validation is unconditional on both endpoints**: `POST /endorsements` and `POST /webhook/endorsement` both enforce transcript-subject binding on every request, regardless of whether `attestation` is present. The webhook backward compatibility (accepting missing `attestation` and falling back to `hash_verification_results`) does NOT extend to transcript — transcript is always required and always validated. This prevents a compromised notary server from bypassing subject binding via the fallback hash path.

- **No server-side attestation signature verification yet**: Full verification of the notary's signature over the attestation requires the notary's public key. This is deferred to when the own notary server is deployed. The P0 fixes ensure data binding integrity, not full chain verification.

- **Email and ci_logs proof types blocked until binding is designed**: Only `git_history` has a clear transcript-subject binding (the `/repos/{owner}/{repo}` path). Email proofs involve a mail server transcript with no repo path. `ci_logs` would need its own path pattern. Both return 400 with a clear error until binding logic is implemented. This prevents an unvalidated bypass.

- **Identifier canonicalization to lowercase**: All subject identifiers are lowercased on ingestion (`upsert_subject`, `find_subject`). The transcript validation also lowercases before comparison. This prevents case-mismatch failures where the browser URL uses `Owner/Repo` but the database stores `owner/repo`.

- **Global unique constraint on proof_hash**: `UNIQUE(proof_hash)` rather than `UNIQUE(subject_id, proof_hash)`. A given attestation should produce exactly one endorsement globally — there is no valid reason for the same attestation to endorse multiple subjects (the transcript already binds it to one). This is strictly stronger than a per-subject constraint.

- **Transcript encoding**: The revealed portion of `transcript.sent` is the HTTP request line, which is valid ASCII. The extension should hex-encode the raw transcript bytes for transport (safe for any byte content), and the backend hex-decodes before parsing. This handles the case where TLSNotary WASM returns raw bytes rather than a UTF-8 string.

- **Reject incomplete transcript**: If the revealed transcript portion is too short to contain a complete `/repos/{owner}/{repo}` path (e.g., because someone modified the extension to reveal only 10 bytes), the validation rejects with 400. The parser must find a complete path, not a partial match.

## Open Questions

### Resolved During Planning

- **Which endpoint should the extension use?** `POST /endorsements`. The webhook stays for notary-server-to-backend communication.
- **Should we store the full attestation?** Yes — needed for later server-side verification and on-chain attestation reference.
- **What status for extension-submitted endorsements?** Keep `pending_attestation` until the notary server can confirm.
- **Case sensitivity?** Lowercase all identifiers on ingestion. Case-insensitive transcript comparison.
- **Email proof types?** Block with 400 until binding logic is designed. Track as a deferred task.
- **Replay prevention?** Add `UNIQUE(proof_hash)` constraint (global — one attestation, one endorsement globally). Ships with this fix.
- **Transcript parsing strategy?** Structural path extraction from HTTP request line. Not substring matching.
- **Transcript encoding over the wire?** Hex-encoded bytes from extension. Backend hex-decodes before parsing.

### Deferred to Implementation

- Exact column name and storage format for attestation data in SQLite — depends on how `create_endorsement` is currently structured
- Whether `prover.transcript().sent` from tlsn-js returns a string or Uint8Array — verify during Unit 3 implementation. If Uint8Array, hex-encode. If string, verify it's valid UTF-8 before hex-encoding.
- Optimal revealed byte ranges — currently 200 bytes sent, 500 bytes received. May need adjustment if 200 bytes doesn't always capture the full request URL for repos with long names

## Implementation Units

- [x] **Unit 0: Backend — Identifier canonicalization**

**Goal:** Normalize subject identifiers to lowercase on ingestion to prevent case-mismatch failures in transcript validation and subject lookup.

**Requirements:** R6

**Dependencies:** None

**Files:**
- Modify: `src/database.rs`
- Modify: `src/routes/endorsement.rs` (lowercase subject_id on input)
- Modify: `src/routes/webhook.rs` (lowercase subject_id from session.data)
- Modify: `src/routes/trust_card.rs` (lowercase identifier on lookup, if applicable)
- Test: `tests/api.rs`

**Approach:**
- In `database.rs`: lowercase the `identifier` field in `upsert_subject` before storing. Lowercase the lookup parameter in `find_subject`.
- In request handlers: lowercase `subject_id` / `subject_kind` at the entry point before any processing.
- This is a preparatory change that makes the subsequent transcript validation work correctly regardless of URL casing.

**Patterns to follow:**
- Existing `SubjectKind::parse()` pattern for input normalization

**Test scenarios:**
- Happy path: subject created with "Owner/Repo" is stored as "owner/repo"
- Happy path: `find_subject("github", "Owner/Repo")` finds a subject stored as "owner/repo"
- Edge case: mixed-case subject_id in endorsement POST matches existing lowercase subject
- Integration: trust-card lookup with mixed case still returns correct data

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- No existing tests break from the normalization

---

- [x] **Unit 1: Backend — Transcript-subject binding**

**Goal:** Reject endorsements where the claimed subject doesn't match the proof transcript. Fixes P0 #2.

**Requirements:** R2, R3

**Dependencies:** Unit 0

**Files:**
- Create: `src/validation.rs` (shared validation functions)
- Modify: `src/routes/endorsement.rs`
- Modify: `src/routes/webhook.rs`
- Modify: `src/main.rs` (add `mod validation`)
- Test: `tests/api.rs`

**Approach:**
- Create a `validate_transcript_subject` function that takes `transcript_sent: &str`, `proof_type: &ProofType`, and `subject_id: &str`, returns `Result<(), StatusCode>`
- Parse the HTTP request line from transcript_sent: extract the first line, split on spaces to get method and path, then extract the URL path
- For `git_history` proof type: split the path after `/repos/` on `/` to extract owner and repo. Validate each component matches `[a-zA-Z0-9_.\-]+` (reject `%`, null bytes, non-printable chars). Compare each component case-insensitively against the subject_id components. Reject if the path is incomplete, doesn't contain `/repos/`, or components don't match.
- For `email` and `ci_logs` proof types: return 400 with a message indicating these proof types are not yet supported for transcript binding
- In `endorsement.rs`: add `transcript_sent: String` and keep existing `proof_type: String` in `SubmitEndorsementRequest`. Parse `proof_type` via `ProofType::parse()` as already done, then call `validate_transcript_subject` with the parsed proof_type before creating endorsement.
- In `webhook.rs`: change `transcript: Option<RedactedTranscript>` to `transcript: RedactedTranscript`. Make `sent` required within `RedactedTranscript` (change from `Option<String>` to `String`). Call the shared validation function. Transcript validation is unconditional — called on every webhook request regardless of whether `attestation` is present.
- Return 400 with tracing::warn on mismatch

**Patterns to follow:**
- `SubjectKind::parse()` / `.as_str()` enum pattern in `src/models.rs`
- `server_name` validation block in `webhook.rs:102-114` for proof-type-specific validation
- `tracing::warn!` for security-relevant rejections

**Test scenarios:**
- Happy path: transcript_sent contains `GET /repos/owner/repo HTTP/1.1\r\n...` and subject_id is `owner/repo` → passes validation
- Happy path: case-insensitive match — transcript has `/repos/Owner/Repo`, subject_id is `owner/repo` → passes
- Edge case: transcript_sent with query parameters (`/repos/owner/repo?per_page=1 HTTP/1.1`) → still matches
- Edge case: transcript_sent with extra path segments after repo (`/repos/owner/repo/commits HTTP/1.1`) → still matches (owner/repo extracted correctly)
- Error path: transcript_sent contains `/repos/owner/repoA` but subject_id is `owner/repoB` → 400
- Error path: transcript_sent is empty string → 400
- Error path: transcript_sent doesn't contain a valid HTTP request line → 400
- Error path: transcript_sent too short to contain complete `/repos/{owner}/{repo}` path → 400
- Error path: path component contains percent-encoded characters (`/repos/victim%2Frepo`) → 400
- Error path: path component contains null bytes or non-printable characters → 400
- Error path: `POST /endorsements` request missing `transcript_sent` field → deserialization error (400)
- Error path: email proof_type submitted → 400 (unsupported proof type for transcript binding)
- Error path: ci_logs proof_type submitted → 400 (unsupported proof type for transcript binding)
- Integration: webhook with missing `transcript.sent` → 400
- Happy path: existing webhook happy-path test updated to include valid transcript → still passes

**Verification:**
- `cargo test` passes with no regressions in existing webhook/endorsement tests
- New tests prove that mismatched subject/transcript is rejected
- `cargo clippy -- -D warnings` clean

---

- [x] **Unit 2: Backend — Attestation-based proof_hash + replay guard**

**Goal:** Derive proof_hash from the TLSNotary attestation bytes instead of attacker-controlled fields. Add unique constraint to prevent exact replay. Fixes P0 #1 and R7.

**Requirements:** R1, R4, R7

**Dependencies:** Unit 1

**Files:**
- Modify: `src/routes/endorsement.rs`
- Modify: `src/routes/webhook.rs`
- Modify: `src/database.rs` (attestation storage + unique constraint)
- Test: `tests/api.rs`

**Approach:**
- In `endorsement.rs`: replace `proof_hash: String` with `attestation: String` in request struct. Hex-decode, reject if empty. Compute `SHA-256(attestation_bytes)` as proof_hash. Store attestation data alongside endorsement.
- In `webhook.rs`: add `attestation: Option<String>` to `VerifierWebhook`. When present, use it for proof_hash. When absent (backward compat with notary server), fall back to `hash_verification_results` but include `transcript.sent` content in the hash to strengthen binding. Add TODO to require attestation once notary server sends it.
- Add `attestation_data` column to endorsements table (BLOB, nullable for existing rows).
- Add `UNIQUE(proof_hash)` constraint to endorsements table (global — one attestation, one endorsement). Before adding the constraint, check for existing duplicate `proof_hash` values and deduplicate (keep the earliest by `created_at`, delete others). Handle `rusqlite::Error::SqliteFailure` with `libsqlite3_sys::SQLITE_CONSTRAINT_UNIQUE` extended code — pattern match and return 409 Conflict. The existing `map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)` pattern must be replaced with explicit error matching for `create_endorsement`.
- Remove or deprecate `hash_verification_results` once the notary server sends attestation data (deferred — add TODO).

**Patterns to follow:**
- `sha2::Sha256` usage already in `hash_verification_results`
- `hex::decode` pattern in `endorsement.rs:33`
- Database migration pattern: add nullable column, existing rows unaffected

**Test scenarios:**
- Happy path: valid attestation hex string → proof_hash is SHA-256 of decoded bytes, endorsement created
- Edge case: attestation is valid hex but empty (0 bytes) → 400
- Error path: attestation is not valid hex → 400
- Error path: `POST /endorsements` request with old `proof_hash` field instead of `attestation` → deserialization error
- Error path: duplicate endorsement (same attestation for any subject) → 409 Conflict (global unique on proof_hash)
- Happy path: webhook with attestation field → proof_hash derived from attestation
- Happy path: webhook without attestation field (backward compat) → falls back to hash_verification_results with transcript included
- Integration: proof_hash stored in database matches SHA-256 of the submitted attestation bytes
- Integration: second endorsement with different attestation for same subject → succeeds (different proof_hash)

**Verification:**
- `cargo test` passes — all existing tests updated for new request format
- New tests confirm proof_hash is deterministically derived from attestation
- Replay test confirms duplicate is rejected
- `cargo clippy -- -D warnings` clean

---

- [x] **Unit 3: Extension — Send full attestation and transcript**

**Goal:** Send the complete TLSNotary attestation and revealed transcript to the backend instead of truncated data. Fixes R5.

**Requirements:** R5

**Dependencies:** Unit 2

**Files:**
- Modify: `extension/src/prove-worker.ts`
- Modify: `extension/src/background.ts`

**Approach:**
- In `prove-worker.ts`: after `prover.notarize(commit)`, return the full `notarization.attestation` string and the revealed sent transcript. Remove `.substring(0, 64)`.
- Determine the type of `prover.transcript().sent` (string vs Uint8Array). If Uint8Array, hex-encode it. If string, hex-encode the UTF-8 bytes for consistency with the backend's expectation.
- Update the worker message type to include `attestation: string` and `transcriptSent: string` (hex-encoded) instead of just `proofHash`.
- In `background.ts`: update `handleStartEndorsement` to send `attestation` and `transcript_sent` in the POST body instead of `proof_hash`. Remove the `proof_hash` field.
- Update the `ProveResult` interface to carry `attestation` and `transcriptSent` instead of `proofHash`.

**Patterns to follow:**
- Message passing pattern: `self.postMessage()` in worker, `chrome.runtime.sendMessage()` in background
- Config constants from `extension/src/config.ts`

**Test scenarios:**
Test expectation: none — extension code has no unit test infrastructure (Playwright scaffolding only). The correctness of this unit is verified by the backend integration tests in Units 1-2 which validate the full request format. Manual testing via the extension on a GitHub repo page confirms the end-to-end flow.

**Verification:**
- Extension builds without errors (`npm run build` in extension/)
- Manual: click "Endorse" on a GitHub repo page → backend receives attestation + transcript_sent → endorsement created
- No `substring(0, 64)` remains in prove-worker.ts
- Log the `typeof prover.transcript().sent` to confirm encoding assumption

## System-Wide Impact

- **Interaction graph:** Content script → background.ts → offscreen/worker → background.ts → `POST /endorsements`. The webhook path (`POST /webhook/endorsement`) is independently affected. No other routes reference endorsement submission.
- **Error propagation:** Transcript validation and attestation decoding failures return 400 to the caller. Replay attempts return 409. No cascading effects — endorsement creation is all-or-nothing.
- **State lifecycle risks:** The unique constraint may cause `SQLITE_CONSTRAINT` errors that must be caught and mapped to 409, not 500.
- **API surface parity:** Both `POST /endorsements` and `POST /webhook/endorsement` get the same transcript-subject validation. The endorsement endpoint gets attestation-based hashing; the webhook gets it as opt-in (backward compat with notary server).
- **Integration coverage:** The case-normalization in Unit 0 affects all subject lookup paths (trust-card, endorsement, webhook). Ensure trust-card lookups still work after normalization.
- **Unchanged invariants:** `GET /endorsements`, `GET /trust-card`, `GET /trust/{kind}/{id}`, `GET /badge/{kind}/{id}.svg` — read paths are unchanged. Commit Score computation reads endorsement count only, not proof_hash, so scoring is unaffected.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| TLSNotary attestation format changes across versions | Pin to v0.1.0-alpha.12 (already pinned in extension + verifier). Document format assumptions. |
| 200 bytes of revealed transcript doesn't always capture full URL | GitHub API repo URL is ~50 bytes. 200 bytes is generous. Repos with extremely long names (>150 chars combined) could theoretically exceed the window — add a length check in validation that rejects incomplete paths. |
| Existing notary server callback format doesn't include attestation | Webhook handler accepts attestation as optional, falls back to strengthened hash. Require it once own notary server is deployed. |
| GitHub API URL format changes | The `/repos/{owner}/{repo}` path is a core API surface unlikely to change. Document the path pattern as a known maintenance point in the validation code. |
| Lowercase normalization changes existing subject lookups | Unit 0 is a preparatory commit. Run all existing tests after normalization to catch regressions before proceeding. |
| UNIQUE constraint fails for existing duplicate proof_hash rows | Unit 2: query for duplicates before adding constraint. Keep earliest by created_at, delete others. Run as part of the schema migration. |
| SQLITE constraint violation mapped to 500 instead of 409 | Unit 2: explicitly pattern-match `rusqlite::Error::SqliteFailure` with `SQLITE_CONSTRAINT_UNIQUE` extended code. Replace the blanket `map_err(\|_\| 500)` on `create_endorsement`. |

## Loose Threads

Items explicitly deferred from this fix that must be tracked:

1. **Email proof type transcript binding** — Currently blocked with 400. Needs a design for how a mail server transcript (e.g., `mail.google.com`) binds to a GitHub repo subject. Options: separate proof that email contains repo URL, or multi-step proof chain.
2. **`ci_logs` proof type transcript binding** — Similar to email. The URL pattern is `/repos/{owner}/{repo}/actions/runs/{id}/logs` which is structurally feasible but needs its own parser branch.
3. **Full attestation signature verification** — Requires own notary server with known public key. Until then, the backend stores the attestation but can't verify the notary's signature. The binding fixes (transcript + hash) are defense-in-depth layers that work without signature verification.
4. **Attestation nonce-based replay prevention** — The unique constraint prevents exact replays. A more sophisticated attack where the user generates a new MPC-TLS session for the same repo (producing a different attestation) is not prevented. This requires rate limiting by device key or similar — out of scope.
5. **Webhook `hash_verification_results` deprecation** — Falls back to the old function when attestation is absent. Should be removed once the notary server is configured to send attestation data.
6. **Score integrity without device binding** — `POST /endorsements` has no authentication. After the fix, anyone with a valid attestation+transcript can submit an endorsement. The unique constraint prevents exact replay, but generating new TLSNotary sessions for the same repo is cheap (~5s). Until Ed25519 request signing is added, Commit Score from `pending_attestation` endorsements should be weighted lower than `verified` ones. Consider rate limiting by IP or requiring the extension keypair signature in a follow-up.
7. **HTTP request pipelining in transcript** — If a TLSNotary session contains multiple HTTP requests (HTTP/1.1 keep-alive), the parser extracts the first request line. A carefully constructed session could start with a benign request and then fetch the target repo. The revealed byte range (offset 0, 200 bytes) mitigates this for now since the extension only sends one request per session, but the parser should validate that only one request line is present in the revealed portion.

## Sources & References

- CEO plan: `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md`
- Test plan: `~/.gstack/projects/commit/hakon-unknown-eng-review-test-plan-20260410-133500.md`
- TLSNotary WASM integration: `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md`
- TLSNotary JS library: tlsn-js v0.1.0-alpha.12
- Current extension config: `extension/src/config.ts`
