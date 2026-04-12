# Changelog

All notable changes to Commit will be documented in this file.

## [0.1.3.0] - 2026-04-12

### Added
- Server-side ECDSA-secp256k1 attestation signature verification using `tlsn-core`, `k256`, and `bcs`
- `verify_attestation_signature()` function in `src/validation.rs` that deserializes TLSNotary attestations from BCS and verifies the notary's signature
- Signature algorithm validation (rejects non-secp256k1 attestations)
- Solution documentation for the verification approach (`docs/solutions/security-issues/`)

### Changed
- `AppState.notary_public_key` upgraded from raw PEM string to parsed `k256::ecdsa::VerifyingKey` (parsed once at startup)
- Webhook `attestation` field is now required (was `Option<String>`)
- `tlsn-core` dependency pinned by commit SHA instead of mutable git tag

### Removed
- `hash_verification_results_with_transcript` fallback function (webhook always requires attestation now)
- Webhook backward-compatibility path for missing attestation data

## [0.1.2.0] - 2026-04-12

### Added
- Own TLSNotary notary server deployed to Fly.io (`commit-verifier.fly.dev`) with persistent secp256k1 signing key
- Notary server Dockerfile, config.yaml, and entrypoint.sh for key injection from Fly secrets
- `NOTARY_PUBLIC_KEY` config in backend AppState for future attestation signature verification
- Extension host_permissions for the own notary server domain

### Changed
- Extension `NOTARY_URL` points to `commit-verifier.fly.dev` instead of public `notary.pse.dev`
- WebSocket proxy (`PROXY_BASE`) remains on `notary.pse.dev` (stateless, deferred)
- Notary server fly.toml switched from image reference to local Dockerfile build

## [0.1.1.0] - 2026-04-11

### Added
- TLSNotary MPC-TLS proving in Chrome extension via offscreen WASM document (~5s proof generation)
- "Endorse" button on GitHub trust cards for ZK-verified endorsements
- Webhook endpoint (POST /webhook/endorsement) for receiving verified proofs from TLSNotary verifier
- Fail-closed webhook authentication with VERIFIER_WEBHOOK_SECRET
- Server name validation per proof type (api.github.com for git_history, Google/Outlook for email)
- Verifier server Fly.io deployment config (verifier/fly.toml)
- 5 webhook endpoint tests (auth, validation, server name, 404)

### Changed
- Extension bumped to v0.2.0 with offscreen permission and wasm-unsafe-eval CSP
- Replaced innerHTML with DOM construction in content script (XSS prevention)
- Corrected ZK documentation: TLSNotary uses MPC-TLS + QuickSilver, not Halo2

## [0.1.0.0] - 2026-04-11

### Added
- L2 attestation registry contract deployed on Base Sepolia testnet
- `attest()` and `attestBatch()` for recording endorsement proof hashes on-chain
- `verify()` for public proof hash verification
- Owner-only access control with `transferOwnership()` escape hatch
- Zero proof hash rejection and MAX_BATCH_SIZE=500 safety guards
- 20 Foundry tests covering all functions, revert paths, and edge cases
- Deployment receipt at `contracts/deployments/base-sepolia.json`
- Foundry project setup (Solidity 0.8.30, forge-std v1.15.0)
