use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    GithubRepo,
    NpmPackage,
    CratesIoCrate,
    Business,
    Service,
}

impl SubjectKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SubjectKind::GithubRepo => "github_repo",
            SubjectKind::NpmPackage => "npm_package",
            SubjectKind::CratesIoCrate => "crates_io_crate",
            SubjectKind::Business => "business",
            SubjectKind::Service => "service",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "github_repo" | "github" => Some(SubjectKind::GithubRepo),
            "npm_package" | "npm" => Some(SubjectKind::NpmPackage),
            "crates_io_crate" | "crates" => Some(SubjectKind::CratesIoCrate),
            "business" | "brreg" => Some(SubjectKind::Business),
            "service" => Some(SubjectKind::Service),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub id: Uuid,
    pub kind: SubjectKind,
    pub identifier: String,
    pub display_name: String,
    pub endorsement_count: u32,
}
