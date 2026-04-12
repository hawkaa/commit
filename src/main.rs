use std::sync::Mutex;

use axum::{
    Router,
    routing::{get, post},
};
use commit_backend::services::{db::Database, github::GitHubClient};
use commit_backend::{AppState, routes};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("commit_backend=info".parse().unwrap()),
        )
        .json()
        .init();

    let github_token = std::env::var("GITHUB_TOKEN").ok();
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "commit.db".to_string());
    let notary_public_key = match std::env::var("NOTARY_PUBLIC_KEY") {
        Ok(pem) => {
            use k256::pkcs8::DecodePublicKey;
            let key = k256::ecdsa::VerifyingKey::from_public_key_pem(&pem)
                .expect("NOTARY_PUBLIC_KEY contains invalid PEM — cannot start");
            let sec1_bytes = key.to_encoded_point(true);
            let bytes = sec1_bytes.as_bytes();
            let fingerprint = hex::encode(&bytes[bytes.len().saturating_sub(6)..]);
            tracing::info!("Notary public key loaded (tail: ...{fingerprint})");
            Some(key)
        }
        Err(_) => {
            tracing::warn!(
                "NOTARY_PUBLIC_KEY not set — attestation signature verification unavailable"
            );
            None
        }
    };

    let db = Database::open(&db_path).expect("Failed to open database");
    let github = GitHubClient::new(github_token);

    let state = AppState {
        db: std::sync::Arc::new(Mutex::new(db)),
        github: std::sync::Arc::new(github),
        notary_public_key,
    };

    let app = Router::new()
        .route("/trust-card", get(routes::trust_card::get_trust_card))
        .route(
            "/trust/{kind}/{*id}",
            get(routes::trust_page::get_trust_page),
        )
        .route("/badge/{kind}/{*id}", get(routes::badge::get_badge))
        .route(
            "/endorsements",
            post(routes::endorsement::submit_endorsement)
                .get(routes::endorsement::get_endorsements),
        )
        .route("/privacy", get(routes::privacy::get_privacy_page))
        .route(
            "/webhook/endorsement",
            post(routes::webhook::receive_endorsement_webhook),
        )
        .route(
            "/network-query",
            post(routes::network::network_query),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = "0.0.0.0:3000";
    tracing::info!("Commit backend listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
