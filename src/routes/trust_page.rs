use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};

use crate::AppState;
use crate::models::{
    CommitScore, CommitmentSignal, EndorsementSummary, ScoreBreakdown, Subject, SubjectKind,
};
use crate::services::score::{
    build_signals, score_github_repo, score_github_repo_with_endorsements,
};
use uuid::Uuid;

// TODO: replace after CWS approval
const CHROME_WEBSTORE_URL: &str = "https://chromewebstore.google.com/";

const DEFAULT_PUBLIC_URL: &str = "https://commit-backend.fly.dev";

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

        // Compute score with endorsement status weighting
        let (verified_count, pending_count) = db
            .get_endorsement_counts_by_status(&subject.id)
            .unwrap_or((0, 0));
        let score = if verified_count > 0 || pending_count > 0 {
            let avg_tenure_months = db.get_endorsement_tenure_months(&subject.id).unwrap_or(0.0);
            score_github_repo_with_endorsements(
                &gh_repo,
                contributor_count,
                verified_count,
                pending_count,
                avg_tenure_months,
                0, // unique_endorser_count: not yet available (network keyring)
            )
        } else {
            score_github_repo(&gh_repo, contributor_count)
        };

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
        let endorsement_ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let attestation_map = db
            .get_attestations_for_endorsements(&endorsement_ids)
            .unwrap_or_default();
        let summaries: Vec<EndorsementSummary> = rows
            .into_iter()
            .map(|r| {
                let (on_chain, tx_hash) = attestation_map.get(&r.id).map_or((false, None), |att| {
                    if att.tx_hash.is_some() {
                        (true, att.tx_hash.clone())
                    } else {
                        (false, None)
                    }
                });
                EndorsementSummary {
                    id: r.id,
                    category: r.category,
                    proof_type: r.proof_type,
                    status: r.status,
                    sentiment: r.sentiment,
                    created_at: r.created_at,
                    on_chain,
                    tx_hash,
                }
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
    let public_url = std::env::var("PUBLIC_URL").unwrap_or_else(|_| DEFAULT_PUBLIC_URL.to_string());
    let (color_start, _color_end) = score_color(score.score);
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

    let breakdown_html = render_breakdown(&score.breakdown, score.layer1_only);
    let endorsements_html = render_endorsements_section(endorsement_count, recent_endorsements);

    let layer_label = if score.layer1_only {
        r#"<span class="layer-badge">Public data only</span>"#
    } else {
        r#"<span class="layer-badge layer-badge-zk">Public + ZK data</span>"#
    };

    let cta_url = CHROME_WEBSTORE_URL;

    // SVG score arc geometry
    let radius: f64 = 30.0; // inner radius for stroke ring (72px circle - stroke offset)
    let circumference = 2.0 * std::f64::consts::PI * radius;
    let score_fraction = score.score.map_or(0.0, |s| f64::from(s) / 100.0);
    let dash_offset = circumference * (1.0 - score_fraction);

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
  <meta property="og:image" content="{public_url}/badge/github/{owner}/{repo}.svg">
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
      position: relative;
      flex-shrink: 0;
    }}
    .score-circle svg {{
      width: 72px;
      height: 72px;
      transform: rotate(-90deg);
    }}
    .score-track {{
      fill: none;
      stroke: #e5e5e0;
      stroke-width: 6;
    }}
    .score-arc {{
      fill: none;
      stroke-width: 6;
      stroke-linecap: round;
      animation: score-fill 400ms ease-out forwards;
    }}
    .score-number {{
      position: absolute;
      top: 50%;
      left: 50%;
      transform: translate(-50%, -50%);
      font-family: 'Geist', sans-serif;
      font-weight: 800;
      font-size: 28px;
      color: #1a1a2e;
      animation: fade-in 400ms ease-out;
    }}
    @keyframes fade-in {{
      from {{ opacity: 0; }}
      to {{ opacity: 1; }}
    }}
    @media (prefers-reduced-motion: reduce) {{
      .score-arc {{ animation: none; }}
      .score-number {{ animation: none; }}
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
    .breakdown-section-label {{
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 1px;
      color: #888;
      margin-bottom: 8px;
      margin-top: 16px;
    }}
    .breakdown-section-label:first-child {{
      margin-top: 0;
    }}
    .breakdown-section-label--zk {{
      color: #7c3aed;
    }}
    .breakdown-label--zk {{
      color: #7c3aed;
    }}
    .breakdown-zk-tag {{
      font-size: 9px;
      background: rgba(124, 58, 237, 0.1);
      color: #7c3aed;
      padding: 1px 4px;
      border-radius: 3px;
      font-weight: 600;
      margin-right: 4px;
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
    .endorsement-onchain {{
      display: inline-block;
      font-size: 10px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.5px;
      padding: 2px 8px;
      border-radius: 4px;
      background: rgba(22, 163, 74, 0.1);
      color: #16a34a;
      text-decoration: none;
      margin-left: 4px;
    }}
    .endorsement-onchain:hover {{
      background: rgba(22, 163, 74, 0.2);
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
    .install-cta-empty {{
      display: inline-block;
      margin-top: 8px;
    }}
    .install-cta {{
      background: #fff;
      border: 1px solid #e5e5e0;
      border-radius: 12px;
      padding: 24px;
      margin-bottom: 24px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
    }}
    .install-cta-title {{
      font-size: 16px;
      font-weight: 700;
      color: #1a1a2e;
      margin-bottom: 4px;
    }}
    .install-cta-subtitle {{
      font-size: 13px;
      font-weight: 400;
      color: #666;
    }}
    .install-cta-btn {{
      display: inline-block;
      background: #1a1a2e;
      color: #fff;
      padding: 10px 16px;
      border-radius: 6px;
      font-size: 13px;
      font-family: 'Geist', -apple-system, BlinkMacSystemFont, sans-serif;
      font-weight: 600;
      text-decoration: none;
      white-space: nowrap;
    }}
    .install-cta-btn:hover {{
      opacity: 0.9;
    }}
    a:focus-visible, button:focus-visible {{
      outline: 2px solid #16a34a;
      outline-offset: 2px;
      border-radius: 2px;
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
    .badge-code-wrapper {{
      display: flex;
      align-items: flex-start;
      gap: 8px;
    }}
    .badge-code {{
      font-family: 'JetBrains Mono', monospace;
      font-size: 12px;
      background: #f5f5f0;
      padding: 12px;
      border-radius: 6px;
      overflow-x: auto;
      color: #666;
      flex: 1;
    }}
    .copy-btn {{
      background: #1a1a2e;
      color: #fff;
      border: none;
      padding: 8px 12px;
      border-radius: 6px;
      font-size: 12px;
      font-family: 'Geist', -apple-system, BlinkMacSystemFont, sans-serif;
      font-weight: 600;
      cursor: pointer;
      white-space: nowrap;
    }}
    .copy-btn:hover {{
      opacity: 0.9;
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
    @media (max-width: 375px) {{
      .container {{ padding: 24px 16px; }}
      .hero {{ gap: 16px; }}
      .score-circle {{ width: 56px; height: 56px; }}
      .score-number {{ font-size: 22px; }}
      .signals {{ grid-template-columns: 1fr 1fr; }}
      .breakdown {{ grid-template-columns: 1fr; }}
      .install-cta {{ flex-direction: column; align-items: stretch; }}
      .install-cta-btn {{ text-align: center; width: 100%; }}
    }}
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <span>commit</span>
      <span class="sep">/</span>
      <a href="/trust/github/{owner}/{repo}">github/{owner}/{repo}</a>
    </div>

    <style>
      @keyframes score-fill {{
        from {{ stroke-dashoffset: {circumference}; }}
        to {{ stroke-dashoffset: {dash_offset}; }}
      }}
    </style>
    <div class="hero">
      <div class="score-circle">
        <svg viewBox="0 0 72 72">
          <circle class="score-track" cx="36" cy="36" r="{radius}" />
          <circle class="score-arc" cx="36" cy="36" r="{radius}" stroke="{color_start}" stroke-dasharray="{circumference}" stroke-dashoffset="{dash_offset}" />
        </svg>
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

    <div class="install-cta">
      <div class="install-cta-text">
        <div class="install-cta-title">Endorse this repo</div>
        <div class="install-cta-subtitle">Add ZK-verified commitment signals with one click &mdash; on GitHub, Google, and everywhere you already browse.</div>
      </div>
      <a href="{cta_url}" target="_blank" rel="noopener" class="install-cta-btn">Get the Commit extension</a>
    </div>

    <div class="badge-section">
      <div class="card-title">Add badge to README</div>
      <div class="badge-preview">
        <img src="/badge/github/{owner}/{repo}.svg" alt="Commit Score" height="20">
      </div>
      <div class="badge-code-wrapper">
        <div class="badge-code" id="badge-code">[![Commit Score]({public_url}/badge/github/{owner}/{repo}.svg)]({public_url}/trust/github/{owner}/{repo})</div>
        <button type="button" class="copy-btn" data-copy-target="badge-code">Copy</button>
      </div>
    </div>

    <div class="footer">
      <span>Commit Score (beta) — based on public data. Endorse this repo to improve accuracy.</span>
      <a href="https://github.com/getcommit-dev/commit">GitHub</a>
    </div>
  </div>
  <script>
    document.querySelectorAll('[data-copy-target]').forEach(function(btn) {{
      btn.addEventListener('click', function() {{
        var targetId = btn.getAttribute('data-copy-target');
        var el = document.getElementById(targetId);
        if (!el) return;
        var text = el.textContent;
        var orig = btn.textContent;
        function done() {{ btn.textContent = 'Copied!'; setTimeout(function() {{ btn.textContent = orig; }}, 1500); }}
        if (navigator.clipboard && navigator.clipboard.writeText) {{
          navigator.clipboard.writeText(text).then(done).catch(function() {{
            fallbackCopy(text); done();
          }});
        }} else {{
          fallbackCopy(text); done();
        }}
      }});
    }});
    function fallbackCopy(text) {{
      var ta = document.createElement('textarea');
      ta.value = text; ta.style.position = 'fixed'; ta.style.left = '-9999px';
      document.body.appendChild(ta); ta.select();
      try {{ document.execCommand('copy'); }} catch(e) {{}}
      document.body.removeChild(ta);
    }}
  </script>
</body>
</html>"#
    )
}

fn render_endorsements_section(count: u32, endorsements: &[EndorsementSummary]) -> String {
    let title = if count > 0 {
        format!("Endorsements ({count})")
    } else {
        "Endorsements".to_string()
    };

    let cta_url = CHROME_WEBSTORE_URL;
    let body = if endorsements.is_empty() {
        format!(
            r#"<div class="endorsement-empty">
        <p>No endorsements yet.</p>
        <a href="{cta_url}" target="_blank" rel="noopener" class="install-cta-btn install-cta-empty">Install the Commit extension to endorse</a>
      </div>"#
        )
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
                let on_chain_tag = if e.on_chain {
                    if let Some(ref hash) = e.tx_hash {
                        format!(
                            r#" <a href="https://sepolia.basescan.org/tx/{}" class="endorsement-onchain" rel="noopener" target="_blank">On-chain</a>"#,
                            html_escape(hash)
                        )
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                format!(
                    r#"<div class="endorsement-row">
          <div class="endorsement-info">
            <span class="endorsement-category">{}</span>
            <span class="endorsement-proof">{}</span>
          </div>
          <div>
            <span class="endorsement-status {status_class}">{status_label}</span>{on_chain_tag}
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

fn render_breakdown(b: &ScoreBreakdown, layer1_only: bool) -> String {
    let l1_items = [
        ("Longevity", b.longevity, 15.0),
        ("Maintenance", b.maintenance, 10.0),
        ("Community", b.community, 10.0),
        ("Financial", b.financial, 5.0),
    ];

    let l1_html: String = l1_items
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

    if layer1_only {
        return format!(
            r#"<div class="breakdown-section-label">Public Signals</div>
      <div class="breakdown">{l1_html}</div>"#
        );
    }

    // Layer 2 items — only include non-zero fields (skip network_density when 0)
    let mut l2_items: Vec<(&str, f64, f64)> = vec![
        ("Endorsements", b.endorsements, 30.0),
        ("Proof Strength", b.proof_strength, 15.0),
        ("Tenure", b.tenure, 10.0),
    ];
    if b.network_density > 0.0 {
        l2_items.push(("Network Density", b.network_density, 15.0));
    }

    let l2_html: String = l2_items
        .iter()
        .map(|(label, val, max)| {
            format!(
                r#"<div class="breakdown-item">
          <span class="breakdown-label breakdown-label--zk"><span class="breakdown-zk-tag">ZK</span> {label}</span>
          <span class="breakdown-value">{val:.1} / {max:.0}</span>
        </div>"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ");

    format!(
        r#"<div class="breakdown-section-label">Public Signals</div>
      <div class="breakdown">{l1_html}</div>
      <div class="breakdown-section-label breakdown-section-label--zk">ZK Endorsement Signals</div>
      <div class="breakdown">{l2_html}</div>"#
    )
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
