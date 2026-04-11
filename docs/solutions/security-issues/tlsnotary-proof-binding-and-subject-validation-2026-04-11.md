---
title: TLSNotary Proof Binding and Subject Validation
date: 2026-04-11
category: security-issues
module: endorsement
problem_type: security_issue
component: authentication
symptoms:
  - proof_hash computed from attacker-controlled JSON fields (server_name, session.id, results) with no binding to the cryptographic attestation
  - proof for repo A could endorse repo B because subject_id was never validated against the TLS transcript
  - transcript field was Optional and never read in the webhook handler
  - extension truncated attestation to 64 characters, losing the cryptographic material
root_cause: missing_validation
resolution_type: code_fix
severity: critical
tags:
  - tlsnotary
  - proof-binding
  - attestation
  - transcript-validation
  - endorsement
  - replay-prevention
  - subject-spoofing
---

# TLSNotary Proof Binding and Subject Validation

## Problem

Two P0 security vulnerabilities in the TLSNotary endorsement flow allowed proof forgery and subject spoofing. The `proof_hash` was computed from attacker-controlled JSON fields instead of the cryptographic attestation, and the claimed `subject_id` was never validated against the HTTP request URL in the proof transcript.

## Symptoms

- `hash_verification_results()` in `src/routes/webhook.rs` hashed `server_name`, `session.id`, and `results` strings, all of which came from the JSON request body. The hash had no cryptographic binding to the actual MPC-TLS session.
- The extension (`prove-worker.ts`) sent `notarization.attestation.substring(0, 64)` as the proof hash, truncating the full attestation to 64 characters.
- `subject_id` was read from `session.data` (a `HashMap<String, String>`) with no cross-check against the transcript. `POST /endorsements` accepted any client-provided `proof_hash` and `subject_id` combination.
- `transcript: Option<RedactedTranscript>` was declared but never read, meaning a proof for `api.github.com/repos/owner/repoA` could endorse `owner/repoB`.

## What Didn't Work

- **Server-side attestation signature verification** was considered but deferred. It requires running an own notary server with a known public key. The public `notary.pse.dev` does not expose its key for third-party verification. This is tracked as a loose thread in the plan.
- **transcript_sent independence from attestation bytes**: On `POST /endorsements`, the backend receives `transcript_sent` and `attestation` as separate fields. It cannot yet extract the transcript from the attestation blob. This is a known gap until the attestation format is parsed server-side.
- **Webhook fallback path**: When `attestation` is absent (backward compat with notary.pse.dev), the hash still derives from request fields. The fallback now includes `transcript.sent` to strengthen binding, but it's not cryptographically sound. Tracked for removal once the own notary is deployed.
- Initial implementation of path validation used `path.find("/repos/")` which accepted `/repos/` anywhere in the URL including query strings. Caught during code review and fixed to `starts_with`.

## Solution

### 1. Transcript-subject binding (`src/validation.rs`)

New `validate_transcript_subject()` function that structurally parses the HTTP request line from the transcript:

```rust
// Strip query string BEFORE checking path prefix
let path_no_query = path.split('?').next().unwrap_or(path);

// Require path to START with /repos/ (not just contain it)
if !path_no_query.starts_with("/repos/") {
    return Err(StatusCode::BAD_REQUEST);
}
let after_repos = &path_no_query["/repos/".len()..];

// Extract owner/repo, validate clean ASCII
let path_parts: Vec<&str> = after_repos.splitn(3, '/').collect();
if !is_valid_path_component(transcript_owner)
    || !is_valid_path_component(transcript_repo) {
    return Err(StatusCode::BAD_REQUEST);  // reject %, null bytes, non-printable
}

// Case-insensitive comparison
if !transcript_owner.eq_ignore_ascii_case(expected_owner)
    || !transcript_repo.eq_ignore_ascii_case(expected_repo) {
    return Err(StatusCode::BAD_REQUEST);
}
```

Applied unconditionally on both `POST /endorsements` and the webhook. `email` and `ci_logs` proof types return 400 until their transcript binding is designed.

### 2. Attestation-based proof_hash

`POST /endorsements` now accepts an `attestation` field (hex-encoded) instead of `proof_hash`. The backend computes `proof_hash = SHA-256(attestation_bytes)`:

```rust
let attestation_bytes = hex::decode(&req.attestation)?;
if attestation_bytes.is_empty() { return Err(StatusCode::BAD_REQUEST); }
let proof_hash = Sha256::digest(&attestation_bytes).to_vec();
```

The webhook accepts an optional `attestation` field. When present, it uses the attestation for the hash. When absent (backward compat), it falls back to a strengthened hash that includes `transcript.sent`.

### 3. Replay guard

`UNIQUE(proof_hash)` index on the endorsements table. `map_db_error` pattern-matches `SQLITE_CONSTRAINT_UNIQUE` to return 409 Conflict:

```rust
pub fn map_db_error(e: rusqlite::Error) -> StatusCode {
    if let rusqlite::Error::SqliteFailure(err, _) = &e
        && err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    {
        return StatusCode::CONFLICT;
    }
    StatusCode::INTERNAL_SERVER_ERROR
}
```

### 4. Supporting changes

- **Identifier canonicalization**: `find_subject` and `upsert_subject` lowercase all identifiers. Migration deduplicates existing case-insensitive collisions with cascade deletes on related rows.
- **Attestation size limit**: 1MB hex / 500KB decoded max on both endpoints to prevent memory exhaustion.
- **Extension**: Full attestation string + decoded `transcript.sent` sent to backend. `substring(0, 64)` truncation removed.

## Why This Works

**P0 #1 (proof_hash)**: The hash is now derived from the raw attestation bytes, which are the output of the TLSNotary MPC-TLS session. The hash is as trustworthy as the attestation itself. An attacker cannot control the hash without controlling the attestation, which requires breaking the MPC-TLS protocol.

**P0 #2 (subject binding)**: The HTTP request line in the transcript is cryptographically committed by the TLSNotary session. By structurally parsing the path and comparing against the claimed subject, the backend ensures the proof actually corresponds to the claimed repo. Query-string injection is prevented by stripping the query before checking the `/repos/` prefix. Percent-encoding and null bytes are rejected by the ASCII character allowlist.

**Replay guard**: `UNIQUE(proof_hash)` ensures each attestation produces exactly one endorsement globally. Since `proof_hash = SHA-256(attestation_bytes)`, the same attestation always hashes to the same value.

## Prevention

- Never derive a security-critical hash from fields in the same request body that carries the claim. Always hash the raw cryptographic artifact.
- Treat any `Optional` field that carries a security invariant as a required validation input, not an optional enrichment.
- Use `starts_with` for URL path matching, not `find` or `contains`. Strip query strings before path prefix checks. Validate each path component individually for encoding attacks (`[a-zA-Z0-9_.\-]+`).
- Add `UNIQUE` constraints on idempotency/replay keys at schema definition time. Handle the constraint violation explicitly (409 Conflict) rather than mapping all DB errors to 500.
- Canonicalize identifiers at the persistence boundary to prevent case-variant duplicates.
- Maintain a "loose threads" section in the plan for deferred security items so they survive between sessions. This project tracks 7 loose threads in `docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md`.

## Related Issues

- Plan: `docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md`
- Related doc: `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md` (TLSNotary WASM integration, different focus)
- PR: hawkaa/commit#5
