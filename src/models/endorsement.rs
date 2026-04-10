use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofType {
    Email,
    Payment,
    GitHistory,
    CiLogs,
    Visit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestationStatus {
    Verified,
    PendingAttestation,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub category: String,
    pub proof_hash: Vec<u8>,
    pub proof_type: ProofType,
    pub status: AttestationStatus,
    pub created_at: String,
}
