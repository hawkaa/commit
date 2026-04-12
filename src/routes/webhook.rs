use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AppState;
use crate::models::{EndorsementCategory, ProofType, SubjectKind};
use crate::services::db::map_db_error;
use crate::validation::{validate_transcript_subject, verify_attestation_signature};

/// Webhook payload from the TLSNotary verifier server.
/// Sent after successful MPC-TLS verification of an endorsement proof.
#[derive(Deserialize)]
pub struct VerifierWebhook {
    pub server_name: String,
    pub results: Vec<HandlerResult>,
    pub session: SessionInfo,
    pub transcript: RedactedTranscript,
    /// Raw attestation from TLSNotary (hex-encoded).
    pub attestation: String,
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
    pub sent: String,
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

    // Validate transcript matches claimed subject (unconditional — always required)
    validate_transcript_subject(&payload.transcript.sent, &proof_type, subject_id_str)?;

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

    // Decode attestation and compute proof_hash
    if payload.attestation.len() > 1_000_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let attestation_bytes =
        hex::decode(&payload.attestation).map_err(|_| StatusCode::BAD_REQUEST)?;
    if attestation_bytes.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Verify attestation signature when notary public key is configured
    if let Some(ref key) = state.notary_public_key {
        verify_attestation_signature(&attestation_bytes, key)?;
    }

    let proof_hash = Sha256::digest(&attestation_bytes).to_vec();
    let attestation_data = Some(attestation_bytes);

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
        attestation_data.as_deref(),
    )
    .map_err(map_db_error)?;

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

