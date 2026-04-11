use axum::{
    Router,
    routing::{get, post},
};
use axum_test::TestServer;
use commit_backend::{
    AppState,
    services::{db::Database, github::GitHubClient},
};
use std::sync::{Arc, Mutex};

fn test_app() -> TestServer {
    let db = Database::open(":memory:").expect("in-memory db");
    let github = GitHubClient::new(None);
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        github: Arc::new(github),
    };
    let app = Router::new()
        .route(
            "/trust-card",
            get(commit_backend::routes::trust_card::get_trust_card),
        )
        .route(
            "/trust/{kind}/{*id}",
            get(commit_backend::routes::trust_page::get_trust_page),
        )
        .route(
            "/badge/{kind}/{*id}",
            get(commit_backend::routes::badge::get_badge),
        )
        .route(
            "/endorsements",
            post(commit_backend::routes::endorsement::submit_endorsement)
                .get(commit_backend::routes::endorsement::get_endorsements),
        )
        .route(
            "/privacy",
            get(commit_backend::routes::privacy::get_privacy_page),
        )
        .route(
            "/webhook/endorsement",
            post(commit_backend::routes::webhook::receive_endorsement_webhook),
        )
        .with_state(state);
    TestServer::new(app)
}

#[tokio::test]
async fn trust_card_invalid_kind_returns_400() {
    let server = test_app();
    let resp = server.get("/trust-card?kind=bogus&id=test").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn trust_card_missing_params_returns_400() {
    let server = test_app();
    let resp = server.get("/trust-card?kind=github").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn trust_card_no_slash_returns_400() {
    let server = test_app();
    let resp = server.get("/trust-card?kind=github&id=noslash").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn trust_card_unimplemented_kind_returns_501() {
    let server = test_app();
    let resp = server.get("/trust-card?kind=npm&id=react").await;
    resp.assert_status(axum::http::StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn badge_unknown_repo_returns_200_with_dash() {
    let server = test_app();
    let resp = server.get("/badge/github/nonexistent/repo.svg").await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("image/svg+xml") || body.contains("<svg"));
    assert!(body.contains("\u{2014}") || body.contains("#9ca3af"));
}

#[tokio::test]
async fn badge_returns_svg_content_type() {
    let server = test_app();
    let resp = server.get("/badge/github/any/repo.svg").await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.starts_with("<svg"));
}

#[tokio::test]
async fn nonexistent_route_returns_404() {
    let server = test_app();
    let resp = server.get("/nonexistent").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// --- Privacy page tests ---

#[tokio::test]
async fn privacy_page_returns_200_with_html() {
    let server = test_app();
    let resp = server.get("/privacy").await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("Privacy Policy"));
}

// --- Trust page tests ---

#[tokio::test]
async fn trust_page_invalid_kind_returns_404() {
    let server = test_app();
    let resp = server.get("/trust/bogus/owner/repo").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trust_page_no_slash_in_id_returns_404() {
    let server = test_app();
    let resp = server.get("/trust/github/noslash").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn trust_page_unimplemented_kind_returns_501() {
    let server = test_app();
    let resp = server.get("/trust/npm/react").await;
    resp.assert_status(axum::http::StatusCode::NOT_IMPLEMENTED);
}

// --- Endorsement tests ---

#[tokio::test]
async fn endorsement_get_unknown_subject_returns_404() {
    let server = test_app();
    let resp = server
        .get("/endorsements?kind=github&id=nonexistent/repo")
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn endorsement_get_invalid_kind_returns_400() {
    let server = test_app();
    let resp = server.get("/endorsements?kind=bogus&id=test").await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_unknown_subject_returns_404() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "nonexistent/repo",
            "category": "usage",
            "proof_hash": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/nonexistent/repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn endorsement_post_invalid_hex_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "proof_hash": "not-hex!!",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

// --- Webhook tests ---

use axum::http::HeaderName;
use axum::http::HeaderValue;
use serial_test::serial;

fn webhook_payload(subject_kind: &str, subject_id: &str, proof_type: &str) -> serde_json::Value {
    serde_json::json!({
        "server_name": "api.github.com",
        "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
        "session": {
            "id": "test-session-123",
            "subject_kind": subject_kind,
            "subject_id": subject_id,
            "category": "usage",
            "proof_type": proof_type
        },
        "transcript": {
            "sent": format!("GET /repos/{subject_id} HTTP/1.1\r\nHost: api.github.com\r\n"),
            "recv": "HTTP/1.1 200 OK\r\n"
        }
    })
}

fn auth_header(token: &str) -> (HeaderName, HeaderValue) {
    (
        HeaderName::from_static("authorization"),
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    )
}

#[tokio::test]
#[serial]
async fn webhook_rejects_without_secret() {
    // VERIFIER_WEBHOOK_SECRET not set → 500
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
    let server = test_app();
    let resp = server
        .post("/webhook/endorsement")
        .json(&webhook_payload("github", "owner/repo", "git_history"))
        .await;
    resp.assert_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
#[serial]
async fn webhook_rejects_bad_auth() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret") };
    let server = test_app();
    let (name, value) = auth_header("wrong-token");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&webhook_payload("github", "owner/repo", "git_history"))
        .await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_missing_subject_kind_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-2") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-2");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [],
            "session": { "id": "s1" },
            "transcript": { "sent": "GET / HTTP/1.1\r\n", "recv": "" }
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_invalid_server_name_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-3") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-3");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "evil.com",
            "results": [],
            "session": {
                "id": "s2",
                "subject_kind": "github",
                "subject_id": "owner/repo",
                "proof_type": "git_history"
            },
            "transcript": {
                "sent": "GET /repos/owner/repo HTTP/1.1\r\nHost: evil.com\r\n",
                "recv": ""
            }
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_happy_path_creates_endorsement() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-4") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-4");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&webhook_payload("github", "test-org/test-repo", "git_history"))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");
    assert!(body["endorsement_id"].as_str().is_some());
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_email_proof_type_blocked() {
    // Email proof type is blocked until transcript binding is designed
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-5") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-5");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "mail.google.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "email-proof"}],
            "session": {
                "id": "email-session",
                "subject_kind": "github",
                "subject_id": "email-org/email-repo",
                "category": "usage",
                "proof_type": "email"
            },
            "transcript": {
                "sent": "GET / HTTP/1.1\r\nHost: mail.google.com\r\n",
                "recv": ""
            }
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

// --- Transcript-subject binding tests ---

#[tokio::test]
#[serial]
async fn webhook_transcript_subject_mismatch_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-7") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-7");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
            "session": {
                "id": "mismatch-session",
                "subject_kind": "github",
                "subject_id": "owner/repoB",
                "proof_type": "git_history"
            },
            "transcript": {
                "sent": "GET /repos/owner/repoA HTTP/1.1\r\nHost: api.github.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n"
            }
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
async fn endorsement_post_transcript_mismatch_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repoB",
            "category": "usage",
            "proof_hash": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/owner/repoA HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_empty_transcript_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "proof_hash": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": ""
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_email_proof_type_blocked() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "proof_hash": "abcd1234",
            "proof_type": "email",
            "transcript_sent": "GET / HTTP/1.1\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}
