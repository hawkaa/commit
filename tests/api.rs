use axum::{
    Router,
    routing::{get, post},
};
use axum_test::TestServer;
use commit_backend::{
    AppState,
    models::{Subject, SubjectKind},
    services::{db::Database, github::GitHubClient},
};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

fn build_app() -> (TestServer, AppState) {
    let db = Database::open(":memory:").expect("in-memory db");
    let github = GitHubClient::new(None);
    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        github: Arc::new(github),
        notary_public_key: None,
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
        .with_state(state.clone());
    (TestServer::new(app), state)
}

fn test_app() -> TestServer {
    build_app().0
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

#[tokio::test]
async fn trust_page_includes_install_cta() {
    let (server, state) = build_app();

    // Seed a subject with cached signals so the trust page renders without GitHub API
    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "cta-org/cta-repo".to_string(),
            display_name: "cta-org/cta-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "cta-org/cta-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"2yr","verification":"public_api","timestamp":"2024-01-01","confidence":0.9}]"#,
            r#"{"score":42,"breakdown":{"longevity":7.0,"maintenance":5.0,"community":3.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let resp = server.get("/trust/github/cta-org/cta-repo").await;
    resp.assert_status_ok();
    let body = resp.text();

    // CTA text is present
    assert!(
        body.contains("Get the Commit extension"),
        "trust page should contain the install CTA text"
    );

    // CTA link opens in new tab with noopener
    assert!(
        body.contains(r##"target="_blank""##),
        "CTA link should have target=_blank"
    );
    assert!(
        body.contains(r#"rel="noopener""#),
        "CTA link should have rel=noopener"
    );
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
            "attestation": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/nonexistent/repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn endorsement_post_invalid_attestation_hex_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "not-hex!!",
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
    let transcript_sent = match proof_type {
        "ci_logs" => {
            format!("GET /repos/{subject_id}/actions/runs HTTP/1.1\r\nHost: api.github.com\r\n")
        }
        _ => format!("GET /repos/{subject_id} HTTP/1.1\r\nHost: api.github.com\r\n"),
    };
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
            "sent": transcript_sent,
            "recv": "HTTP/1.1 200 OK\r\n"
        },
        "attestation": "deadbeef01020304"
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
            "transcript": { "sent": "GET / HTTP/1.1\r\n", "recv": "" },
            "attestation": "deadbeef"
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
            },
            "attestation": "deadbeef"
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
        .json(&webhook_payload(
            "github",
            "test-org/test-repo",
            "git_history",
        ))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");
    assert!(body["endorsement_id"].as_str().is_some());
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_email_proof_type_happy_path() {
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
                "recv": "HTTP/1.1 200 OK\r\n\r\nCheck https://github.com/email-org/email-repo/issues/1"
            },
            "attestation": "deadbeef"
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_email_no_matching_recv_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-5b") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-5b");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "mail.google.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "email-proof"}],
            "session": {
                "id": "email-session-bad",
                "subject_kind": "github",
                "subject_id": "email-org/email-repo",
                "category": "usage",
                "proof_type": "email"
            },
            "transcript": {
                "sent": "GET / HTTP/1.1\r\nHost: mail.google.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n\r\nNo repo URL here"
            },
            "attestation": "deadbeef"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_email_missing_recv_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-5c") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-5c");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "mail.google.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "email-proof"}],
            "session": {
                "id": "email-session-no-recv",
                "subject_kind": "github",
                "subject_id": "email-org/email-repo",
                "category": "usage",
                "proof_type": "email"
            },
            "transcript": {
                "sent": "GET / HTTP/1.1\r\nHost: mail.google.com\r\n"
            },
            "attestation": "deadbeef"
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
            },
            "attestation": "deadbeef"
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
            "attestation": "abcd1234",
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
            "attestation": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": ""
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_empty_attestation_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_email_missing_recv_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "email",
            "transcript_sent": "GET / HTTP/1.1\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn endorsement_post_email_with_recv_validates() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "email",
            "transcript_sent": "GET / HTTP/1.1\r\n",
            "transcript_recv": "HTTP/1.1 200 OK\r\n\r\nhttps://github.com/owner/repo"
        }))
        .await;
    // Subject doesn't exist -> 404 (but transcript validation passed)
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// --- Endorsement happy path + replay prevention tests ---

#[tokio::test]
#[serial]
async fn webhook_happy_path_with_attestation_uses_attestation_hash() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-att") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-att");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
            "session": {
                "id": "att-session",
                "subject_kind": "github",
                "subject_id": "att-org/att-repo",
                "category": "usage",
                "proof_type": "git_history"
            },
            "transcript": {
                "sent": "GET /repos/att-org/att-repo HTTP/1.1\r\nHost: api.github.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n"
            },
            "attestation": "deadbeef01020304"
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_missing_attestation_returns_422() {
    // Webhook without required attestation field is rejected at deserialization
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-compat") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-compat");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "compat-test"}],
            "session": {
                "id": "compat-session",
                "subject_kind": "github",
                "subject_id": "compat-org/compat-repo",
                "category": "usage",
                "proof_type": "git_history"
            },
            "transcript": {
                "sent": "GET /repos/compat-org/compat-repo HTTP/1.1\r\nHost: api.github.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n"
            }
        }))
        .await;
    // axum rejects the request because the required `attestation` field is missing
    resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_duplicate_attestation_returns_409() {
    // Same attestation submitted twice should return 409 Conflict (replay prevention)
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-dup") };
    let server = test_app();

    let payload = serde_json::json!({
        "server_name": "api.github.com",
        "results": [{"type": "RECV", "part": "BODY", "value": "dup-test"}],
        "session": {
            "id": "dup-session",
            "subject_kind": "github",
            "subject_id": "dup-org/dup-repo",
            "category": "usage",
            "proof_type": "git_history"
        },
        "transcript": {
            "sent": "GET /repos/dup-org/dup-repo HTTP/1.1\r\nHost: api.github.com\r\n",
            "recv": "HTTP/1.1 200 OK\r\n"
        },
        "attestation": "aabbccdd11223344"
    });

    // First submission: should succeed
    let (name, value) = auth_header("test-secret-dup");
    let resp1 = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&payload)
        .await;
    resp1.assert_status_ok();

    // Second submission with same attestation: should return 409
    let (name2, value2) = auth_header("test-secret-dup");
    let resp2 = server
        .post("/webhook/endorsement")
        .add_header(name2, value2)
        .json(&payload)
        .await;
    resp2.assert_status(axum::http::StatusCode::CONFLICT);

    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

// --- ci_logs transcript binding tests ---

#[tokio::test]
#[serial]
async fn webhook_ci_logs_happy_path() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-ci") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-ci");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&webhook_payload("github", "ci-org/ci-repo", "ci_logs"))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
#[serial]
async fn webhook_ci_logs_missing_actions_returns_400() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-ci2") };
    let server = test_app();
    let (name, value) = auth_header("test-secret-ci2");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
            "session": {
                "id": "ci-session-bad",
                "subject_kind": "github",
                "subject_id": "ci-org/ci-repo",
                "proof_type": "ci_logs"
            },
            "transcript": {
                "sent": "GET /repos/ci-org/ci-repo/commits HTTP/1.1\r\nHost: api.github.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n"
            },
            "attestation": "deadbeef01020304"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
async fn endorsement_post_ci_logs_happy_path() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "ci-org/ci-repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "ci_logs",
            "transcript_sent": "GET /repos/ci-org/ci-repo/actions/runs HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn endorsement_post_ci_logs_missing_actions_returns_400() {
    let server = test_app();
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "ci_logs",
            "transcript_sent": "GET /repos/owner/repo/commits HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

// --- Rate limiting tests ---

#[tokio::test]
#[serial]
async fn webhook_rate_limit_triggers_after_5_endorsements() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-rate") };
    let server = test_app();

    for i in 0..5 {
        let (name, value) = auth_header("test-secret-rate");
        let resp = server
            .post("/webhook/endorsement")
            .add_header(name, value)
            .json(&serde_json::json!({
                "server_name": "api.github.com",
                "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
                "session": {
                    "id": format!("rate-session-{i}"),
                    "subject_kind": "github",
                    "subject_id": "rate-org/rate-repo",
                    "proof_type": "git_history"
                },
                "transcript": {
                    "sent": "GET /repos/rate-org/rate-repo HTTP/1.1\r\nHost: api.github.com\r\n",
                    "recv": "HTTP/1.1 200 OK\r\n"
                },
                "attestation": format!("deadbeef{i:08x}{i:08x}")
            }))
            .await;
        resp.assert_status_ok();
    }

    let (name, value) = auth_header("test-secret-rate");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&serde_json::json!({
            "server_name": "api.github.com",
            "results": [{"type": "RECV", "part": "BODY", "value": "test"}],
            "session": {
                "id": "rate-session-6",
                "subject_kind": "github",
                "subject_id": "rate-org/rate-repo",
                "proof_type": "git_history"
            },
            "transcript": {
                "sent": "GET /repos/rate-org/rate-repo HTTP/1.1\r\nHost: api.github.com\r\n",
                "recv": "HTTP/1.1 200 OK\r\n"
            },
            "attestation": "deadbeef99999999"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::TOO_MANY_REQUESTS);

    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

// --- E2E integration test: endorsement appears in trust card ---

#[tokio::test]
async fn endorsement_appears_in_trust_card_response() {
    let (server, state) = build_app();

    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "e2e-org/e2e-repo".to_string(),
            display_name: "e2e-org/e2e-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "e2e-org/e2e-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"3yr","verification":"public_api","timestamp":"2023-01-01","confidence":0.9}]"#,
            r#"{"score":55,"breakdown":{"longevity":9.0,"maintenance":6.0,"community":5.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "e2e-org/e2e-repo",
            "category": "usage",
            "attestation": "deadbeefcafe0123456789abcdef0123",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/e2e-org/e2e-repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status_ok();
    let endorsement: serde_json::Value = resp.json();
    assert!(endorsement["id"].as_str().is_some());
    assert_eq!(endorsement["status"], "pending_attestation");

    // Re-seed cache after endorsement (endorsement creation invalidates the cache,
    // and without a live GitHub API the trust card fetch would fail).
    // NOTE: This re-seeds with layer1_only:true, so it does NOT exercise the real
    // score computation path with endorsements factored in. The score computation
    // with endorsements is covered by the score.rs unit tests.
    {
        let db = state.db.lock().unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "e2e-org/e2e-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"3yr","verification":"public_api","timestamp":"2023-01-01","confidence":0.9}]"#,
            r#"{"score":55,"breakdown":{"longevity":9.0,"maintenance":6.0,"community":5.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let trust_resp = server
        .get("/trust-card?kind=github&id=e2e-org/e2e-repo")
        .await;
    trust_resp.assert_status_ok();
    let trust_card: serde_json::Value = trust_resp.json();
    assert!(
        trust_card["endorsement_count"].as_u64().unwrap() > 0,
        "endorsement_count should be > 0 after creating an endorsement"
    );
    assert!(
        !trust_card["recent_endorsements"]
            .as_array()
            .unwrap()
            .is_empty(),
        "recent_endorsements should not be empty"
    );
    assert_eq!(
        trust_card["recent_endorsements"][0]["category"], "usage",
        "endorsement category should match"
    );
}

// --- Attestation row creation tests ---

#[tokio::test]
async fn endorsement_post_without_notary_key_creates_no_attestation_row() {
    let (server, state) = build_app();

    // Set up a subject
    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "att-test-org/att-test-repo".to_string(),
            display_name: "att-test-org/att-test-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
    }

    // Submit endorsement without notary key configured (state.notary_public_key is None)
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "att-test-org/att-test-repo",
            "category": "usage",
            "attestation": "deadbeef01020304aabbccdd",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/att-test-org/att-test-repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "pending_attestation");

    // Verify no attestation row was created (since endorsement is not verified)
    let db = state.db.lock().unwrap();
    let endorsement_id = body["id"].as_str().unwrap();
    let attestation = db.get_attestation_for_endorsement(endorsement_id).unwrap();
    assert!(
        attestation.is_none(),
        "No attestation row should be created for pending_attestation endorsements"
    );
}

#[tokio::test]
#[serial]
async fn webhook_creates_attestation_row_with_base_sepolia_chain() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-att-row") };
    let (server, state) = build_app();
    let (name, value) = auth_header("test-secret-att-row");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&webhook_payload(
            "github",
            "att-row-org/att-row-repo",
            "git_history",
        ))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "verified");

    // Verify attestation row exists with chain = "base_sepolia" and tx_hash = NULL
    let db = state.db.lock().unwrap();
    let endorsement_id = body["endorsement_id"].as_str().unwrap();
    let attestation = db
        .get_attestation_for_endorsement(endorsement_id)
        .unwrap()
        .expect("attestation row should exist for verified webhook endorsement");
    assert_eq!(attestation.chain, "base_sepolia");
    assert!(
        attestation.tx_hash.is_none(),
        "tx_hash should be NULL before L2 submission"
    );

    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

// --- Pending attestation query tests ---

#[tokio::test]
#[serial]
async fn pending_attestation_appears_in_get_pending_attestations() {
    unsafe { std::env::set_var("VERIFIER_WEBHOOK_SECRET", "test-secret-pending") };
    let (server, state) = build_app();
    let (name, value) = auth_header("test-secret-pending");
    let resp = server
        .post("/webhook/endorsement")
        .add_header(name, value)
        .json(&webhook_payload(
            "github",
            "pending-org/pending-repo",
            "git_history",
        ))
        .await;
    resp.assert_status_ok();

    // Verify the attestation appears in get_pending_attestations
    let db = state.db.lock().unwrap();
    let pending = db.get_pending_attestations(100).unwrap();
    assert!(
        !pending.is_empty(),
        "should have at least one pending attestation"
    );

    let found = pending.iter().any(|p| !p.endorsement_proof_hash.is_empty());
    assert!(
        found,
        "pending attestation should have a non-empty proof hash"
    );

    unsafe { std::env::remove_var("VERIFIER_WEBHOOK_SECRET") };
}

#[tokio::test]
async fn update_attestation_tx_removes_from_pending() {
    let (_server, state) = build_app();

    let db = state.db.lock().unwrap();

    // Create a subject and endorsement manually
    let subject = Subject {
        id: Uuid::new_v4(),
        kind: SubjectKind::GithubRepo,
        identifier: "pending-remove-org/pending-remove-repo".to_string(),
        display_name: "pending-remove-org/pending-remove-repo".to_string(),
        endorsement_count: 0,
    };
    db.upsert_subject(&subject).unwrap();
    let stored = db
        .find_subject(
            &SubjectKind::GithubRepo,
            "pending-remove-org/pending-remove-repo",
        )
        .unwrap()
        .unwrap();

    let endorsement_id = Uuid::new_v4();
    let proof_hash = [0xab_u8; 32];
    db.create_endorsement(
        &endorsement_id,
        &stored.id,
        "usage",
        &proof_hash,
        "git_history",
        None,
        None, // endorser_key_hash
    )
    .unwrap();
    db.update_endorsement_status(&endorsement_id, "verified")
        .unwrap();

    let attestation_id = Uuid::new_v4();
    db.create_attestation(&attestation_id, &endorsement_id, "base_sepolia")
        .unwrap();

    // Should appear in pending
    let pending = db.get_pending_attestations(100).unwrap();
    let found = pending
        .iter()
        .any(|p| p.endorsement_id == endorsement_id.to_string());
    assert!(found, "attestation should be in pending list");

    // Update with tx_hash — should no longer appear in pending
    db.update_attestation_tx(&attestation_id, "0xdeadbeef", 12345)
        .unwrap();

    let pending_after = db.get_pending_attestations(100).unwrap();
    let still_found = pending_after
        .iter()
        .any(|p| p.endorsement_id == endorsement_id.to_string());
    assert!(
        !still_found,
        "attestation should not be in pending list after update"
    );
}

#[tokio::test]
async fn get_pending_attestations_returns_empty_when_none() {
    let (_server, state) = build_app();
    let db = state.db.lock().unwrap();
    // Fresh in-memory DB should have no pending attestations
    let pending = db.get_pending_attestations(100).unwrap();
    assert!(
        pending.is_empty(),
        "should return empty when no pending attestations exist"
    );
}

// --- Attestation status in trust card / endorsement API tests ---

#[tokio::test]
async fn trust_card_endorsement_shows_on_chain_false_when_no_attestation() {
    let (server, state) = build_app();

    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "onchain-org/onchain-repo".to_string(),
            display_name: "onchain-org/onchain-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "onchain-org/onchain-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"1yr","verification":"public_api","timestamp":"2024-01-01","confidence":0.9}]"#,
            r#"{"score":50,"breakdown":{"longevity":8.0,"maintenance":5.0,"community":4.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    // Create endorsement without notary key (no attestation row)
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "onchain-org/onchain-repo",
            "category": "usage",
            "attestation": "aabb1122334455667788",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/onchain-org/onchain-repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status_ok();

    // Re-seed cache after endorsement (cache was invalidated by endorsement creation)
    {
        let db = state.db.lock().unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "onchain-org/onchain-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"1yr","verification":"public_api","timestamp":"2024-01-01","confidence":0.9}]"#,
            r#"{"score":50,"breakdown":{"longevity":8.0,"maintenance":5.0,"community":4.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let trust_resp = server
        .get("/trust-card?kind=github&id=onchain-org/onchain-repo")
        .await;
    trust_resp.assert_status_ok();
    let trust_card: serde_json::Value = trust_resp.json();

    let endorsements = trust_card["recent_endorsements"].as_array().unwrap();
    assert!(!endorsements.is_empty());
    assert_eq!(endorsements[0]["on_chain"], false);
    assert!(endorsements[0]["tx_hash"].is_null());
}

#[tokio::test]
async fn trust_card_endorsement_shows_on_chain_true_when_attested() {
    let (server, state) = build_app();

    let endorsement_id;
    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "attested-org/attested-repo".to_string(),
            display_name: "attested-org/attested-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "attested-org/attested-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"1yr","verification":"public_api","timestamp":"2024-01-01","confidence":0.9}]"#,
            r#"{"score":50,"breakdown":{"longevity":8.0,"maintenance":5.0,"community":4.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();

        // Create endorsement + attestation manually to simulate on-chain submission
        endorsement_id = Uuid::new_v4();
        let proof_hash = [0xcc_u8; 32];
        db.create_endorsement(
            &endorsement_id,
            &stored.id,
            "usage",
            &proof_hash,
            "git_history",
            None,
            None, // endorser_key_hash
        )
        .unwrap();
        db.update_endorsement_status(&endorsement_id, "verified")
            .unwrap();

        let attestation_id = Uuid::new_v4();
        db.create_attestation(&attestation_id, &endorsement_id, "base_sepolia")
            .unwrap();
        db.update_attestation_tx(&attestation_id, "0xdeadbeefcafe1234", 99999)
            .unwrap();
    }

    let trust_resp = server
        .get("/trust-card?kind=github&id=attested-org/attested-repo")
        .await;
    trust_resp.assert_status_ok();
    let trust_card: serde_json::Value = trust_resp.json();

    let endorsements = trust_card["recent_endorsements"].as_array().unwrap();
    assert!(!endorsements.is_empty());
    assert_eq!(endorsements[0]["on_chain"], true);
    assert_eq!(endorsements[0]["tx_hash"], "0xdeadbeefcafe1234");
}

#[tokio::test]
async fn endorsement_with_pending_attestation_shows_on_chain_false() {
    let (_server, state) = build_app();

    let db = state.db.lock().unwrap();
    let subject = Subject {
        id: Uuid::new_v4(),
        kind: SubjectKind::GithubRepo,
        identifier: "pending-chain-org/pending-chain-repo".to_string(),
        display_name: "pending-chain-org/pending-chain-repo".to_string(),
        endorsement_count: 0,
    };
    db.upsert_subject(&subject).unwrap();
    let stored = db
        .find_subject(
            &SubjectKind::GithubRepo,
            "pending-chain-org/pending-chain-repo",
        )
        .unwrap()
        .unwrap();

    let endorsement_id = Uuid::new_v4();
    let proof_hash = [0xdd_u8; 32];
    db.create_endorsement(
        &endorsement_id,
        &stored.id,
        "usage",
        &proof_hash,
        "git_history",
        None,
        None, // endorser_key_hash
    )
    .unwrap();
    db.update_endorsement_status(&endorsement_id, "verified")
        .unwrap();

    // Create attestation but don't set tx_hash (still pending)
    let attestation_id = Uuid::new_v4();
    db.create_attestation(&attestation_id, &endorsement_id, "base_sepolia")
        .unwrap();

    let attestation = db
        .get_attestation_for_endorsement(&endorsement_id.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(attestation.chain, "base_sepolia");
    assert!(attestation.tx_hash.is_none());
    // Pending attestation should not show as on_chain
    // (verified by the map_endorsement_rows logic which checks tx_hash.is_some())
}
// --- Endorser key hash tests ---

#[tokio::test]
async fn endorsement_with_key_hash_stores_hash() {
    let (server, state) = build_app();
    let key_hash = "a".repeat(64); // valid 64-char hex

    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "keyhash-org/keyhash-repo".to_string(),
            display_name: "keyhash-org/keyhash-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "keyhash-org/keyhash-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"1yr","verification":"public_api","timestamp":"2023-01-01","confidence":0.9}]"#,
            r#"{"score":50,"breakdown":{"longevity":5.0,"maintenance":5.0,"community":5.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "keyhash-org/keyhash-repo",
            "category": "usage",
            "attestation": "deadbeef11223344aabbccdd",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/keyhash-org/keyhash-repo HTTP/1.1\r\nHost: api.github.com\r\n",
            "endorser_key_hash": key_hash
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(body["id"].as_str().is_some());
}

#[tokio::test]
async fn endorsement_without_key_hash_backward_compat() {
    let (server, state) = build_app();

    {
        let db = state.db.lock().unwrap();
        let subject = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: "compat-org/compat-repo".to_string(),
            display_name: "compat-org/compat-repo".to_string(),
            endorsement_count: 0,
        };
        db.upsert_subject(&subject).unwrap();
        let stored = db
            .find_subject(&SubjectKind::GithubRepo, "compat-org/compat-repo")
            .unwrap()
            .unwrap();
        db.cache_signals(
            &stored.id,
            r#"[{"source":"registry","category":"longevity","label":"Age","value":"1yr","verification":"public_api","timestamp":"2023-01-01","confidence":0.9}]"#,
            r#"{"score":50,"breakdown":{"longevity":5.0,"maintenance":5.0,"community":5.0,"financial":0.0,"endorsements":0.0,"network_density":0.0,"proof_strength":0.0,"tenure":0.0},"layer1_only":true}"#,
        )
        .unwrap();
    }

    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "compat-org/compat-repo",
            "category": "usage",
            "attestation": "deadbeef55667788",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/compat-org/compat-repo HTTP/1.1\r\nHost: api.github.com\r\n"
        }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert!(body["id"].as_str().is_some());
}

#[tokio::test]
async fn endorsement_invalid_key_hash_returns_400() {
    let server = test_app();

    // Too short
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n",
            "endorser_key_hash": "tooshort"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    // Non-hex characters
    let resp = server
        .post("/endorsements")
        .json(&serde_json::json!({
            "subject_kind": "github",
            "subject_id": "owner/repo",
            "category": "usage",
            "attestation": "abcd1234",
            "proof_type": "git_history",
            "transcript_sent": "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n",
            "endorser_key_hash": "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

// --- Network query endpoint removal regression test ---

#[tokio::test]
async fn network_query_endpoint_removed() {
    let server = test_app();
    let key_a = "a".repeat(64);
    let resp = server
        .post("/network-query")
        .json(&serde_json::json!({
            "kind": "github",
            "id": "owner/repo",
            "key_hashes": [key_a]
        }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}
