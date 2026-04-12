---
title: "feat: L2 attestation submission pipeline"
type: feat
status: active
date: 2026-04-12
---

# feat: L2 attestation submission pipeline

## Overview

Wire up the backend to submit endorsement attestation hashes to the `CommitAttestationRegistry` contract on Base Sepolia. The contract is deployed and tested. The backend already creates local `attestations` table rows with `chain = 'pending'`. This plan adds the missing piece: a background batch job that reads pending attestations, calls `attestBatch()` on the L2 contract, and updates the local rows with `tx_hash` and `block_number`.

## Problem Frame

The `attestations` table stores a row for every verified endorsement (created in `webhook.rs`), but `tx_hash` is always NULL and `chain` stays `'pending'`. No code exists to actually submit these hashes on-chain. The L2 contract (`CommitAttestationRegistry` at `0x08AE2e7fd94130645725Afc69e9BE2140f2395d7` on Base Sepolia) is deployed and has `attest()` and `attestBatch()` functions, both `onlyOwner`. The backend needs an Ethereum client library, a signing key, and a periodic batch submission job.

The attestation table is also only populated via the webhook path (`POST /webhook/endorsement`). The direct submission path (`POST /endorsements`) does not create attestation rows. This must be fixed so all verified endorsements get on-chain attestation.

## Requirements Trace

- R1. All `verified` endorsements must have a corresponding attestation row in the local DB
- R2. Pending attestations must be submitted to the L2 contract in batches (up to 500 per tx)
- R3. After successful on-chain submission, the local attestation row must be updated with `tx_hash`, `block_number`, and `attested_at`
- R4. Failed submissions must be retried on the next batch cycle, not silently dropped
- R5. The L2 submission must run as a background task, not blocking API request handling
- R6. The contract owner private key must be configured via environment variable, never hardcoded
- R7. The system must be resilient to RPC failures and not crash the backend on L2 errors

## Scope Boundaries

- Base Mainnet deployment — this plan targets Base Sepolia only. Mainnet migration is a separate task.
- Trust card display of on-chain attestation status — the trust page already shows endorsements; adding "on-chain verified" badges is a follow-up.
- Gas price optimization or EIP-1559 fee management — Base Sepolia has negligible gas costs. Not worth optimizing now.
- Contract upgrades or redeployment — the current contract is sufficient.

### Deferred to Separate Tasks

- Base Mainnet deployment: new contract deployment + key management for real funds
- On-chain verification display in trust card / trust page UI
- Gas monitoring and alerting

## Context & Research

### Relevant Code and Patterns

- `contracts/src/CommitAttestationRegistry.sol` — `attest(bytes32, bytes32)`, `attestBatch(bytes32[], bytes32[])`, `verify(bytes32, bytes32)`, MAX_BATCH_SIZE=500, onlyOwner
- `contracts/deployments/base-sepolia.json` — address `0x08AE2e7fd94130645725Afc69e9BE2140f2395d7`, chain_id 84532
- `src/services/db.rs:259-275` — `create_attestation()` (stores pending row), `update_attestation_tx()` (updates tx_hash + block_number)
- `src/routes/webhook.rs:~200` — calls `db.create_attestation()` after creating verified endorsement
- `src/routes/endorsement.rs` — direct submission path does NOT create attestation rows
- `src/main.rs` — tokio runtime, AppState with Arc<Mutex<Database>>
- `Cargo.toml` — no Ethereum client library yet. Has `k256` for ECDSA, `sha2` for hashing, `hex` for encoding.

### L2 Client Library Choice

**alloy** (Paradigm) is the modern Rust Ethereum library, replacing ethers-rs. It provides:
- Transaction signing with local private keys
- Contract interaction via ABI encoding
- Provider for JSON-RPC calls
- Built on the same `k256` ECDSA primitives already in the project

alloy is well-maintained, async-native (tokio), and the recommended choice for new Rust Ethereum projects as of 2025+.

### Contract ABI

The contract is simple enough that ABI encoding can be done manually using alloy's `sol!` macro, which generates type-safe Rust bindings from Solidity function signatures. No need for a separate ABI JSON file.

## Key Technical Decisions

- **alloy as the Ethereum client.** Modern, tokio-native, same `k256` foundation already in the project. The `sol!` macro generates type-safe bindings directly from Solidity function signatures.

- **Background tokio task, not a cron job.** The batch submitter runs as a `tokio::spawn` task at startup with a configurable interval (default: 5 minutes). It reads pending attestations, batches them, submits, and updates. This avoids external cron complexity and runs in the same process. If the backend restarts, the task restarts — pending attestations are already persisted in SQLite.

- **Batch size of 100, not 500.** The contract allows 500 per tx, but smaller batches reduce gas cost per transaction and limit blast radius if a tx fails. 100 is a reasonable default that can handle Phase 2 volumes (< 100 endorsements/day) in a single batch most cycles.

- **Direct submission path must also create attestation rows.** Currently only the webhook path creates them. After the endorsement status is set to `verified` in `endorsement.rs` (when attestation signature verification passes), the same `create_attestation()` call should follow. This ensures all verified endorsements get on-chain attestation regardless of submission path.

- **Private key via `L2_PRIVATE_KEY` env var.** Hex-encoded 32-byte secp256k1 private key. The same key that deployed the contract (contract owner). If not set, the background task logs a warning and does not start — the backend works without L2 submission.

- **Contract address and RPC URL via env vars.** `L2_CONTRACT_ADDRESS` and `L2_RPC_URL` with defaults for Base Sepolia (the address from `deployments/base-sepolia.json` and `https://sepolia.base.org`). This allows switching to mainnet by changing env vars.

- **Graceful degradation on RPC failure.** If the RPC call fails, log the error and leave attestations as pending. The next batch cycle will retry. No exponential backoff — the 5-minute interval provides natural spacing. If a specific attestation is rejected by the contract (e.g., "already attested"), mark it as `attested` locally and skip it in future batches.

## Open Questions

### Resolved During Planning

- **alloy vs ethers-rs?** alloy — it's the successor, actively maintained, tokio-native.
- **Batch or individual submissions?** Batch — reduces transaction count and gas cost.
- **What if the backend restarts mid-batch?** Attestations remain `pending` in SQLite. The next batch cycle picks them up. No in-memory state to lose.
- **Should the extension interact with L2?** No — the backend is the contract owner and sole submitter. The extension never touches L2 directly.

### Deferred to Implementation

- Exact alloy crate features to enable (minimal set for contract calls + signing)
- Whether `alloy-sol-types` or `alloy-sol-macro` is the better approach for the `sol!` macro
- Gas limit estimation — may need to be hardcoded for Base Sepolia or use `eth_estimateGas`

## Implementation Units

- [ ] **Unit 1: Backend — Create attestation rows from direct submission path**

**Goal:** Ensure all verified endorsements — not just webhook-submitted ones — get attestation rows in the database.

**Requirements:** R1

**Dependencies:** None

**Files:**
- Modify: `src/routes/endorsement.rs`
- Test: `tests/api.rs`

**Approach:**
- In `submit_endorsement()`, after the block that updates endorsement status to `verified` (when `verify_attestation_signature()` passes), add a `db.create_attestation()` call with a new UUID, the endorsement ID, and `"base_sepolia"` chain.
- Follow the exact pattern from `webhook.rs:~200` where this already happens.
- If the notary public key is not configured (status stays `pending_attestation`), skip the attestation row — only verified endorsements get on-chain attestation.

**Patterns to follow:**
- `webhook.rs` attestation creation block

**Test scenarios:**
- Happy path: POST /endorsements with valid attestation + notary key configured → endorsement verified + attestation row created in DB
- Happy path: POST /endorsements without notary key → pending_attestation status, no attestation row
- Regression: existing webhook path still creates attestation rows

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean

---

- [ ] **Unit 2: Backend — Add alloy dependency and L2 service module**

**Goal:** Add the alloy Ethereum client library and create a service module that encapsulates L2 contract interaction.

**Requirements:** R6

**Dependencies:** None (parallel with Unit 1)

**Files:**
- Modify: `Cargo.toml` (add alloy dependencies)
- Create: `src/services/l2.rs`
- Modify: `src/services/mod.rs` (add `pub mod l2`)

**Approach:**
- Add alloy dependencies to `Cargo.toml`: `alloy-provider`, `alloy-signer-local`, `alloy-primitives`, `alloy-sol-types`, `alloy-network`, `alloy-transport-http` with minimal features. Use the `alloy` meta-crate if it simplifies feature selection.
- Create `src/services/l2.rs` with:
  - `sol!` macro generating bindings for `attestBatch(bytes32[], bytes32[])` function signature
  - `L2Client` struct holding the provider, signer, and contract address
  - `L2Client::new(rpc_url: &str, private_key: &str, contract_address: &str) -> Result<Self>` constructor
  - `L2Client::submit_batch(endorsement_ids: &[Uuid], proof_hashes: &[[u8; 32]]) -> Result<TxHash>` method that encodes and sends the `attestBatch` call
  - `L2Client::wait_for_receipt(tx_hash: TxHash) -> Result<(String, u64)>` that polls for confirmation and returns `(tx_hash_hex, block_number)`
- The private key is parsed from hex into an alloy `LocalSigner`.
- The contract address is parsed from hex into an `Address`.

**Patterns to follow:**
- alloy `sol!` macro for contract bindings
- alloy `ProviderBuilder::new().on_http(url)` for RPC connection
- Existing service module pattern (`src/services/github.rs`, `src/services/db.rs`)

**Test scenarios:**
Test expectation: none — L2 client requires a real RPC endpoint. Verified via integration in Unit 3.

**Verification:**
- `cargo build` succeeds with new dependencies
- `cargo clippy -- -D warnings` clean
- Module compiles without errors

---

- [ ] **Unit 3: Backend — Background batch submission task**

**Goal:** A tokio background task periodically reads pending attestations from SQLite, batches them, submits to the L2 contract, and updates local records on success.

**Requirements:** R2, R3, R4, R5, R7

**Dependencies:** Units 1, 2

**Files:**
- Modify: `src/services/db.rs` (add `get_pending_attestations` and `mark_attestation_failed` queries)
- Modify: `src/services/l2.rs` (add batch submission orchestration)
- Modify: `src/main.rs` (spawn background task, parse env vars, add L2Client to AppState or pass separately)
- Modify: `src/lib.rs` (optionally add L2Client to AppState)
- Test: `tests/api.rs` (verify attestation rows are created; L2 submission itself is tested manually against Base Sepolia)

**Approach:**
- In `db.rs`: add `get_pending_attestations(limit: u32) -> Result<Vec<PendingAttestation>>` where `PendingAttestation = { id, endorsement_id, endorsement_proof_hash }`. Query joins `attestations` with `endorsements` to get the proof_hash. Filters: `attestations.tx_hash IS NULL AND attestations.chain = 'base_sepolia'`. Ordered by `created_at ASC` (oldest first).
- In `l2.rs`: add `run_batch_submitter(db: Arc<Mutex<Database>>, l2: L2Client, interval_secs: u64)` async function. Loop: sleep for interval, query pending, chunk into batches of 100, submit each batch, on success update each attestation row with `tx_hash` and `block_number`, on failure log and continue (retry next cycle).
- Convert endorsement UUID to `bytes32`: parse UUID string, left-pad to 32 bytes. The contract uses `bytes32` for endorsement IDs.
- Convert proof_hash (`Vec<u8>` from DB, SHA-256 = 32 bytes) to `bytes32` directly.
- Handle contract revert "already attested": mark the local attestation as complete (idempotent) rather than retrying forever.
- In `main.rs`: parse `L2_PRIVATE_KEY`, `L2_CONTRACT_ADDRESS` (default: `0x08AE2e7fd94130645725Afc69e9BE2140f2395d7`), `L2_RPC_URL` (default: `https://sepolia.base.org`), `L2_BATCH_INTERVAL_SECS` (default: 300). If `L2_PRIVATE_KEY` is set, construct `L2Client` and spawn the background task. If not set, log info and skip.

**Patterns to follow:**
- `tokio::spawn` for background tasks
- `tokio::time::interval` for periodic execution
- `tracing::info!` / `tracing::error!` for observability
- `std::env::var` pattern from `main.rs` (GITHUB_TOKEN, NOTARY_PUBLIC_KEY)

**Test scenarios:**
- Happy path: create endorsement → verify it creates attestation row → confirm `tx_hash` is NULL → (manual) run submitter against Base Sepolia → confirm `tx_hash` is populated
- Error: RPC endpoint unreachable → attestation stays pending, no crash
- Error: contract reverts "already attested" → attestation marked complete locally
- Edge case: no pending attestations → batch submitter sleeps, no RPC call
- Regression: backend starts without L2_PRIVATE_KEY → no L2 task, no crash, warning logged

**Verification:**
- `cargo test` passes (unit tests don't require L2 connectivity)
- Backend starts with and without `L2_PRIVATE_KEY` — both work
- Manual: set env vars, create an endorsement, wait for batch cycle, verify tx on Base Sepolia block explorer

---

- [ ] **Unit 4: Backend — Attestation status query for trust card**

**Goal:** Expose on-chain attestation status so the trust card API and SSR page can display whether an endorsement has been attested on-chain.

**Requirements:** R3

**Dependencies:** Unit 3

**Files:**
- Modify: `src/services/db.rs` (add `get_attestation_status` query)
- Modify: `src/routes/trust_card.rs` (include attestation status in `EndorsementSummary`)
- Modify: `src/routes/trust_page.rs` (render on-chain badge for attested endorsements)
- Modify: `src/models/endorsement.rs` (add `on_chain` field to `EndorsementSummary`)

**Approach:**
- In `db.rs`: add `get_attestation_for_endorsement(endorsement_id: &str) -> Result<Option<AttestationRow>>` where `AttestationRow = { tx_hash: Option<String>, chain: String, block_number: Option<i64> }`.
- In `EndorsementSummary`: add `on_chain: bool` and `tx_hash: Option<String>` fields.
- In `trust_card.rs` `map_endorsement_rows()`: for each endorsement, check if an attestation with a non-NULL `tx_hash` exists. Set `on_chain = true` and include the `tx_hash` if so.
- In `trust_page.rs`: for endorsed endorsements that are on-chain, render a small "On-chain" tag with a link to the Base Sepolia block explorer (`https://sepolia.basescan.org/tx/{tx_hash}`).
- This adds a single extra query per recent endorsement when building the trust card. At current volumes (< 5 recent endorsements), this is negligible. If it becomes a concern, batch the query.

**Patterns to follow:**
- `get_recent_endorsements` query pattern
- `EndorsementSummary` struct in `src/models/endorsement.rs`
- Trust page rendering patterns in `trust_page.rs`

**Test scenarios:**
- Happy path: endorsement with attested attestation → `on_chain: true`, `tx_hash` present in API response
- Happy path: endorsement with pending attestation → `on_chain: false`, `tx_hash: null`
- Happy path: endorsement with no attestation row → `on_chain: false`
- Integration: trust card API includes `on_chain` field in `recent_endorsements`

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- Trust card API response includes `on_chain` status for each endorsement

## System-Wide Impact

- **New dependency:** alloy crate adds Ethereum client capabilities. This increases compile time (alloy is substantial). The `tlsn-core` git dep already dominates build time, so the relative impact is moderate. CI caching mitigates.
- **Background task:** The batch submitter runs in the tokio runtime alongside the axum server. It holds a `Mutex<Database>` lock briefly per batch (< 100ms for reading pending + updating). This should not cause contention at current request volumes.
- **Env var additions:** `L2_PRIVATE_KEY`, `L2_CONTRACT_ADDRESS`, `L2_RPC_URL`, `L2_BATCH_INTERVAL_SECS`. All optional — backend works without L2 config.
- **Trust card API response change:** `EndorsementSummary` gains `on_chain: bool` and `tx_hash: Option<String>`. Additive, non-breaking.
- **Direct submission parity:** After Unit 1, both `POST /endorsements` and `POST /webhook/endorsement` create attestation rows for verified endorsements. Previously only the webhook path did.
- **Unchanged invariants:** Score computation, extension endorsement flow, badge API, network keyring — all unaffected.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| alloy compile time increase | CI caching via `Swatinem/rust-cache` already handles large deps. Monitor first CI build time. |
| Base Sepolia RPC unreliable | Graceful degradation: log error, retry next cycle. Attestations persist locally regardless of L2 status. Consider adding a backup RPC endpoint as a follow-up. |
| Private key exposure in env var | Fly.io secrets are encrypted at rest. The key is only for Base Sepolia (testnet) — no real funds at risk. For mainnet, use a more secure key management approach (e.g., Fly.io secrets + hardware signer). |
| Contract owner key rotation | If the deployer key is lost, a new contract must be deployed. Store the key securely. The `transferOwnership()` function allows migrating to a new key without redeployment. |
| Mutex contention from background task | The batch submitter holds the DB lock for < 100ms per cycle. At 5-minute intervals, this is negligible. If needed, switch to a connection pool (r2d2-rusqlite) later. |
| UUID-to-bytes32 conversion ambiguity | UUID v4 is 16 bytes. Left-pad with zeros to 32 bytes. This is deterministic and reversible. Document the encoding. |

## Sources & References

- Contract source: `contracts/src/CommitAttestationRegistry.sol`
- Deployment record: `contracts/deployments/base-sepolia.json`
- Attestation DB methods: `src/services/db.rs:259-275`
- Webhook attestation creation: `src/routes/webhook.rs:~200`
- alloy documentation: https://alloy.rs
