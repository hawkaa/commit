# Changelog

All notable changes to Commit will be documented in this file.

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
