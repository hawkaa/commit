---
title: TLSNotary attestation signature verification in Rust
date: 2026-04-12
category: security-issues
module: endorsement
problem_type: security_issue
component: authentication
symptoms:
  - Backend accepted any hex bytes as valid attestation data without cryptographic verification
  - POST /endorsements and POST /webhook/endorsement stored attestations with no proof the trusted notary signed them
  - proof_hash was SHA-256 of raw bytes, not bound to notary identity
root_cause: missing_validation
resolution_type: code_fix
severity: high
tags:
  - tlsnotary
  - attestation
  - ecdsa
  - secp256k1
  - signature-verification
  - bcs
  - k256
  - notary-public-key
---

# TLSNotary attestation signature verification in Rust

## Problem

The backend computed `proof_hash = SHA-256(attestation_bytes)` and stored TLSNotary attestations without verifying the notary's ECDSA-secp256k1 signature. Any valid-looking hex byte sequence was accepted as a legitimate attestation. This meant there was no cryptographic proof that the trusted notary actually signed the attestation, undermining the entire endorsement trust chain.

This was a known P0 security loose thread from the proof-binding fix, explicitly deferred until the own notary server was deployed with a known public key.

## Symptoms

- `POST /endorsements` accepted arbitrary hex in the `attestation` field with no signature check
- `POST /webhook/endorsement` fell back to `hash_verification_results_with_transcript` when attestation was absent, computing proof_hash from attacker-controlled JSON fields
- No startup-time key configuration or validation existed
- The `NOTARY_PUBLIC_KEY` env var was loaded into `AppState` as a raw PEM string but never parsed or used

## What Didn't Work

- **`Presentation::verify()` (tlsn-core public API):** The extension sends raw `Attestation` hex from `prover.notarize()`, not a `Presentation`. The `Presentation` type wraps `AttestationProof` with selective disclosure, but the extension never creates one. This API path was unusable without extension changes. (session history)

- **`AttestationProof::verify()` (internal API):** This method handles the full verification chain (Merkle proof + signature), but it's `pub(crate)` in `tlsn-core` v0.1.0-alpha.12. Not callable from outside the crate.

- **Minimal struct replication (avoid tlsn-core dependency):** Considered replicating just `Attestation`, `Header`, `Signature` structs with matching BCS serde attributes to avoid the ~50-crate transitive dependency tree. Rejected because BCS is field-order-dependent and the `Body` type has many nested fields. Any mismatch in struct layout would silently break deserialization. The risk wasn't worth the compile-time savings.

## Solution

Added server-side ECDSA-secp256k1 signature verification using three new dependencies:

- `tlsn-core` (git dep, pinned by commit SHA `f2ff4ba7`) for `Attestation`, `Header`, `Signature` types
- `k256 v0.13` (features: `ecdsa`, `pem`) for ECDSA verification and PEM parsing
- `bcs v0.1` for Binary Canonical Serialization

### Key parsing at startup (`src/main.rs`)

```rust
let notary_public_key = match std::env::var("NOTARY_PUBLIC_KEY") {
    Ok(pem) => {
        use k256::pkcs8::DecodePublicKey;
        let key = k256::ecdsa::VerifyingKey::from_public_key_pem(&pem)
            .expect("NOTARY_PUBLIC_KEY contains invalid PEM — cannot start");
        Some(key)
    }
    Err(_) => None,
};
```

`AppState.notary_public_key` changed from `Option<String>` to `Option<k256::ecdsa::VerifyingKey>`. Parse once, fail fast on invalid PEM.

### Verification function (`src/validation.rs`)

```rust
pub fn verify_attestation_signature(
    attestation_bytes: &[u8],
    trusted_key: &k256::ecdsa::VerifyingKey,
) -> Result<(), StatusCode> {
    // 1. BCS-deserialize the full Attestation
    let attestation: tlsn_core::attestation::Attestation =
        bcs::from_bytes(attestation_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;

    // 2. Validate algorithm is SECP256K1
    if attestation.signature.alg != tlsn_core::signing::SignatureAlgId::SECP256K1 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // 3. BCS-serialize the Header (this is what the notary signed)
    let header_bytes = bcs::to_bytes(&attestation.header)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // 4. Parse ECDSA signature from raw bytes
    let signature = k256::ecdsa::Signature::from_slice(&attestation.signature.data)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // 5. Verify with the TRUSTED key (not the embedded body key)
    trusted_key.verify(&header_bytes, &signature)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}
```

### Integration into both endpoints

```rust
// After hex-decode, before proof_hash computation
if let Some(ref key) = state.notary_public_key {
    verify_attestation_signature(&attestation_bytes, key)?;
}
```

### Webhook hardening

- `attestation: Option<String>` changed to `attestation: String` (required field)
- Removed `hash_verification_results_with_transcript` fallback function entirely
- All webhook test payloads updated to include `attestation` field

## Why This Works

The TLSNotary attestation blob is BCS-serialized (Binary Canonical Serialization, not bincode). The notary signs `bcs::to_bytes(&header)` where `Header` contains:
- `id: Uid` (random 16-byte identifier)
- `version: Version`
- `root: TypedHash` (Merkle root committing to all Body fields)

The signature is ECDSA-secp256k1 with SHA-256 hashing (done internally by `k256::ecdsa::VerifyingKey::verify()`).

**Why verify with the trusted key, not the embedded key:** The `Attestation.body.verifying_key` contains a self-certifying key embedded by the notary. Verifying against `NOTARY_PUBLIC_KEY` (from `GET /info` on our own notary server) is strictly stronger. It proves *our* notary signed it, not just *any* notary.

**Why the Header signature transitively authenticates the Body:** `Header.root` is a Merkle root of all Body fields (verifying_key, connection_info, transcript_commitments, etc.). The notary computes this root before signing the header. Tampering with the body after signing would require finding a SHA-256 preimage collision.

**Error code semantics:** 400 = malformed request (bad hex, BCS failure, wrong algorithm). 401 = structurally valid attestation but not signed by the trusted notary.

## Prevention

- **Pin `tlsn-core` by commit SHA, not tag.** Git tags are mutable. The Cargo.toml uses `rev = "f2ff4ba7"` to prevent supply-chain attacks via tag force-push. This is a security-critical dependency where type definitions directly affect signature verification correctness.

- **Parse the key at startup, not per-request.** `VerifyingKey::from_public_key_pem()` runs once. Invalid PEM panics immediately rather than silently failing on every request. The parsed key is immutable in `AppState`.

- **Add `git` to Dockerfile `apk add`.** Cargo needs git to fetch git dependencies. Without it, Docker builds fail at `cargo fetch` with a confusing "registry not found" error.

- **When adding attestation as a required field, update ALL test payloads.** The `webhook_payload()` helper and every custom webhook test JSON payload must include the field. Missing it causes 422 deserialization errors that mask the actual test logic (auth checks, validation, etc.).

- **Test the dependency compiles before committing to the approach.** `tlsn-core` with Rust edition 2024 was the load-bearing unknown. Running `cargo check` before writing any implementation code resolved it in seconds and avoided a potential rework of the entire approach.

## Related Issues

- [P0 proof-binding security fix](../security-issues/tlsnotary-proof-binding-and-subject-validation-2026-04-11.md) — predecessor fix that bound proof_hash to attestation bytes and transcript to subject. This signature verification was loose thread #3 from that plan.
- [Own notary server deployment plan](../../docs/plans/2026-04-12-001-feat-own-notary-server-plan.md) — deployed `commit-verifier.fly.dev` with persistent secp256k1 signing key, unblocking this work.
- [Attestation verification plan](../../docs/plans/2026-04-12-002-feat-attestation-signature-verification-plan.md) — the implementation plan for this fix.
