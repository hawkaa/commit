# Changelog

All notable changes to Commit will be documented in this file.

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
