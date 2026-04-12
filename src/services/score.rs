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

#[must_use]
pub fn score_github_repo(repo: &GitHubRepo, contributor_count: usize) -> CommitScore {
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

    let breakdown = ScoreBreakdown {
        longevity: (years_active * 3.0).min(15.0),
        maintenance: maintenance_proxy,
        community,
        financial: 0.0,
        ..ScoreBreakdown::default()
    };

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
/// Layer 2 scoring activates when any non-failed endorsements exist.
#[must_use]
pub fn score_github_repo_with_endorsements(
    repo: &GitHubRepo,
    contributor_count: usize,
    verified_count: u32,
    pending_count: u32,
) -> CommitScore {
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

    let has_layer2 = verified_count > 0 || pending_count > 0;

    #[allow(clippy::cast_precision_loss)]
    let weighted_sum =
        f64::from(verified_count) * VERIFIED_WEIGHT + f64::from(pending_count) * PENDING_WEIGHT;

    let endorsements = (weighted_sum * 5.0).min(30.0);

    #[allow(clippy::cast_precision_loss)]
    let total = f64::from(verified_count + pending_count);
    let proof_strength = if total > 0.0 {
        (weighted_sum / total) * 15.0
    } else {
        0.0
    };

    let breakdown = ScoreBreakdown {
        longevity: (years_active * 3.0).min(15.0),
        maintenance: maintenance_proxy,
        community,
        financial: 0.0,
        endorsements,
        proof_strength,
        ..ScoreBreakdown::default()
    };

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
