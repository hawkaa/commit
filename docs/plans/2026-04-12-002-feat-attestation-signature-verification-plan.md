---
title: "feat: Verify attestation signatures against notary public key"
type: feat
status: active
date: 2026-04-12
origin: docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md
---

# feat: Verify attestation signatures against notary public key

## Overview

Add server-side ECDSA-secp256k1 signature verification for TLSNotary attestations. When the notary public key is configured, both endorsement endpoints verify that the attestation was signed by the trusted notary before accepting it. This closes the "full attestation signature verification" loose thread from the P0 proof-binding fix and makes attestation required on the webhook (removing the `hash_verification_results_with_transcript` fallback), now that the own notary server is deployed.

## Problem Frame

The backend stores TLSNotary attestations and computes `proof_hash = SHA-256(attestation_bytes)`, but never verifies the notary's cryptographic signature over the attestation. This means any valid-looking byte sequence is accepted — there is no proof that our trusted notary actually signed it. The P0 fix (see origin) explicitly deferred signature verification until the own notary server was deployed with a known public key. That server is now live at `commit-verifier.fly.dev`, and `NOTARY_PUBLIC_KEY` is loaded into `AppState` but unused.

Separately, the webhook endpoint still accepts requests without attestation data (falling back to `hash_verification_results_with_transcript`), even though the own notary now sends attestation. This fallback path has weaker cryptographic binding and should be removed.

## Requirements Trace

- R1. Verify ECDSA-secp256k1 signature over the BCS-serialized attestation header using the trusted notary public key
- R2. Reject endorsements whose attestation was not signed by the trusted notary (401)
- R3. Parse notary public key from SPKI PEM at startup; fail fast on malformed PEM
- R4. Graceful degradation: when `NOTARY_PUBLIC_KEY` is not set, skip verification (dev/test environments, existing behavior preserved)
- R5. Existing tests must not regress
- R6. Make `attestation` required on the webhook endpoint (remove `Option`)
- R7. Remove `hash_verification_results_with_transcript` fallback and its backward-compat test

## Scope Boundaries

- Merkle tree recomputation (verifying body field integrity against `header.root`) — the signature over the header implicitly commits to the Merkle root, but independent body verification is a separate hardening pass
- Extension changes — the extension sends raw `Attestation` hex; no changes needed
- Notary key rotation automation
- Email/ci_logs proof type transcript binding

### Deferred to Separate Tasks

- Merkle body verification: future hardening — verify that the `Body` fields hash to the Merkle root in `Header.root`
- Key rotation: operational procedure to rotate the notary signing key and update `NOTARY_PUBLIC_KEY` on the backend

## Context & Research

### Relevant Code and Patterns

- `src/lib.rs:16` — `AppState.notary_public_key: Option<String>` (PEM, currently unused)
- `src/main.rs:23-34` — loads `NOTARY_PUBLIC_KEY` from env, logs fingerprint
- `src/routes/endorsement.rs:44-49` — hex-decodes attestation, computes SHA-256 proof_hash
- `src/routes/webhook.rs:22-23` — `attestation: Option<String>` with TODO to require it
- `src/routes/webhook.rs:126-140` — attestation/fallback branching
- `src/routes/webhook.rs:208-220` — `hash_verification_results_with_transcript` (to be removed)
- `src/validation.rs` — existing `validate_transcript_subject` function; new verification function goes here
- `tests/api.rs:18` — `notary_public_key: None` in `test_app()`
- `Cargo.toml` — existing deps: `sha2 0.11`, `hex 0.4.3`; no secp256k1/ECDSA crate yet

### TLSNotary Attestation Format (v0.1.0-alpha.12)

The attestation blob from `prover.notarize()` is BCS-serialized (Binary Canonical Serialization, not bincode):

```
Attestation {
    signature: Signature { alg: SignatureAlgId, data: Vec<u8> },
    header: Header { id: [u8; 16], version: Version, root: TypedHash },
    body: Body { verifying_key, connection_info, transcript_commitments, ... },
}
```

**What gets signed:** `bcs::to_bytes(&header)` — the BCS-serialized Header only. The Header contains a Merkle root that commits to all Body fields. The `k256::ecdsa::VerifyingKey::verify()` method internally SHA-256-hashes the message before ECDSA verification.

**Key format:** The notary's public key is SPKI PEM (from `GET /info`). The embedded key in `Body.verifying_key` is SEC1 compressed (33 bytes). Both represent the same secp256k1 point.

### External References

- `tlsn-core` crate (tag `v0.1.0-alpha.12`): `crates/core/src/attestation.rs`, `crates/core/src/signing.rs`
- `k256` crate: `ecdsa::VerifyingKey::from_public_key_pem()`, `signature::Verifier` trait
- BCS serialization: `bcs::to_bytes()` / `bcs::from_bytes()` — deterministic, field-order-dependent

## Key Technical Decisions

- **Use `tlsn-core` as a git dependency** pinned to `v0.1.0-alpha.12`. This gives correct `Attestation`, `Header`, and `Signature` types for BCS deserialization without fragile struct replication. The public API (`Presentation::verify()`) is not usable because the extension sends raw `Attestation`, not `Presentation`, and `AttestationProof::verify()` is `pub(crate)`. Manual verification using public struct fields is the correct approach.

- **Verify with the trusted key directly**, not the embedded body key. Since we pin the notary's public key via `NOTARY_PUBLIC_KEY`, we verify the signature against that known key. This is strictly stronger than verifying against the self-certifying embedded key — it proves *our* notary signed the attestation, not just *any* notary.

- **Store parsed `VerifyingKey` in `AppState`**, not the raw PEM string. Parse once at startup, fail fast on invalid PEM. This avoids repeated PEM parsing on every request and catches configuration errors immediately.

- **Return 401 Unauthorized** when attestation verification fails. This distinguishes "your attestation isn't signed by our notary" (auth failure) from "your request is malformed" (400).

- **Remove webhook fallback in this plan.** The own notary server is deployed and sends attestation data. The `hash_verification_results_with_transcript` function and its backward-compat test are removed. The webhook `attestation` field becomes required (not `Option`).

## Open Questions

### Resolved During Planning

- **Which crate for secp256k1 verification?** `k256` — the same crate `tlsn-core` uses internally. Ensures format compatibility for signature and key bytes.
- **Can we use `Presentation::verify()`?** No. The extension sends raw `Attestation` hex, not `Presentation`. And `AttestationProof::verify()` is `pub(crate)`. Manual verification using public fields works.
- **Should we verify the embedded body key matches the trusted key?** Not required for security — the signature check with the trusted key is strictly stronger. But logging a warning if they differ is useful for debugging.
- **What about Merkle body integrity?** Deferred. The signature over the header commits to the Merkle root of the body. Body tampering after signing would require breaking SHA-256 preimage resistance. Defense-in-depth body verification is a separate task.

### Deferred to Implementation

- Whether `tlsn-core` v0.1.0-alpha.12 compiles cleanly with edition 2024 and existing deps. If dependency conflicts arise, the fallback is minimal struct replication with `k256` + `bcs`.
- Exact test fixture approach: capturing a real attestation from the live system vs constructing valid BCS bytes programmatically. `Body` has private fields, so constructing valid `Attestation` structs from outside `tlsn-core` requires either a builder API or manual BCS byte construction.

## Implementation Units

- [ ] **Unit 1: Add dependencies and parse notary key at startup**

**Goal:** Add cryptographic dependencies, change `AppState.notary_public_key` from raw PEM string to a parsed `k256::ecdsa::VerifyingKey`, and fail fast on malformed PEM at startup.

**Requirements:** R3, R4, R5

**Dependencies:** None

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Modify: `src/bin/mcp.rs` (also constructs `AppState` — needs same type change)
- Modify: `Dockerfile` (add `git` to `apk add` — required for git dependencies)
- Test: `tests/api.rs`

**Approach:**
- Add `tlsn-core = { git = "https://github.com/tlsnotary/tlsn", tag = "v0.1.0-alpha.12" }`, `bcs` crate, and `k256 = { version = "0.13", features = ["ecdsa", "pem"] }` to `[dependencies]`
- In `Dockerfile`: add `git` to the `apk add` line (cargo needs git to fetch the `tlsn-core` git dependency)
- In `lib.rs`: change `notary_public_key: Option<String>` to `Option<k256::ecdsa::VerifyingKey>`
- In `main.rs`: parse PEM using `k256::pkcs8::DecodePublicKey` trait's `from_public_key_pem()`. On parse failure, panic with a clear message (misconfigured key is a fatal startup error). Keep the fingerprint logging (derive from the parsed key's SEC1 encoding).
- In `tests/api.rs`: `test_app()` keeps `notary_public_key: None` — all existing tests pass unchanged

**Patterns to follow:**
- `GITHUB_TOKEN` optional env var pattern in `main.rs`
- Existing `AppState` field pattern

**Test scenarios:**
- Happy path: all existing tests pass with `notary_public_key: None` — no regressions from the type change
- Happy path: backend starts without `NOTARY_PUBLIC_KEY` → `None`, warn logged (existing behavior preserved)
- Error path: backend panics with clear message if `NOTARY_PUBLIC_KEY` is set but contains invalid PEM

**Verification:**
- `cargo test` passes with no regressions
- `cargo clippy -- -D warnings` clean
- `cargo build` succeeds with the new dependencies

---

- [ ] **Unit 2: Create attestation signature verification function**

**Goal:** Add a function to `validation.rs` that deserializes a TLSNotary attestation from BCS bytes and verifies its signature against the trusted notary public key.

**Requirements:** R1, R2

**Dependencies:** Unit 1

**Files:**
- Modify: `src/validation.rs`

**Approach:**
- Add `verify_attestation_signature(attestation_bytes: &[u8], trusted_key: &k256::ecdsa::VerifyingKey) -> Result<(), StatusCode>`
- Deserialize: `bcs::from_bytes::<tlsn_core::attestation::Attestation>(attestation_bytes)` — return 400 on failure
- Extract header: `bcs::to_bytes(&attestation.header)` — this is the signed message
- Extract signature: `k256::ecdsa::Signature::from_slice(&attestation.signature.data)` — return 400 on malformed signature
- Verify: `trusted_key.verify(&header_bcs, &sig)` using the `signature::Verifier` trait — return 401 on failure
- Log `tracing::warn!` on verification failure with the signature algorithm and failure reason
- Optionally log at debug level if `attestation.body.verifying_key().data` doesn't match the trusted key's SEC1 bytes (for diagnostic purposes, not security-critical)

**Patterns to follow:**
- `validate_transcript_subject` function structure in the same file
- `tracing::warn!` for security-relevant rejections (matches existing pattern)

**Test scenarios:**
- Error path: random garbage bytes → 400 (BCS deserialization fails)
- Error path: valid BCS for a different struct type (not Attestation) → 400
- Error path: truncated attestation bytes → 400
- Happy path + Error path with generated test keypair: if a test fixture can be constructed, verify it passes with the matching key and fails with a different key → 401. Exact construction approach deferred to implementation.

**Verification:**
- Unit tests in `validation.rs` pass
- `cargo clippy -- -D warnings` clean

---

- [ ] **Unit 3: Integrate verification into POST /endorsements**

**Goal:** Call attestation signature verification in the endorsement endpoint, with graceful skip when no notary key is configured.

**Requirements:** R1, R2, R4

**Dependencies:** Unit 2

**Files:**
- Modify: `src/routes/endorsement.rs`
- Test: `tests/api.rs`

**Approach:**
- After hex-decoding attestation bytes (line 44-48) and before computing proof_hash, check `state.notary_public_key`:
  - `Some(key)` → call `verify_attestation_signature(&attestation_bytes, key)?`
  - `None` → skip verification (log at debug level)
- The verification runs before proof_hash computation — reject early before any DB work

**Patterns to follow:**
- `validate_transcript_subject` call pattern (line 37) — same position in the request lifecycle

**Test scenarios:**
- Happy path: existing endorsement tests pass unchanged (notary_public_key is None in test_app, so verification is skipped)
- Happy path: with a configured key, a valid signed attestation → endorsement created
- Error path: with a configured key, an unsigned/mis-signed attestation → 401
- Error path: with a configured key, garbage attestation bytes → 400

**Verification:**
- `cargo test` passes — all existing endorsement tests still work
- New tests confirm verification is enforced when key is configured

---

- [ ] **Unit 4: Require attestation on webhook, remove fallback, integrate verification**

**Goal:** Make `attestation` required on the webhook endpoint, remove the `hash_verification_results_with_transcript` fallback, and add signature verification. This tightens the webhook path now that the own notary server sends attestation data.

**Requirements:** R1, R2, R4, R6, R7

**Dependencies:** Unit 2

**Files:**
- Modify: `src/routes/webhook.rs`
- Test: `tests/api.rs`

**Approach:**
- In `VerifierWebhook`: change `attestation: Option<String>` to `attestation: String`. Remove the TODO comment (line 22).
- Replace the attestation/fallback branching (lines 126-140) with direct attestation handling: hex-decode, verify signature (when key configured), compute proof_hash. Remove the `else` branch.
- Delete `hash_verification_results_with_transcript` function (lines 208-220) and its TODO comment.
- Add signature verification call after hex-decode, same pattern as Unit 3.
- Update the `webhook_payload()` test helper (tests/api.rs:196) to include an `attestation` field (e.g., `"attestation": "deadbeef01020304"`). Three tests use this helper (`webhook_rejects_without_secret`, `webhook_rejects_bad_auth`, `webhook_happy_path_creates_endorsement`) — without this change they will fail at deserialization before reaching the logic they test.
- Update `webhook_backward_compat_no_attestation` test — this test now expects 422 (missing required `attestation` field in JSON). Rename to `webhook_missing_attestation_returns_422`.
- Remove any other tests that specifically test the fallback hash path.

**Patterns to follow:**
- Unit 3 verification integration pattern

**Test scenarios:**
- Happy path: webhook with attestation field → still works (existing test `webhook_happy_path_with_attestation_uses_attestation_hash`)
- Error path: webhook without `attestation` field → deserialization error (422 or 400)
- Error path: webhook with invalid attestation hex → 400
- Error path: webhook with attestation but wrong notary key → 401 (when key is configured)
- Happy path: all other existing webhook tests pass (they already include attestation or are unaffected)
- Integration: duplicate attestation still returns 409

**Verification:**
- `cargo test` passes with updated test expectations
- `cargo clippy -- -D warnings` clean
- No references to `hash_verification_results` remain in the codebase

## System-Wide Impact

- **Interaction graph:** Verification is added to both endorsement ingestion paths (`POST /endorsements` and `POST /webhook/endorsement`). No other routes are affected. Read paths (`GET /endorsements`, `GET /trust-card`, etc.) are unchanged.
- **Error propagation:** Verification failure returns 401 to the caller. 400 for malformed attestation bytes. No cascading effects — endorsement creation is all-or-nothing.
- **State lifecycle risks:** The `k256::ecdsa::VerifyingKey` in `AppState` is immutable after startup. No runtime state changes.
- **API surface parity:** Both endorsement endpoints get the same verification. The webhook loses backward compatibility for requests without attestation data — this is intentional since the own notary server sends it.
- **Unchanged invariants:** `GET /endorsements`, `GET /trust-card`, `GET /trust/{kind}/{id}`, `GET /badge/{kind}/{id}.svg`, Commit Score computation — all unchanged. The extension's proving flow is unchanged (it already sends full attestation hex).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `tlsn-core` git dependency has conflicts with existing deps (edition 2024, version mismatches) | Fallback: replicate minimal `Attestation`, `Header`, `Signature` structs with matching BCS serde attributes. More fragile but avoids the dependency tree. |
| `tlsn-core` increases compile time significantly (~19 direct deps) | Accept for correctness. The dependency is security-critical and version-pinned. Consider `cargo build --release` in CI only. |
| BCS serialization of `Header` produces different bytes than what the notary signed (field ordering, version differences) | Pinned to same tag `v0.1.0-alpha.12` across all components. BCS is deterministic for identical struct definitions. |
| Test fixture approach: constructing valid `Attestation` in tests is complex (`Body` has private fields) | Use captured real attestation from the live system as test fixture. Document the capture procedure. |
| Removing webhook fallback breaks any remaining webhook callers not sending attestation | Only caller is the own notary server, which sends attestation. No external callers exist. |
| Notary key rotation invalidates cached `VerifyingKey` in AppState (requires restart) | Acceptable at current scale (<50 users). Document the rotation procedure: update Fly secret → restart both apps. |

## Documentation / Operational Notes

- After deployment: verify with `curl -X POST .../endorsements` with a valid attestation — should succeed. With garbage bytes — should get 401.
- Key rotation procedure: generate new notary signing key → `fly secrets set` on verifier → restart verifier → fetch new public key from `/info` → `fly secrets set NOTARY_PUBLIC_KEY` on backend → restart backend.
- Update CLAUDE.md Phase 2 checklist: mark "full attestation signature verification" and "deprecate webhook hash_verification_results fallback" as done.

## Sources & References

- **Origin document:** [P0 proof-binding fix](docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md) — loose thread #3 (attestation verification), #5 (webhook fallback deprecation)
- **Notary server plan:** [docs/plans/2026-04-12-001-feat-own-notary-server-plan.md](docs/plans/2026-04-12-001-feat-own-notary-server-plan.md) — Unit 3 stored the public key
- TLSNotary source: `github.com/tlsnotary/tlsn` tag `v0.1.0-alpha.12`, `crates/core/src/attestation.rs`, `crates/core/src/signing.rs`
- k256 ECDSA docs: `docs.rs/k256/0.13/k256/ecdsa/`
- BCS serialization: `docs.rs/bcs/`
- TLSNotary WASM integration: `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md`
