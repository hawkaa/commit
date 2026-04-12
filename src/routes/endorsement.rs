use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;
use crate::models::{EndorsementCategory, ProofType, SubjectKind};
use crate::services::db::map_db_error;
use crate::validation::{validate_transcript_subject, verify_attestation_signature};

#[derive(Deserialize)]
pub struct SubmitEndorsementRequest {
    pub subject_kind: String,
    pub subject_id: String,
    pub category: String,
    pub attestation: String,
    pub proof_type: String,
    pub transcript_sent: String,
    pub transcript_recv: Option<String>,
}

#[derive(Serialize)]
pub struct EndorsementResponse {
    pub id: String,
    pub status: String,
}

#[allow(clippy::missing_errors_doc, clippy::unused_async)]
pub async fn submit_endorsement(
    State(state): State<AppState>,
    Json(req): Json<SubmitEndorsementRequest>,
) -> Result<Json<EndorsementResponse>, StatusCode> {
    let kind = SubjectKind::parse(&req.subject_kind).ok_or(StatusCode::BAD_REQUEST)?;
    let category = EndorsementCategory::parse(&req.category).ok_or(StatusCode::BAD_REQUEST)?;
    let proof_type = ProofType::parse(&req.proof_type).ok_or(StatusCode::BAD_REQUEST)?;

    // Validate transcript matches claimed subject
    validate_transcript_subject(
        &req.transcript_sent,
        req.transcript_recv.as_deref(),
        &proof_type,
        &req.subject_id,
    )?;

    // Decode attestation and compute proof_hash server-side
    // Limit attestation size to prevent memory exhaustion (500KB decoded max)
    if req.attestation.len() > 1_000_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let attestation_bytes =
        hex::decode(&req.attestation).map_err(|_| StatusCode::BAD_REQUEST)?;
    if attestation_bytes.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Verify attestation signature when notary public key is configured
    if let Some(ref key) = state.notary_public_key {
        verify_attestation_signature(&attestation_bytes, key)?;
    }

    let proof_hash = Sha256::digest(&attestation_bytes).to_vec();

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify subject exists
    let subject = db
        .find_subject(&kind, &req.subject_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let endorsement_id = Uuid::new_v4();
    db.create_endorsement(
        &endorsement_id,
        &subject.id,
        category.as_str(),
        &proof_hash,
        proof_type.as_str(),
        Some(&attestation_bytes),
    )
    .map_err(map_db_error)?;

    // Create a pending attestation record (will be submitted on-chain in Phase 2)
    let attestation_id = Uuid::new_v4();
    db.create_attestation(&attestation_id, &endorsement_id, "pending")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(EndorsementResponse {
        id: endorsement_id.to_string(),
        status: "pending_attestation".to_string(),
    }))
}

#[allow(clippy::missing_errors_doc, clippy::unused_async)]
pub async fn get_endorsements(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<GetEndorsementsQuery>,
) -> Result<Json<Vec<EndorsementSummary>>, StatusCode> {
    let kind = SubjectKind::parse(&params.kind).ok_or(StatusCode::BAD_REQUEST)?;

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let subject = db
        .find_subject(&kind, &params.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let rows = db
        .get_endorsements_for_subject(&subject.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let summaries: Vec<EndorsementSummary> = rows
        .into_iter()
        .map(|r| EndorsementSummary {
            id: r.id,
            category: r.category,
            proof_type: r.proof_type,
            status: r.status,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(summaries))
}

#[derive(Deserialize)]
pub struct GetEndorsementsQuery {
    pub kind: String,
    pub id: String,
}

#[derive(Serialize)]
pub struct EndorsementSummary {
    pub id: String,
    pub category: String,
    pub proof_type: String,
    pub status: String,
    pub created_at: String,
}
