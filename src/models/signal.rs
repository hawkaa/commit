use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    Registry,
    ProfessionalBody,
    ZkEndorsement,
    ZkAggregate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalCategory {
    Longevity,
    Financial,
    Behavioral,
    Network,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    PublicApi,
    Scraped,
    ZkVerified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentSignal {
    pub source: SignalSource,
    pub category: SignalCategory,
    pub label: String,
    pub value: String,
    pub verification: VerificationLevel,
    pub timestamp: String,
    pub confidence: f64,
}

/// Commit Score (0-100), the brand primitive.
///
/// Phase 1 (Layer 1 only, normalized to 0-100):
/// - longevity:   `min(years_active * 3, 15)`
/// - maintenance: `min(commits_last_year / 10, 10)`
/// - community:   `min(contributors * 0.5, 10)`
/// - financial:   `positive_equity ? 5 : 0` (businesses only)
///
/// Phase 2+ (Layer 1 * 0.3 + Layer 2 * 0.7):
/// - endorsements:    `min(count * 5, 30)`
/// - network:         `min(unique_endorsers * 3, 15)`
/// - `proof_strength`: `avg(confidence) * 15`
/// - tenure:          `min(avg_endorser_months, 10)`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitScore {
    pub score: Option<u8>,
    pub breakdown: ScoreBreakdown,
    pub layer1_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub longevity: f64,
    pub maintenance: f64,
    pub community: f64,
    pub financial: f64,
    pub endorsements: f64,
    pub network_density: f64,
    pub proof_strength: f64,
    pub tenure: f64,
}

impl ScoreBreakdown {
    pub fn layer1_total(&self) -> f64 {
        self.longevity + self.maintenance + self.community + self.financial
    }

    pub fn layer2_total(&self) -> f64 {
        self.endorsements + self.network_density + self.proof_strength + self.tenure
    }
}

pub fn compute_score(breakdown: &ScoreBreakdown, has_layer2: bool) -> Option<u8> {
    let l1 = breakdown.layer1_total();
    if l1 == 0.0 {
        return None; // No data = no score, not 0
    }

    let raw = if has_layer2 {
        let l2 = breakdown.layer2_total();
        (l1 * 0.3) + (l2 * 0.7)
    } else {
        // Phase 1: normalize Layer 1 (max 40) to 0-100
        (l1 / 40.0) * 100.0
    };

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(raw.round().min(100.0) as u8)
}
