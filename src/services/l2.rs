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
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
    ) -> Result<String, Box<dyn std::error::Error>> {
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
    ) -> Result<(String, u64), Box<dyn std::error::Error>> {
        let hash: FixedBytes<32> = tx_hash.parse()?;
        let receipt = self
            .provider
            .get_transaction_receipt(hash)
            .await?
            .ok_or("transaction receipt not found")?;

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
pub async fn run_batch_submitter(
    db: Arc<Mutex<Database>>,
    l2: L2Client,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
    // Skip the first immediate tick — let the server finish starting
    interval.tick().await;

    loop {
        interval.tick().await;
        if let Err(e) = process_pending_batch(&db, &l2).await {
            tracing::error!("L2 batch submission error: {e}");
        }
    }
}

async fn process_pending_batch(
    db: &Arc<Mutex<Database>>,
    l2: &L2Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let pending = {
        let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
        db.get_pending_attestations(BATCH_SIZE as u32)?
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
            // If the contract reverts with "already attested", mark all as complete
            if err_str.contains("already attested") {
                tracing::warn!("Contract reports 'already attested' — marking batch as complete");
                let db = db.lock().map_err(|e| format!("DB lock error: {e}"))?;
                for (i, att_id) in attestation_ids.iter().enumerate() {
                    let uid = Uuid::parse_str(att_id)?;
                    db.update_attestation_tx(&uid, "already_attested", 0)?;
                    tracing::info!(
                        "Marked attestation {} as already attested (endorsement {})",
                        att_id,
                        pending[i].endorsement_id
                    );
                }
                return Ok(());
            }
            return Err(e);
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
        let uid = Uuid::parse_str(att_id)?;
        db.update_attestation_tx(&uid, &tx_hash_confirmed, block_number as i64)?;
    }

    tracing::info!(
        "Updated {} attestations with tx {} at block {}",
        attestation_ids.len(),
        tx_hash_confirmed,
        block_number
    );

    Ok(())
}
