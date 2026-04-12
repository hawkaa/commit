use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::models::SubjectKind;

/// Maximum number of key hashes allowed per network query request.
const MAX_KEY_HASHES: usize = 200;

#[derive(Deserialize)]
pub struct NetworkQueryRequest {
    pub kind: String,
    pub id: String,
    pub key_hashes: Vec<String>,
}

#[derive(Serialize)]
pub struct NetworkQueryResponse {
    pub network_endorsement_count: u32,
    pub total_endorsement_count: u32,
}

#[allow(clippy::missing_errors_doc, clippy::unused_async)]
pub async fn network_query(
    State(state): State<AppState>,
    Json(req): Json<NetworkQueryRequest>,
) -> Result<Json<NetworkQueryResponse>, StatusCode> {
    let kind = SubjectKind::parse(&req.kind).ok_or(StatusCode::BAD_REQUEST)?;

    // Validate key_hashes: non-empty, within limit, each must be 64-char hex
    if req.key_hashes.is_empty() || req.key_hashes.len() > MAX_KEY_HASHES {
        return Err(StatusCode::BAD_REQUEST);
    }
    for kh in &req.key_hashes {
        if kh.len() != 64 || !kh.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let subject = db
        .find_subject(&kind, &req.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let total_endorsement_count = db
        .get_endorsement_count(&subject.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let network_endorsement_count = db
        .count_network_endorsements(&subject.id, &req.key_hashes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(NetworkQueryResponse {
        network_endorsement_count,
        total_endorsement_count,
    }))
}
