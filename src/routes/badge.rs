use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};

use crate::AppState;
use crate::models::{CommitScore, SubjectKind};

#[allow(clippy::unused_async, clippy::missing_panics_doc)] // Axum handler
pub async fn get_badge(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
) -> impl IntoResponse {
    let id = id.trim_end_matches(".svg");
    let subject_kind = SubjectKind::parse(&kind);

    let score: Option<u8> = subject_kind.and_then(|kind| {
        let db = state.db.lock().ok()?;
        let subject = db.find_subject(&kind, id).ok()??;
        let (_, score_json) = db.get_cached_signals(&subject.id).ok()??;
        let commit_score: CommitScore = serde_json::from_str(&score_json).ok()?;
        commit_score.score
    });

    let (value_text, color) = match score {
        Some(s) if s > 70 => (format!("{s}"), "#16a34a"),
        Some(s) if s > 40 => (format!("{s}"), "#ca8a04"),
        Some(s) => (format!("{s}"), "#6b7280"),
        None => ("\u{2014}".to_string(), "#9ca3af"),
    };

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="88" height="20">
  <rect width="52" height="20" rx="3" fill="#555"/>
  <rect x="52" width="36" height="20" rx="3" fill="{color}"/>
  <rect x="52" width="4" height="20" fill="{color}"/>
  <text x="26" y="14" fill="#fff" font-family="Verdana,sans-serif" font-size="11" text-anchor="middle">commit</text>
  <text x="70" y="14" fill="#fff" font-family="Verdana,sans-serif" font-size="11" font-weight="bold" text-anchor="middle">{value_text}</text>
</svg>"##
    );

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "image/svg+xml".parse().unwrap());
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );

    (StatusCode::OK, headers, svg)
}
