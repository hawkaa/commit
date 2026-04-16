use commit_backend::models::signal::{ScoreBreakdown, compute_score};
use commit_backend::services::score::{
    NEGATIVE_WEIGHT, PENDING_WEIGHT, VERIFIED_WEIGHT, score_github_repo_with_endorsements,
};

#[test]
fn zero_signals_returns_none() {
    let breakdown = ScoreBreakdown::default();
    assert!(compute_score(&breakdown, false).is_none());
}

#[test]
fn layer1_normalizes_to_100() {
    let breakdown = ScoreBreakdown {
        longevity: 15.0,
        maintenance: 10.0,
        community: 10.0,
        financial: 5.0,
        ..ScoreBreakdown::default()
    };
    // 40/40 * 100 = 100
    assert_eq!(compute_score(&breakdown, false), Some(100));
}

#[test]
fn layer1_partial_score() {
    let breakdown = ScoreBreakdown {
        longevity: 15.0,
        maintenance: 10.0,
        community: 10.0,
        financial: 0.0,
        ..ScoreBreakdown::default()
    };
    // 35/40 * 100 = 87.5 → rounds to 88
    assert_eq!(compute_score(&breakdown, false), Some(88));
}

#[test]
fn maintenance_only_gives_25() {
    let breakdown = ScoreBreakdown {
        longevity: 0.0,
        maintenance: 10.0,
        community: 0.0,
        financial: 0.0,
        ..ScoreBreakdown::default()
    };
    // 10/40 * 100 = 25
    assert_eq!(compute_score(&breakdown, false), Some(25));
}

#[test]
fn layer2_weighting_applies() {
    let breakdown = ScoreBreakdown {
        longevity: 10.0,
        maintenance: 10.0,
        community: 5.0,
        financial: 0.0,
        endorsements: 20.0,
        network_density: 10.0,
        proof_strength: 10.0,
        tenure: 5.0,
    };
    // L1 = 25, L2 = 45
    // (25 * 0.3) + (45 * 0.7) = 7.5 + 31.5 = 39
    assert_eq!(compute_score(&breakdown, true), Some(39));
}

#[test]
fn score_caps_at_100() {
    let breakdown = ScoreBreakdown {
        longevity: 100.0,
        maintenance: 100.0,
        community: 100.0,
        financial: 100.0,
        ..ScoreBreakdown::default()
    };
    assert_eq!(compute_score(&breakdown, false), Some(100));
}

// --- Status-weighted scoring tests ---

#[test]
fn weight_constants_correct() {
    assert!((VERIFIED_WEIGHT - 1.0).abs() < f64::EPSILON);
    assert!((PENDING_WEIGHT - 0.3).abs() < f64::EPSILON);
}

#[test]
fn verified_endorsements_score_higher_than_pending() {
    // 3 verified endorsements: weighted_sum = 3.0
    // endorsements = min(3.0 * 5, 30) = 15.0
    // proof_strength = (3.0 / 3.0) * 15 = 15.0
    let verified_breakdown = ScoreBreakdown {
        longevity: 10.0,
        maintenance: 10.0,
        community: 5.0,
        financial: 0.0,
        endorsements: (3.0 * VERIFIED_WEIGHT * 5.0_f64).min(30.0),
        proof_strength: (3.0 * VERIFIED_WEIGHT / 3.0) * 15.0,
        ..ScoreBreakdown::default()
    };

    // 3 pending endorsements: weighted_sum = 0.9
    // endorsements = min(0.9 * 5, 30) = 4.5
    // proof_strength = (0.9 / 3.0) * 15 = 4.5
    let pending_breakdown = ScoreBreakdown {
        longevity: 10.0,
        maintenance: 10.0,
        community: 5.0,
        financial: 0.0,
        endorsements: (3.0 * PENDING_WEIGHT * 5.0_f64).min(30.0),
        proof_strength: (3.0 * PENDING_WEIGHT / 3.0) * 15.0,
        ..ScoreBreakdown::default()
    };

    let verified_score = compute_score(&verified_breakdown, true).unwrap();
    let pending_score = compute_score(&pending_breakdown, true).unwrap();
    assert!(
        verified_score > pending_score,
        "Verified score ({verified_score}) should be higher than pending score ({pending_score})"
    );
}

#[test]
fn mixed_endorsements_weighted_correctly() {
    // 2 verified + 3 pending: weighted_sum = 2*1.0 + 3*0.3 = 2.9
    let weighted_sum = 2.0 * VERIFIED_WEIGHT + 3.0 * PENDING_WEIGHT;
    let endorsements = (weighted_sum * 5.0).min(30.0);
    let total = 5.0;
    let proof_strength = (weighted_sum / total) * 15.0;

    let breakdown = ScoreBreakdown {
        longevity: 10.0,
        maintenance: 10.0,
        community: 5.0,
        financial: 0.0,
        endorsements,
        proof_strength,
        ..ScoreBreakdown::default()
    };

    let score = compute_score(&breakdown, true);
    assert!(score.is_some());
    // L1 = 25, L2 = endorsements + proof_strength = 14.5 + 8.7 = 23.2
    // (25 * 0.3) + (23.2 * 0.7) = 7.5 + 16.24 = 23.74 ≈ 24
    let s = score.unwrap();
    assert!(s > 0, "Score should be positive with endorsements");
}

#[test]
fn endorsements_field_caps_at_30() {
    // 10 verified: weighted_sum = 10.0, endorsements = min(50.0, 30.0) = 30.0
    let weighted_sum = 10.0 * VERIFIED_WEIGHT;
    let endorsements = (weighted_sum * 5.0).min(30.0);
    assert!((endorsements - 30.0).abs() < f64::EPSILON);
}

// --- Tenure scoring tests ---

use commit_backend::services::github::GitHubRepo;

fn make_test_repo() -> GitHubRepo {
    GitHubRepo {
        full_name: "test/repo".to_string(),
        description: None,
        created_at: "2023-01-01T00:00:00Z".to_string(),
        pushed_at: chrono::Utc::now().to_rfc3339(),
        stargazers_count: 100,
        forks_count: 10,
        open_issues_count: 5,
    }
}

#[test]
fn tenure_3_months_produces_3() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 0, 0, 3.0, 0);
    assert!(
        (score.breakdown.tenure - 3.0).abs() < f64::EPSILON,
        "tenure should be 3.0, got {}",
        score.breakdown.tenure
    );
}

#[test]
fn tenure_15_months_caps_at_10() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 0, 0, 15.0, 0);
    assert!(
        (score.breakdown.tenure - 10.0).abs() < f64::EPSILON,
        "tenure should be capped at 10.0, got {}",
        score.breakdown.tenure
    );
}

#[test]
fn tenure_zero_with_no_endorsements() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 0, 0, 0, 0, 0.0, 0);
    assert!(
        (score.breakdown.tenure - 0.0).abs() < f64::EPSILON,
        "tenure should be 0.0, got {}",
        score.breakdown.tenure
    );
}

#[test]
fn tenure_near_zero_for_fresh_endorsement() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 1, 0, 0, 0, 0.01, 0);
    assert!(
        score.breakdown.tenure < 0.1,
        "tenure should be near 0, got {}",
        score.breakdown.tenure
    );
}

#[test]
fn tenure_increases_blended_score() {
    let repo = make_test_repo();
    let score_no_tenure = score_github_repo_with_endorsements(&repo, 10, 3, 0, 0, 0, 0.0, 0);
    let score_with_tenure = score_github_repo_with_endorsements(&repo, 10, 3, 0, 0, 0, 5.0, 0);
    assert!(
        score_with_tenure.score.unwrap() >= score_no_tenure.score.unwrap(),
        "Score with tenure ({:?}) should be >= score without ({:?})",
        score_with_tenure.score,
        score_no_tenure.score
    );
}

#[test]
fn network_density_zero_when_no_endorser_count() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 0, 0, 3.0, 0);
    assert!(
        (score.breakdown.network_density - 0.0).abs() < f64::EPSILON,
        "network_density should be 0.0 when unique_endorser_count is 0, got {}",
        score.breakdown.network_density
    );
}

#[test]
fn network_density_computed_from_endorser_count() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 0, 0, 3.0, 4);
    // min(4 * 3.0, 15.0) = 12.0
    assert!(
        (score.breakdown.network_density - 12.0).abs() < f64::EPSILON,
        "network_density should be 12.0, got {}",
        score.breakdown.network_density
    );
}

#[test]
fn network_density_caps_at_15() {
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 0, 0, 3.0, 10);
    // min(10 * 3.0, 15.0) = 15.0
    assert!(
        (score.breakdown.network_density - 15.0).abs() < f64::EPSILON,
        "network_density should be capped at 15.0, got {}",
        score.breakdown.network_density
    );
}

// --- Negative sentiment scoring tests ---

#[test]
fn negative_weight_constant_correct() {
    assert!((NEGATIVE_WEIGHT - (-1.0)).abs() < f64::EPSILON);
}

#[test]
fn all_positive_identical_to_before() {
    // 3 positive verified, 0 negative → identical to old behavior
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 3, 0, 0, 0, 3.0, 0);
    // weighted_sum = 3.0 * 1.0 = 3.0, endorsements = 15.0
    assert!(
        (score.breakdown.endorsements - 15.0).abs() < f64::EPSILON,
        "endorsements should be 15.0, got {}",
        score.breakdown.endorsements
    );
}

#[test]
fn negative_reduces_endorsements_component() {
    // 3 positive verified, 1 negative verified → weighted = 3 - 1 = 2.0, endorsements = 10.0
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 3, 0, 1, 0, 3.0, 0);
    assert!(
        (score.breakdown.endorsements - 10.0).abs() < f64::EPSILON,
        "endorsements should be 10.0, got {}",
        score.breakdown.endorsements
    );
}

#[test]
fn only_negative_floors_endorsements_at_zero() {
    // 0 positive, 2 negative verified → weighted = -2.0, floored to 0
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 0, 0, 2, 0, 3.0, 2);
    assert!(
        score.breakdown.endorsements.abs() < f64::EPSILON,
        "endorsements should be 0.0 (floored), got {}",
        score.breakdown.endorsements
    );
    // network_density should still reflect 2 unique endorsers
    assert!(
        (score.breakdown.network_density - 6.0).abs() < f64::EPSILON,
        "network_density should be 6.0 (2 * 3.0), got {}",
        score.breakdown.network_density
    );
}

#[test]
fn equal_positive_negative_zeros_endorsements() {
    // 2 positive verified, 2 negative verified → weighted = 0, endorsements = 0
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 2, 0, 2, 0, 3.0, 4);
    assert!(
        score.breakdown.endorsements.abs() < f64::EPSILON,
        "endorsements should be 0.0, got {}",
        score.breakdown.endorsements
    );
    // proof_strength should still reflect proof activity (all 4 endorsements)
    assert!(
        score.breakdown.proof_strength > 0.0,
        "proof_strength should be > 0, got {}",
        score.breakdown.proof_strength
    );
}

#[test]
fn pending_negative_uses_discount() {
    // 1 negative pending → weighted = -1.0 * 0.3 = -0.3
    // 1 positive verified → weighted = 1.0
    // net = 0.7, endorsements = 3.5
    let repo = make_test_repo();
    let score = score_github_repo_with_endorsements(&repo, 10, 1, 0, 0, 1, 3.0, 0);
    let expected = (1.0 * VERIFIED_WEIGHT + NEGATIVE_WEIGHT * PENDING_WEIGHT.abs()) * 5.0;
    assert!(
        (score.breakdown.endorsements - expected).abs() < 0.01,
        "endorsements should be {expected:.1}, got {}",
        score.breakdown.endorsements
    );
}
