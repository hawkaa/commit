use commit_backend::models::signal::{ScoreBreakdown, compute_score};

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
