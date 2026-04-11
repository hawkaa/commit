use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AppState;
use crate::models::{EndorsementCategory, ProofType, SubjectKind};

/// Webhook payload from the TLSNotary verifier server.
/// Sent after successful MPC-TLS verification of an endorsement proof.
#[derive(Deserialize)]
pub struct VerifierWebhook {
    pub server_name: String,
    pub results: Vec<HandlerResult>,
    pub session: SessionInfo,
    pub transcript: Option<RedactedTranscript>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct HandlerResult {
    #[serde(rename = "type")]
    pub handler_type: String,
    pub part: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(flatten)]
    pub data: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct RedactedTranscript {
    pub sent: Option<String>,
    pub recv: Option<String>,
}

#[derive(Serialize)]
pub struct WebhookResponse {
    pub endorsement_id: String,
    pub status: String,
}

/// Receives verified endorsement data from the TLSNotary verifier server.
///
/// The verifier has already cryptographically verified the MPC-TLS proof.
/// We trust the webhook because it's authenticated with a shared secret
/// and the verifier runs on our infrastructure.
#[allow(clippy::missing_errors_doc, clippy::unused_async)]
pub async fn receive_endorsement_webhook(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<VerifierWebhook>,
) -> Result<Json<WebhookResponse>, StatusCode> {
    // Authenticate webhook with shared secret (fail closed)
    let expected_token = std::env::var("VERIFIER_WEBHOOK_SECRET")
        .map_err(|_| {
            tracing::error!("VERIFIER_WEBHOOK_SECRET not set — rejecting webhook");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = format!("Bearer {expected_token}");
    // Constant-time comparison to prevent timing side-channel on the secret
    if !constant_time_eq(auth.as_bytes(), expected.as_bytes()) {
        tracing::warn!("Webhook auth failed from verifier");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Extract subject info from session_data (set by the extension during registration)
    let subject_kind_str = payload
        .session
        .data
        .get("subject_kind")
        .ok_or(StatusCode::BAD_REQUEST)?;
    let subject_id_str = payload
        .session
        .data
        .get("subject_id")
        .ok_or(StatusCode::BAD_REQUEST)?;
    let category_str = payload
        .session
        .data
        .get("category")
        .map_or("usage", |s| s.as_str());
    let category = EndorsementCategory::parse(category_str).ok_or(StatusCode::BAD_REQUEST)?;
    let proof_type_str = payload
        .session
        .data
        .get("proof_type")
        .map_or("git_history", |s| s.as_str());
    let proof_type = ProofType::parse(proof_type_str).ok_or(StatusCode::BAD_REQUEST)?;

    let kind = SubjectKind::parse(subject_kind_str).ok_or(StatusCode::BAD_REQUEST)?;

    // Validate the server_name matches expected target for proof type
    let valid_server = match proof_type_str {
        "git_history" | "ci_logs" => payload.server_name == "api.github.com",
        "email" => payload.server_name.ends_with(".google.com") || payload.server_name.ends_with(".outlook.com"),
        _ => false,
    };
    if !valid_server {
        tracing::warn!(
            "Server name mismatch: got {} for proof type {}",
            payload.server_name,
            proof_type_str
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Hash the verified results to produce a proof_hash
    let proof_hash = hash_verification_results(&payload);

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find or create the subject (upsert so webhooks work for repos not yet viewed)
    let candidate = crate::models::Subject {
        id: Uuid::new_v4(),
        kind: kind.clone(),
        identifier: subject_id_str.to_string(),
        display_name: subject_id_str.to_string(),
        endorsement_count: 0,
    };
    let _ = db.upsert_subject(&candidate);
    let subject = db
        .find_subject(&kind, subject_id_str)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create the endorsement with verified status
    let endorsement_id = Uuid::new_v4();
    db.create_endorsement(
        &endorsement_id,
        &subject.id,
        category.as_str(),
        &proof_hash,
        proof_type.as_str(),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Mark as verified (proof already confirmed by verifier)
    db.update_endorsement_status(&endorsement_id, "verified")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Create attestation record (pending L2 submission)
    let attestation_id = Uuid::new_v4();
    db.create_attestation(&attestation_id, &endorsement_id, "base_sepolia")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!(
        "Verified endorsement created: {} for {}/{}",
        endorsement_id,
        subject_kind_str,
        subject_id_str
    );

    Ok(Json(WebhookResponse {
        endorsement_id: endorsement_id.to_string(),
        status: "verified".to_string(),
    }))
}

/// Constant-time byte comparison to prevent timing side-channel on webhook secret.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Hash the verification results to produce a deterministic proof hash.
fn hash_verification_results(payload: &VerifierWebhook) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(payload.server_name.as_bytes());
    hasher.update(payload.session.id.as_bytes());
    for result in &payload.results {
        hasher.update(result.handler_type.as_bytes());
        hasher.update(result.part.as_bytes());
        hasher.update(result.value.as_bytes());
    }
    hasher.finalize().to_vec()
}
