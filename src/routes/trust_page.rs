use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};

use crate::AppState;
use crate::models::{CommitScore, CommitmentSignal, ScoreBreakdown, Subject, SubjectKind};
use crate::routes::endorsement::EndorsementSummary;
use crate::services::score::{build_signals, score_github_repo};
use uuid::Uuid;

#[allow(clippy::missing_errors_doc)]
pub async fn get_trust_page(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<(StatusCode, HeaderMap, String), StatusCode> {
    let subject_kind = SubjectKind::parse(&kind).ok_or(StatusCode::NOT_FOUND)?;

    match subject_kind {
        SubjectKind::GithubRepo => render_github_trust_page(&state, &id).await,
        _ => Err(StatusCode::NOT_IMPLEMENTED),
    }
}

async fn render_github_trust_page(
    state: &AppState,
    identifier: &str,
) -> Result<(StatusCode, HeaderMap, String), StatusCode> {
    let identifier = identifier.trim_end_matches('/');
    let parts: Vec<&str> = identifier.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(StatusCode::NOT_FOUND);
    }
    let (owner, repo_name) = (parts[0], parts[1]);

    // Try cache first
    let cached = {
        let db = state
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Ok(Some(subject)) = db.find_subject(&SubjectKind::GithubRepo, identifier)
            && let Ok(Some((signals_json, score_json))) = db.get_cached_signals(&subject.id)
        {
            let signals: Vec<CommitmentSignal> =
                serde_json::from_str(&signals_json).unwrap_or_default();
            let score: CommitScore = serde_json::from_str(&score_json).unwrap_or(CommitScore {
                score: None,
                breakdown: ScoreBreakdown::default(),
                layer1_only: true,
            });
            Some((subject, signals, score))
        } else {
            None
        }
    };

    let (subject, signals, score) = if let Some(data) = cached {
        data
    } else {
        // Fetch from GitHub
        let gh_repo = state
            .github
            .get_repo(owner, repo_name)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;

        let contributor_count = state
            .github
            .get_contributor_count(owner, repo_name)
            .await
            .unwrap_or(0);

        let score = score_github_repo(&gh_repo, contributor_count);
        let signals = build_signals(&gh_repo, contributor_count);

        let candidate = Subject {
            id: Uuid::new_v4(),
            kind: SubjectKind::GithubRepo,
            identifier: identifier.to_string(),
            display_name: gh_repo.full_name.clone(),
            endorsement_count: 0,
        };

        let db = state
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let _ = db.upsert_subject(&candidate);
        let subject = db
            .find_subject(&SubjectKind::GithubRepo, identifier)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let _ = db.cache_signals(
            &subject.id,
            &serde_json::to_string(&signals).unwrap_or_default(),
            &serde_json::to_string(&score).unwrap_or_default(),
        );

        (subject, signals, score)
    };

    // Query endorsement data
    let (endorsement_count, recent_endorsements) = {
        let db = state
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let count = db.get_endorsement_count(&subject.id).unwrap_or(0);
        let rows = db
            .get_recent_endorsements(&subject.id, 10)
            .unwrap_or_default();
        let summaries: Vec<EndorsementSummary> = rows
            .into_iter()
            .map(|r| EndorsementSummary {
                id: r.id,
                category: r.category,
                proof_type: r.proof_type,
                status: r.status,
                created_at: r.created_at,
            })
            .collect();
        (count, summaries)
    };

    let html = render_html(
        &subject,
        &signals,
        &score,
        owner,
        repo_name,
        endorsement_count,
        &recent_endorsements,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );

    Ok((StatusCode::OK, headers, html))
}

fn score_color(score: Option<u8>) -> (&'static str, &'static str) {
    match score {
        Some(s) if s > 70 => ("#16a34a", "#15803d"),
        Some(s) if s > 40 => ("#ca8a04", "#a16207"),
        Some(_) => ("#6b7280", "#4b5563"),
        None => ("#9ca3af", "#6b7280"),
    }
}

fn score_display(score: Option<u8>) -> String {
    score.map_or("\u{2014}".to_string(), |s| s.to_string())
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn render_html(
    _subject: &Subject,
    signals: &[CommitmentSignal],
    score: &CommitScore,
    owner: &str,
    repo: &str,
    endorsement_count: u32,
    recent_endorsements: &[EndorsementSummary],
) -> String {
    let (color_start, color_end) = score_color(score.score);
    let score_text = score_display(score.score);
    let owner = html_escape(owner);
    let repo = html_escape(repo);
    let description =
        format!("Commit Score for {owner}/{repo}: {score_text}. Trust signals from public data.");
    let title = format!("{owner}/{repo} \u{2014} Commit");

    let signals_html: String = signals
        .iter()
        .map(|s| {
            format!(
                r#"<div class="signal">
          <div class="signal-label">{}</div>
          <div class="signal-value">{}</div>
          <div class="signal-verify">{}</div>
        </div>"#,
                html_escape(&s.label),
                html_escape(&s.value),
                verification_label(&s.verification),
            )
        })
        .collect::<Vec<_>>()
        .join("\n        ");

    let breakdown_html = render_breakdown(&score.breakdown);
    let endorsements_html =
        render_endorsements_section(endorsement_count, recent_endorsements);

    let layer_label = if score.layer1_only {
        r#"<span class="layer-badge">Public data only</span>"#
    } else {
        r#"<span class="layer-badge layer-badge-zk">ZK-verified</span>"#
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <meta name="description" content="{description}">
  <meta property="og:title" content="{title}">
  <meta property="og:description" content="{description}">
  <meta property="og:type" content="website">
  <meta property="og:image" content="https://commit-backend.fly.dev/badge/github/{owner}/{repo}.svg">
  <meta name="twitter:card" content="summary">
  <meta name="twitter:title" content="{title}">
  <meta name="twitter:description" content="{description}">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
  <style>
    *, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      font-family: 'Geist', -apple-system, BlinkMacSystemFont, sans-serif;
      background: #f5f5f0;
      color: #1a1a2e;
      line-height: 1.5;
      -webkit-font-smoothing: antialiased;
    }}
    .container {{
      max-width: 680px;
      margin: 0 auto;
      padding: 48px 32px;
    }}
    .header {{
      display: flex;
      align-items: center;
      gap: 8px;
      margin-bottom: 48px;
    }}
    .header a {{
      color: #666;
      text-decoration: none;
      font-size: 13px;
      font-weight: 500;
    }}
    .header a:hover {{ color: #1a1a2e; }}
    .header .sep {{ color: #ccc; }}
    .hero {{
      display: flex;
      align-items: center;
      gap: 24px;
      margin-bottom: 32px;
    }}
    .score-circle {{
      width: 72px;
      height: 72px;
      border-radius: 50%;
      background: linear-gradient(135deg, {color_start}, {color_end});
      display: flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
    }}
    .score-number {{
      font-family: 'Geist', sans-serif;
      font-weight: 800;
      font-size: 28px;
      color: #fff;
    }}
    .hero-text h1 {{
      font-size: 20px;
      font-weight: 700;
      margin-bottom: 4px;
    }}
    .hero-text h1 a {{
      color: #1a1a2e;
      text-decoration: none;
    }}
    .hero-text h1 a:hover {{ text-decoration: underline; }}
    .hero-text .meta {{
      font-size: 13px;
      color: #666;
      display: flex;
      align-items: center;
      gap: 8px;
    }}
    .layer-badge {{
      display: inline-block;
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      padding: 2px 8px;
      border-radius: 4px;
      background: #e5e5e0;
      color: #666;
    }}
    .layer-badge-zk {{
      background: rgba(124, 58, 237, 0.1);
      color: #7c3aed;
    }}
    .card {{
      background: #fff;
      border: 1px solid #e5e5e0;
      border-radius: 12px;
      padding: 24px;
      margin-bottom: 24px;
    }}
    .card-title {{
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 1px;
      color: #888;
      margin-bottom: 16px;
    }}
    .signals {{
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
      gap: 16px;
    }}
    .signal {{
      padding: 12px;
      background: #f5f5f0;
      border-radius: 6px;
    }}
    .signal-label {{
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      color: #888;
      margin-bottom: 4px;
    }}
    .signal-value {{
      font-size: 16px;
      font-weight: 600;
      color: #1a1a2e;
      font-variant-numeric: tabular-nums;
    }}
    .signal-verify {{
      font-size: 11px;
      color: #aaa;
      margin-top: 4px;
    }}
    .breakdown {{
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 12px;
    }}
    .breakdown-item {{
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 8px 0;
      border-bottom: 1px solid #f0f0eb;
    }}
    .breakdown-label {{
      font-size: 13px;
      color: #666;
    }}
    .breakdown-value {{
      font-family: 'JetBrains Mono', monospace;
      font-size: 13px;
      font-weight: 500;
      color: #1a1a2e;
    }}
    .endorsement-row {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 10px 0;
      border-bottom: 1px solid #f0f0eb;
    }}
    .endorsement-row:last-child {{ border-bottom: none; }}
    .endorsement-info {{
      display: flex;
      align-items: center;
      gap: 8px;
    }}
    .endorsement-category {{
      font-size: 13px;
      font-weight: 500;
      color: #1a1a2e;
    }}
    .endorsement-proof {{
      font-size: 11px;
      color: #888;
    }}
    .endorsement-status {{
      display: inline-block;
      font-size: 10px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      padding: 2px 8px;
      border-radius: 4px;
    }}
    .endorsement-status--verified {{
      background: rgba(124, 58, 237, 0.1);
      color: #7c3aed;
    }}
    .endorsement-status--pending {{
      background: #e5e5e0;
      color: #888;
    }}
    .endorsement-time {{
      font-size: 11px;
      color: #aaa;
    }}
    .endorsement-empty {{
      font-size: 13px;
      color: #888;
      padding: 8px 0;
    }}
    .badge-section {{
      margin-top: 32px;
      padding: 24px;
      background: #fff;
      border: 1px solid #e5e5e0;
      border-radius: 12px;
    }}
    .badge-section .card-title {{ margin-bottom: 12px; }}
    .badge-preview {{
      margin-bottom: 16px;
    }}
    .badge-code {{
      font-family: 'JetBrains Mono', monospace;
      font-size: 12px;
      background: #f5f5f0;
      padding: 12px;
      border-radius: 6px;
      overflow-x: auto;
      color: #666;
      user-select: all;
    }}
    .footer {{
      margin-top: 48px;
      padding-top: 24px;
      border-top: 1px solid #e5e5e0;
      font-size: 13px;
      color: #888;
      display: flex;
      justify-content: space-between;
    }}
    .footer a {{
      color: #666;
      text-decoration: none;
    }}
    .footer a:hover {{ color: #1a1a2e; }}
    @media (max-width: 480px) {{
      .container {{ padding: 24px 16px; }}
      .hero {{ gap: 16px; }}
      .score-circle {{ width: 56px; height: 56px; }}
      .score-number {{ font-size: 22px; }}
      .signals {{ grid-template-columns: 1fr 1fr; }}
      .breakdown {{ grid-template-columns: 1fr; }}
    }}
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <a href="/">commit</a>
      <span class="sep">/</span>
      <a href="/trust/github/{owner}/{repo}">github/{owner}/{repo}</a>
    </div>

    <div class="hero">
      <div class="score-circle">
        <span class="score-number">{score_text}</span>
      </div>
      <div class="hero-text">
        <h1><a href="https://github.com/{owner}/{repo}" rel="noopener">{owner}/{repo}</a></h1>
        <div class="meta">
          Commit Score {layer_label}
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-title">Signals</div>
      <div class="signals">
        {signals_html}
      </div>
    </div>

    <div class="card">
      <div class="card-title">Score Breakdown</div>
      {breakdown_html}
    </div>

    {endorsements_html}

    <div class="badge-section">
      <div class="card-title">Add badge to README</div>
      <div class="badge-preview">
        <img src="/badge/github/{owner}/{repo}.svg" alt="Commit Score" height="20">
      </div>
      <div class="badge-code">[![Commit Score](/badge/github/{owner}/{repo}.svg)](/trust/github/{owner}/{repo})</div>
    </div>

    <div class="footer">
      <span>Commit Score (beta) — based on public data. Endorse this repo to improve accuracy.</span>
      <a href="https://github.com/hawkaa/commit">GitHub</a>
    </div>
  </div>
</body>
</html>"#
    )
}

fn render_endorsements_section(
    count: u32,
    endorsements: &[EndorsementSummary],
) -> String {
    let title = if count > 0 {
        format!("Endorsements ({count})")
    } else {
        "Endorsements".to_string()
    };

    let body = if endorsements.is_empty() {
        r#"<div class="endorsement-empty">No endorsements yet. Install the Commit extension to endorse this repo.</div>"#.to_string()
    } else {
        let rows: String = endorsements
            .iter()
            .map(|e| {
                let status_class = if e.status == "verified" {
                    "endorsement-status--verified"
                } else {
                    "endorsement-status--pending"
                };
                let status_label = if e.status == "verified" {
                    "ZK Verified"
                } else {
                    "Pending"
                };
                format!(
                    r#"<div class="endorsement-row">
          <div class="endorsement-info">
            <span class="endorsement-category">{}</span>
            <span class="endorsement-proof">{}</span>
          </div>
          <div>
            <span class="endorsement-status {status_class}">{status_label}</span>
            <span class="endorsement-time">{}</span>
          </div>
        </div>"#,
                    html_escape(&e.category),
                    html_escape(&e.proof_type),
                    html_escape(&e.created_at),
                )
            })
            .collect::<Vec<_>>()
            .join("\n        ");
        rows
    };

    format!(
        r#"<div class="card">
      <div class="card-title">{title}</div>
      {body}
    </div>"#
    )
}

fn render_breakdown(b: &ScoreBreakdown) -> String {
    let items = [
        ("Longevity", b.longevity, 15.0),
        ("Maintenance", b.maintenance, 10.0),
        ("Community", b.community, 10.0),
        ("Financial", b.financial, 5.0),
    ];

    let html: String = items
        .iter()
        .map(|(label, val, max)| {
            format!(
                r#"<div class="breakdown-item">
          <span class="breakdown-label">{label}</span>
          <span class="breakdown-value">{val:.1} / {max:.0}</span>
        </div>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ");

    format!(r#"<div class="breakdown">{html}</div>"#)
}

fn verification_label(v: &crate::models::VerificationLevel) -> &'static str {
    match v {
        crate::models::VerificationLevel::PublicApi => "Public API",
        crate::models::VerificationLevel::Scraped => "Scraped",
        crate::models::VerificationLevel::ZkVerified => "ZK Verified",
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
