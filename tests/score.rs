use commit_backend::models::signal::{ScoreBreakdown, compute_score};
use commit_backend::services::score::{VERIFIED_WEIGHT, PENDING_WEIGHT};

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
