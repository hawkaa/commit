pub mod models;
pub mod routes;
pub mod services;
pub mod validation;

use std::sync::Mutex;

use services::{db::Database, github::GitHubClient};

#[derive(Clone)]
pub struct AppState {
    pub db: std::sync::Arc<Mutex<Database>>,
    pub github: std::sync::Arc<GitHubClient>,
    /// Parsed TLSNotary notary server public key for attestation signature
    /// verification. `None` if NOTARY_PUBLIC_KEY is not set (verification skipped).
    pub notary_public_key: Option<k256::ecdsa::VerifyingKey>,
}
