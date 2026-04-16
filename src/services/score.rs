use chrono::Utc;

use crate::models::signal::{
    CommitScore, CommitmentSignal, ScoreBreakdown, SignalCategory, SignalSource, VerificationLevel,
    compute_score,
};
use crate::services::github::GitHubRepo;

/// Weight for verified endorsements in score calculation.
pub const VERIFIED_WEIGHT: f64 = 1.0;
/// Weight for pending_attestation endorsements (lower trust).
pub const PENDING_WEIGHT: f64 = 0.3;
/// Weight for negative endorsements (symmetric with positive).
pub const NEGATIVE_WEIGHT: f64 = -1.0;

/// Compute the Layer 1 breakdown from GitHub repo signals.
fn layer1_breakdown(repo: &GitHubRepo, contributor_count: usize) -> ScoreBreakdown {
    let years_active = years_since(&repo.created_at);
    let days_since_push = days_since(&repo.pushed_at);
    let maintenance_proxy = if days_since_push < 30.0 {
        10.0
    } else if days_since_push < 180.0 {
        5.0
    } else {
        1.0
    };

    #[allow(clippy::cast_precision_loss)]
    let community = (contributor_count as f64 * 0.5).min(10.0);

    ScoreBreakdown {
        longevity: (years_active * 3.0).min(15.0),
        maintenance: maintenance_proxy,
        community,
        financial: 0.0,
        ..ScoreBreakdown::default()
    }
}

#[must_use]
pub fn score_github_repo(repo: &GitHubRepo, contributor_count: usize) -> CommitScore {
    let breakdown = layer1_breakdown(repo, contributor_count);
    let score = compute_score(&breakdown, false);

    CommitScore {
        score,
        breakdown,
        layer1_only: true,
    }
}

/// Compute a commit score that factors in endorsement status weighting.
///
/// Verified endorsements count fully, pending ones are down-weighted.
/// Negative endorsements subtract from the weighted sum (floored at 0).
/// Layer 2 scoring activates when any non-failed endorsements exist.
///
/// `avg_tenure_months` is the average age of endorsements for this subject.
/// `unique_endorser_count` is the number of unique endorsers (polarity-agnostic).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn score_github_repo_with_endorsements(
    repo: &GitHubRepo,
    contributor_count: usize,
    positive_verified: u32,
    positive_pending: u32,
    negative_verified: u32,
    negative_pending: u32,
    avg_tenure_months: f64,
    unique_endorser_count: u32,
) -> CommitScore {
    let mut breakdown = layer1_breakdown(repo, contributor_count);

    let total_positive = positive_verified + positive_pending;
    let total_negative = negative_verified + negative_pending;
    let has_layer2 = total_positive > 0 || total_negative > 0;

    // Weighted sum: positive contributes positively, negative subtracts.
    // Pending-discount applies to both polarities.
    #[allow(clippy::cast_precision_loss)]
    let weighted_sum = f64::from(positive_verified) * VERIFIED_WEIGHT
        + f64::from(positive_pending) * PENDING_WEIGHT
        + f64::from(negative_verified) * NEGATIVE_WEIGHT
        + f64::from(negative_pending) * NEGATIVE_WEIGHT * PENDING_WEIGHT.abs();

    // Floor at 0: negative endorsements can zero out the component but not make it negative.
    breakdown.endorsements = weighted_sum.max(0.0).mul_add(5.0, 0.0).min(30.0);

    // proof_strength and tenure use absolute counts (the proof itself is what they measure,
    // regardless of sentiment direction).
    #[allow(clippy::cast_precision_loss)]
    let total = f64::from(total_positive + total_negative);
    let abs_weighted = f64::from(positive_verified) * VERIFIED_WEIGHT
        + f64::from(positive_pending) * PENDING_WEIGHT
        + f64::from(negative_verified) * VERIFIED_WEIGHT
        + f64::from(negative_pending) * PENDING_WEIGHT;
    breakdown.proof_strength = if total > 0.0 {
        (abs_weighted / total) * 15.0
    } else {
        0.0
    };

    breakdown.tenure = avg_tenure_months.min(10.0);
    // network_density counts unique endorsers regardless of polarity
    breakdown.network_density = (f64::from(unique_endorser_count) * 3.0).min(15.0);

    let score = compute_score(&breakdown, has_layer2);

    CommitScore {
        score,
        breakdown,
        layer1_only: !has_layer2,
    }
}

#[must_use]
pub fn build_signals(repo: &GitHubRepo, contributor_count: usize) -> Vec<CommitmentSignal> {
    let now = Utc::now().to_rfc3339();
    let years_active = years_since(&repo.created_at);
    let days_since_push = days_since(&repo.pushed_at);

    let mut signals = Vec::new();

    signals.push(CommitmentSignal {
        source: SignalSource::Registry,
        category: SignalCategory::Longevity,
        label: "Age".to_string(),
        value: format!("{years_active:.1} years"),
        verification: VerificationLevel::PublicApi,
        timestamp: now.clone(),
        confidence: 1.0,
    });

    let maintenance_label = if days_since_push < 30.0 {
        "Active"
    } else if days_since_push < 180.0 {
        "Maintained"
    } else {
        "Inactive"
    };
    signals.push(CommitmentSignal {
        source: SignalSource::Registry,
        category: SignalCategory::Behavioral,
        label: "Maintenance".to_string(),
        value: format!("{maintenance_label} (pushed {days_since_push:.0}d ago)"),
        verification: VerificationLevel::PublicApi,
        timestamp: now.clone(),
        confidence: 0.8,
    });

    signals.push(CommitmentSignal {
        source: SignalSource::Registry,
        category: SignalCategory::Network,
        label: "Contributors".to_string(),
        value: format!("{contributor_count}"),
        verification: VerificationLevel::PublicApi,
        timestamp: now.clone(),
        confidence: 0.9,
    });

    signals.push(CommitmentSignal {
        source: SignalSource::Registry,
        category: SignalCategory::Network,
        label: "Stars".to_string(),
        value: format!("{}", repo.stargazers_count),
        verification: VerificationLevel::PublicApi,
        timestamp: now.clone(),
        confidence: 1.0,
    });

    signals.push(CommitmentSignal {
        source: SignalSource::Registry,
        category: SignalCategory::Network,
        label: "Forks".to_string(),
        value: format!("{}", repo.forks_count),
        verification: VerificationLevel::PublicApi,
        timestamp: now,
        confidence: 1.0,
    });

    signals
}

fn parse_iso_date(iso_date: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(iso_date)
        .or_else(|_| {
            // GitHub returns "2016-08-01T19:28:17Z" which is valid RFC 3339.
            // For dates without timezone suffix, try appending +00:00.
            let stripped = iso_date.trim_end_matches('Z');
            chrono::DateTime::parse_from_rfc3339(&format!("{stripped}+00:00"))
        })
        .ok()
}

#[allow(clippy::cast_precision_loss)]
fn years_since(iso_date: &str) -> f64 {
    parse_iso_date(iso_date).map_or(0.0, |dt| {
        let days = Utc::now().signed_duration_since(dt).num_days();
        days as f64 / 365.25
    })
}

#[allow(clippy::cast_precision_loss)]
fn days_since(iso_date: &str) -> f64 {
    parse_iso_date(iso_date).map_or(999.0, |dt| {
        Utc::now().signed_duration_since(dt).num_days() as f64
    })
}
