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

impl ProofType {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "email" => Some(Self::Email),
            "payment" => Some(Self::Payment),
            "git_history" => Some(Self::GitHistory),
            "ci_logs" => Some(Self::CiLogs),
            "visit" => Some(Self::Visit),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Payment => "payment",
            Self::GitHistory => "git_history",
            Self::CiLogs => "ci_logs",
            Self::Visit => "visit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndorsementCategory {
    Usage,
    Contribution,
    Financial,
    Governance,
    Maintenance,
}

impl EndorsementCategory {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "usage" => Some(Self::Usage),
            "contribution" => Some(Self::Contribution),
            "financial" => Some(Self::Financial),
            "governance" => Some(Self::Governance),
            "maintenance" => Some(Self::Maintenance),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Contribution => "contribution",
            Self::Financial => "financial",
            Self::Governance => "governance",
            Self::Maintenance => "maintenance",
        }
    }
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

/// Lightweight summary for API responses (trust card, endorsement list).
#[derive(Serialize)]
pub struct EndorsementSummary {
    pub id: String,
    pub category: String,
    pub proof_type: String,
    pub status: String,
    pub created_at: String,
    pub sentiment: String,
    pub on_chain: bool,
    pub tx_hash: Option<String>,
}
