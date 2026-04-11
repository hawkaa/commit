# TODOs

## Phase 2 — ZK Integrity

### P0: proof_hash not bound to cryptographic attestation material
**Priority:** P0
**Status:** Open
**Context:** `src/routes/webhook.rs:163-173`. The `hash_verification_results` function hashes payload fields (server_name, session.id, results) which are all attacker-controlled input. The resulting proof_hash has no binding to the actual TLSNotary attestation blob. Fix requires storing and verifying the attestation server-side, which depends on deploying our own notary/verifier.
**Found by:** ce:review on tlsnotary-integration, 2026-04-11

### P0: session.data subject injection — proof for repo A can endorse repo B
**Priority:** P0
**Status:** Open
**Context:** `src/routes/webhook.rs:74-98`. The extension sets subject_kind and subject_id in session.data, and the notary passes them through verbatim. There is no cryptographic binding between the MPC-TLS proof target and the claimed subject. Fix requires extracting the proved server_name + request path from the attestation transcript and matching against the claimed subject.
**Found by:** ce:review on tlsnotary-integration, 2026-04-11
