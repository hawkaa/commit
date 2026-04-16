use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::models::{
    CommitScore, CommitmentSignal, EndorsementSummary, ScoreBreakdown, Subject, SubjectKind,
};
use crate::services::db::EndorsementRow;
use crate::services::score::{
    build_signals, score_github_repo, score_github_repo_with_endorsements,
};

#[derive(Deserialize)]
pub struct TrustCardQuery {
    pub kind: String,
    pub id: String,
}

#[derive(Serialize)]
pub struct TrustCardResponse {
    pub subject: Subject,
    pub signals: Vec<CommitmentSignal>,
    pub score: CommitScore,
    pub endorsement_count: u32,
    pub recent_endorsements: Vec<EndorsementSummary>,
}

fn map_endorsement_rows(
    rows: Vec<EndorsementRow>,
    db: &crate::services::db::Database,
) -> Vec<EndorsementSummary> {
    let endorsement_ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let attestation_map = db
        .get_attestations_for_endorsements(&endorsement_ids)
        .unwrap_or_default();

    rows.into_iter()
        .map(|r| {
            let (on_chain, tx_hash) = attestation_map.get(&r.id).map_or((false, None), |att| {
                if att.tx_hash.is_some() {
                    (true, att.tx_hash.clone())
                } else {
                    (false, None)
                }
            });
            EndorsementSummary {
                id: r.id,
                category: r.category,
                proof_type: r.proof_type,
                status: r.status,
                sentiment: r.sentiment,
                created_at: r.created_at,
                on_chain,
                tx_hash,
            }
        })
        .collect()
}

#[allow(clippy::missing_errors_doc)] // Axum handler
pub async fn get_trust_card(
    State(state): State<AppState>,
    Query(query): Query<TrustCardQuery>,
) -> Result<Json<TrustCardResponse>, StatusCode> {
    let kind = SubjectKind::parse(&query.kind).ok_or(StatusCode::BAD_REQUEST)?;

    match kind {
        SubjectKind::GithubRepo => get_github_trust_card(&state, &query.id).await,
        _ => Err(StatusCode::NOT_IMPLEMENTED),
    }
}

async fn get_github_trust_card(
    state: &AppState,
    identifier: &str,
) -> Result<Json<TrustCardResponse>, StatusCode> {
    let parts: Vec<&str> = identifier.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (owner, repo_name) = (parts[0], parts[1]);

    // Check cache first
    {
        let db = state
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Ok(Some(subject)) = db.find_subject(&SubjectKind::GithubRepo, identifier)
            && let Ok(Some((signals_json, score_json))) = db.get_cached_signals(&subject.id)
        {
            let signals: Vec<CommitmentSignal> =
                serde_json::from_str(&signals_json).unwrap_or_default();
            let score: CommitScore = serde_json::from_str(&score_json).unwrap_or(CommitScore {
                score: None,
                breakdown: ScoreBreakdown::default(),
                layer1_only: true,
            });
            let endorsement_count = db.get_endorsement_count(&subject.id).unwrap_or(0);
            let recent_endorsements = map_endorsement_rows(
                db.get_recent_endorsements(&subject.id, 5)
                    .unwrap_or_default(),
                &db,
            );
            return Ok(Json(TrustCardResponse {
                subject,
                signals,
                score,
                endorsement_count,
                recent_endorsements,
            }));
        }
    }

    // Cache miss: fetch from GitHub
    let gh_repo = state.github.get_repo(owner, repo_name).await.map_err(|e| {
        tracing::error!("GitHub API error for {owner}/{repo_name}: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    let contributor_count = state
        .github
        .get_contributor_count(owner, repo_name)
        .await
        .unwrap_or(0);

    let signals = build_signals(&gh_repo, contributor_count);

    let candidate = Subject {
        id: Uuid::new_v4(),
        kind: SubjectKind::GithubRepo,
        identifier: identifier.to_string(),
        display_name: gh_repo.full_name.clone(),
        endorsement_count: 0,
    };

    // Cache the result — read back the actual stored subject to get the
    // canonical UUID (upsert keeps the original ID on conflict).
    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = db.upsert_subject(&candidate);
    let subject = db
        .find_subject(&SubjectKind::GithubRepo, identifier)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Compute score with endorsement status + sentiment weighting
    let (pv, pp, nv, np) = db
        .get_endorsement_counts_by_status_and_sentiment(&subject.id)
        .unwrap_or((0, 0, 0, 0));
    let score = if pv > 0 || pp > 0 || nv > 0 || np > 0 {
        let avg_tenure_months = db.get_endorsement_tenure_months(&subject.id).unwrap_or(0.0);
        let unique_endorser_count = db.get_unique_endorser_count(&subject.id).unwrap_or(0);
        score_github_repo_with_endorsements(
            &gh_repo,
            contributor_count,
            pv,
            pp,
            nv,
            np,
            avg_tenure_months,
            unique_endorser_count,
        )
    } else {
        score_github_repo(&gh_repo, contributor_count)
    };

    let _ = db.cache_signals(
        &subject.id,
        &serde_json::to_string(&signals).unwrap_or_default(),
        &serde_json::to_string(&score).unwrap_or_default(),
    );
    let endorsement_count = db.get_endorsement_count(&subject.id).unwrap_or(0);
    let recent_endorsements = map_endorsement_rows(
        db.get_recent_endorsements(&subject.id, 5)
            .unwrap_or_default(),
        &db,
    );

    Ok(Json(TrustCardResponse {
        subject,
        signals,
        score,
        endorsement_count,
        recent_endorsements,
    }))
}
