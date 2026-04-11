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
}
