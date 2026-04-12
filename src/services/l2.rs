use alloy::{
    network::Ethereum,
    primitives::{Address, FixedBytes, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::db::Database;

// Type-safe bindings for the CommitAttestationRegistry.attestBatch() function.
sol! {
    function attestBatch(bytes32[] endorsementIds, bytes32[] proofHashes) external;
}

/// Ethereum L2 client for submitting attestation batches to the
/// `CommitAttestationRegistry` contract on Base Sepolia.
pub struct L2Client {
    provider: DynProvider<Ethereum>,
    contract_address: Address,
}

/// Maximum number of attestations per on-chain transaction.
const BATCH_SIZE: usize = 100;

#[allow(clippy::missing_errors_doc)]
impl L2Client {
    /// Create a new L2 client from raw configuration values.
    ///
    /// - `rpc_url`: HTTP JSON-RPC endpoint (e.g. `https://sepolia.base.org`)
    /// - `private_key`: hex-encoded 32-byte secp256k1 private key (with or without `0x` prefix)
    /// - `contract_address`: hex-encoded contract address (with `0x` prefix)
    pub fn new(
        rpc_url: &str,
        private_key: &str,
        contract_address: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signer: PrivateKeySigner = private_key.parse()?;
        let wallet = alloy::network::EthereumWallet::from(signer);

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(rpc_url.parse()?)
            .erased();

        let address: Address = contract_address.parse()?;

        Ok(Self {
            provider,
            contract_address: address,
        })
    }

    /// Submit a batch of attestations to the L2 contract.
    ///
    /// Encodes and sends a `attestBatch(bytes32[], bytes32[])` transaction.
    /// Returns the transaction hash as a hex string.
    pub async fn submit_batch(
        &self,
        endorsement_ids: &[FixedBytes<32>],
        proof_hashes: &[FixedBytes<32>],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let call = attestBatchCall {
            endorsementIds: endorsement_ids.to_vec(),
            proofHashes: proof_hashes.to_vec(),
        };
        let calldata = call.abi_encode();

        let tx = alloy::rpc::types::TransactionRequest::default()
            .to(self.contract_address)
            .input(calldata.into())
            .value(U256::ZERO);

        let pending = self.provider.send_transaction(tx).await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());

        Ok(tx_hash)
    }

    /// Wait for a transaction to be included in a block.
    ///
    /// Returns `(tx_hash_hex, block_number)`.
    pub async fn wait_for_receipt(
        &self,
        tx_hash: &str,
    ) -> Result<(String, u64), Box<dyn std::error::Error + Send + Sync>> {
        let hash: FixedBytes<32> = tx_hash.parse()?;
        let receipt = self
            .provider
            .get_transaction_receipt(hash)
            .await?
            .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                "transaction receipt not found".into()
            })?;

        let block_number = receipt.block_number.unwrap_or(0);
        Ok((tx_hash.to_string(), block_number))
    }
}

/// Convert a UUID v4 (16 bytes) to a `bytes32` by left-padding with zeros.
#[must_use]
pub fn uuid_to_bytes32(id: &Uuid) -> FixedBytes<32> {
    let uuid_bytes = id.as_bytes();
    let mut bytes = [0u8; 32];
    // Left-pad: place 16 UUID bytes in the rightmost 16 positions
    bytes[16..].copy_from_slice(uuid_bytes);
    FixedBytes::from(bytes)
}

/// Convert a 32-byte proof hash to `FixedBytes<32>`.
///
/// Returns `None` if the input is not exactly 32 bytes.
#[must_use]
pub fn proof_hash_to_bytes32(hash: &[u8]) -> Option<FixedBytes<32>> {
    if hash.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash);
    Some(FixedBytes::from(bytes))
}

/// Pending attestation row joined with endorsement proof_hash from the database.
#[derive(Debug)]
pub struct PendingAttestation {
    pub id: String,
    pub endorsement_id: String,
    pub endorsement_proof_hash: Vec<u8>,
}

/// Background batch submitter: periodically reads pending attestations from the
/// database, submits them on-chain in batches, and updates the local records.
///
/// Catches panics from the inner loop to prevent silent death of the task.
pub async fn run_batch_submitter(
    db: Arc<Mutex<Database>>,
    l2: L2Client,
    interval_secs: u64,
) {
    loop {
        let result = run_batch_submitter_inner(&db, &l2, interval_secs).await;
        match result {
            Ok(()) => {
                tracing::error!("L2 batch submitter exited unexpectedly — restarting in 60s");
            }
            Err(e) => {
                tracing::error!(
                    "L2 batch submitter panicked: {e} — restarting in 60s"
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

async fn run_batch_submitter_inner(
    db: &Arc<Mutex<Database>>,
    l2: &L2Client,
    interval_secs: u64,
) -> Result<(), String> {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    // Skip the first immediate tick — let the server finish starting
    interval.tick().await;

    loop {
        interval.tick().await;
        if let Err(e) = process_pending_batch(db, l2).await {
            tracing::error!("L2 batch submission error: {e}");
        }
    }
}

async fn process_pending_batch(
    db: &Arc<Mutex<Database>>,
    l2: &L2Client,
) -> Result<(), String> {
    let pending = {
        let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
        db.get_pending_attestations(BATCH_SIZE as u32)
            .map_err(|e| format!("DB error fetching pending attestations: {e}"))?
    };

    if pending.is_empty() {
        tracing::debug!("No pending attestations to submit");
        return Ok(());
    }

    tracing::info!("Submitting {} attestations to L2", pending.len());

    let mut endorsement_ids = Vec::with_capacity(pending.len());
    let mut proof_hashes = Vec::with_capacity(pending.len());
    let mut attestation_ids = Vec::with_capacity(pending.len());

    for row in &pending {
        let eid = Uuid::parse_str(&row.endorsement_id)
            .map_err(|e| format!("Invalid endorsement UUID: {e}"))?;
        endorsement_ids.push(uuid_to_bytes32(&eid));

        let ph = proof_hash_to_bytes32(&row.endorsement_proof_hash)
            .ok_or_else(|| format!("Invalid proof hash length for attestation {}", row.id))?;
        proof_hashes.push(ph);

        attestation_ids.push(row.id.clone());
    }

    // Submit batch to L2
    let tx_hash = match l2.submit_batch(&endorsement_ids, &proof_hashes).await {
        Ok(hash) => hash,
        Err(e) => {
            let err_str = e.to_string();
            // If the contract reverts with "already attested", one item caused the
            // revert but others may not be on-chain yet. Fall back to single-item
            // submission so each item gets the correct treatment.
            if err_str.contains("already attested") {
                tracing::warn!(
                    "Batch reverted with 'already attested' — falling back to single-item submission"
                );
                submit_items_individually(
                    db,
                    l2,
                    &pending,
                    &endorsement_ids,
                    &proof_hashes,
                    &attestation_ids,
                )
                .await?;
                return Ok(());
            }
            return Err(err_str);
        }
    };

    tracing::info!("L2 tx submitted: {tx_hash}");

    // Wait for receipt and get block number
    let (tx_hash_confirmed, block_number) = match l2.wait_for_receipt(&tx_hash).await {
        Ok(receipt) => receipt,
        Err(e) => {
            tracing::warn!(
                "Could not get receipt for {tx_hash}: {e} — will retry next cycle"
            );
            // Leave attestations as pending; they will be retried
            return Ok(());
        }
    };

    // Update all attestation rows with tx_hash and block_number
    let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
    for att_id in &attestation_ids {
        let uid =
            Uuid::parse_str(att_id).map_err(|e| format!("Invalid attestation UUID: {e}"))?;
        db.update_attestation_tx(&uid, &tx_hash_confirmed, block_number as i64)
            .map_err(|e| format!("DB error updating attestation {att_id}: {e}"))?;
    }

    tracing::info!(
        "Updated {} attestations with tx {} at block {}",
        attestation_ids.len(),
        tx_hash_confirmed,
        block_number
    );

    Ok(())
}

/// Submit attestations one at a time. Used as fallback when a batch reverts
/// with "already attested" — we don't know which item caused the revert.
async fn submit_items_individually(
    db: &Arc<Mutex<Database>>,
    l2: &L2Client,
    pending: &[PendingAttestation],
    endorsement_ids: &[FixedBytes<32>],
    proof_hashes: &[FixedBytes<32>],
    attestation_ids: &[String],
) -> Result<(), String> {
    for (i, att_id) in attestation_ids.iter().enumerate() {
        let uid =
            Uuid::parse_str(att_id).map_err(|e| format!("Invalid attestation UUID: {e}"))?;
        let single_eid = std::slice::from_ref(&endorsement_ids[i]);
        let single_ph = std::slice::from_ref(&proof_hashes[i]);

        match l2.submit_batch(single_eid, single_ph).await {
            Ok(tx_hash) => {
                // Wait for receipt
                match l2.wait_for_receipt(&tx_hash).await {
                    Ok((tx_hash_confirmed, block_number)) => {
                        let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
                        db.update_attestation_tx(&uid, &tx_hash_confirmed, block_number as i64)
                            .map_err(|e| {
                                format!("DB error updating attestation {att_id}: {e}")
                            })?;
                        tracing::info!(
                            "Single-item submit succeeded for attestation {} (endorsement {}) tx={}",
                            att_id,
                            pending[i].endorsement_id,
                            tx_hash_confirmed
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Could not get receipt for single-item tx {tx_hash} (attestation {}): {e} — leaving pending",
                            att_id
                        );
                        // Leave pending for retry next cycle
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("already attested") {
                    // This specific item is already on-chain; mark skipped
                    let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
                    db.mark_attestation_skipped(&uid).map_err(|e| {
                        format!("DB error marking attestation {att_id} skipped: {e}")
                    })?;
                    tracing::info!(
                        "Marked attestation {} as already_attested (endorsement {})",
                        att_id,
                        pending[i].endorsement_id
                    );
                } else {
                    // Some other error — leave pending for retry
                    tracing::warn!(
                        "Single-item submit failed for attestation {} (endorsement {}): {err_str} — leaving pending",
                        att_id,
                        pending[i].endorsement_id
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_to_bytes32_round_trip() {
        let original = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let b32 = uuid_to_bytes32(&original);
        let bytes = b32.as_slice();

        // First 16 bytes must be zero padding
        assert_eq!(&bytes[..16], &[0u8; 16], "left pad must be all zeros");

        // Last 16 bytes must match the UUID bytes
        assert_eq!(
            &bytes[16..],
            original.as_bytes(),
            "right half must equal UUID bytes"
        );

        // Round-trip: extract last 16 bytes and reconstruct UUID
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes[16..]);
        let reconstructed = Uuid::from_bytes(uuid_bytes);
        assert_eq!(reconstructed, original, "round-trip must produce the same UUID");
    }
}
