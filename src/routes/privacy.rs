use axum::http::{HeaderMap, StatusCode, header};

#[allow(clippy::missing_errors_doc)]
pub async fn get_privacy_page() -> Result<(StatusCode, HeaderMap, String), StatusCode> {
    let html = render_html();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=86400".parse().unwrap(),
    );
    Ok((StatusCode::OK, headers, html))
}

fn render_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Privacy Policy — Commit</title>
  <meta name="description" content="Privacy policy for the Commit browser extension.">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
  <style>
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      font-family: 'Geist', -apple-system, BlinkMacSystemFont, sans-serif;
      background: #f5f5f0;
      color: #1a1a2e;
      line-height: 1.6;
      -webkit-font-smoothing: antialiased;
    }
    .container {
      max-width: 680px;
      margin: 0 auto;
      padding: 48px 32px;
    }
    .header {
      display: flex;
      align-items: center;
      gap: 8px;
      margin-bottom: 48px;
    }
    .header a {
      color: #666;
      text-decoration: none;
      font-size: 13px;
      font-weight: 500;
    }
    .header a:hover { color: #1a1a2e; }
    .header .sep { color: #ccc; }
    h1 {
      font-size: 20px;
      font-weight: 700;
      margin-bottom: 8px;
    }
    .updated {
      font-size: 13px;
      color: #888;
      margin-bottom: 32px;
    }
    h2 {
      font-size: 16px;
      font-weight: 600;
      margin-top: 32px;
      margin-bottom: 12px;
    }
    p, ul {
      font-size: 14px;
      color: #444;
      margin-bottom: 16px;
    }
    ul {
      padding-left: 20px;
    }
    li {
      margin-bottom: 8px;
    }
    code {
      font-family: 'JetBrains Mono', monospace;
      font-size: 13px;
      background: #e5e5e0;
      padding: 1px 5px;
      border-radius: 3px;
    }
    .footer {
      margin-top: 48px;
      padding-top: 24px;
      border-top: 1px solid #e5e5e0;
      font-size: 13px;
      color: #888;
      display: flex;
      justify-content: space-between;
    }
    .footer a {
      color: #666;
      text-decoration: none;
    }
    .footer a:hover { color: #1a1a2e; }
    @media (max-width: 480px) {
      .container { padding: 24px 16px; }
    }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <a href="/">commit</a>
      <span class="sep">/</span>
      <a href="/privacy">privacy</a>
    </div>

    <h1>Privacy Policy</h1>
    <p class="updated">Last updated: April 10, 2026</p>

    <h2>What Commit does</h2>
    <p>
      Commit is a browser extension that displays verifiable trust scores for
      GitHub repositories, businesses, and services. It fetches publicly
      available data and shows a Commit Score alongside the pages you visit.
    </p>

    <h2>Data we collect</h2>
    <p>The extension itself collects <strong>no personal data</strong>. Specifically:</p>
    <ul>
      <li><strong>No accounts.</strong> There is no sign-up, login, or user profile.</li>
      <li><strong>No tracking.</strong> No analytics, telemetry, or usage tracking of any kind.</li>
      <li><strong>No cookies.</strong> The extension does not set or read cookies.</li>
    </ul>

    <h2>What is stored locally</h2>
    <ul>
      <li>
        <strong>Ed25519 keypair</strong> — generated on install and stored in
        <code>chrome.storage.local</code>. Used to sign endorsements. Never
        transmitted unless you explicitly endorse a repository.
      </li>
      <li>
        <strong>Trust card cache</strong> — API responses are cached locally
        for up to 1 hour to reduce network requests. Cache is automatically
        cleaned up.
      </li>
    </ul>

    <h2>Network requests</h2>
    <p>
      The extension makes requests to <code>commit-backend.fly.dev</code> to
      fetch trust card data for repositories you visit on GitHub and GitHub
      links that appear in Google search results. These requests contain only
      the repository identifier (e.g. <code>owner/repo</code>). No personal
      information is included.
    </p>

    <h2>Endorsements</h2>
    <p>
      If you choose to endorse a repository, your public key and a signed
      endorsement message are sent to the Commit backend. This is a deliberate
      action — no data is sent without your explicit interaction.
    </p>

    <h2>Third-party services</h2>
    <p>
      The Commit backend uses the GitHub API to fetch public repository
      metadata (stars, contributors, commit history). No user-specific GitHub
      data is accessed.
    </p>

    <h2>Changes to this policy</h2>
    <p>
      If this policy changes, the updated version will be posted at this URL.
      The extension does not auto-update its privacy policy — you can always
      check the current version here.
    </p>

    <h2>Contact</h2>
    <p>
      Questions about this policy can be directed to the
      <a href="https://github.com/hawkaa/commit">Commit GitHub repository</a>.
    </p>

    <div class="footer">
      <span>Commit (beta)</span>
      <a href="https://github.com/hawkaa/commit">GitHub</a>
    </div>
  </div>
</body>
</html>"#
        .to_string()
}
