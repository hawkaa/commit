// Commit Trust Card — Google SERP content script
// Injects compact trust cards next to search results that match known subjects

const API_BASE = "http://localhost:3000";
const CACHE_TTL_MS = 60 * 60 * 1000;

async function injectSerp() {
  const results = document.querySelectorAll("#search .g");
  for (const result of results) {
    if (result.querySelector(".commit-trust-card")) continue;

    const link = result.querySelector("a[href]");
    if (!link) continue;

    const repoId = extractGithubRepo(link.href);
    if (!repoId) continue;

    try {
      const data = await fetchTrustCard("github", repoId);
      if (!data || !data.score.score) continue;

      const card = createSerpCard(data);
      const snippet = result.querySelector("[data-sncf], .VwiC3b, .IsZvec");
      if (snippet) {
        snippet.parentNode.insertBefore(card, snippet.nextSibling);
      }
    } catch {
      // Degraded mode: no card on error
    }
  }
}

function extractGithubRepo(url) {
  try {
    const u = new URL(url);
    if (!u.hostname.includes("github.com")) return null;
    const parts = u.pathname.split("/").filter(Boolean);
    if (parts.length < 2) return null;
    return `${parts[0]}/${parts[1]}`;
  } catch {
    return null;
  }
}

function createSerpCard(data) {
  const { subject, score } = data;
  const scoreValue = score.score;
  const tier = scoreValue > 70 ? "high" : scoreValue > 40 ? "mid" : "none";

  const card = document.createElement("div");
  card.className = "commit-trust-card commit-trust-card--light";
  card.style.marginTop = "8px";

  const circle = document.createElement("div");
  circle.className = `commit-score-circle commit-score-circle--compact commit-score-circle--${tier}`;
  circle.textContent = scoreValue;
  circle.style.cursor = "pointer";
  circle.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    window.open(`https://commit.dev/trust/github/${subject.identifier}`, "_blank");
  });

  const meta = document.createElement("span");
  meta.style.cssText = "font-size: 11px; color: #70757a; margin-left: 8px;";
  meta.innerHTML = `<strong style="color: #1a1a2e;">Commit Score ${scoreValue}</strong>`;

  card.appendChild(circle);
  card.appendChild(meta);
  return card;
}

async function fetchTrustCard(kind, id) {
  const cacheKey = `trust-card:${kind}:${id}`;
  const cached = await chrome.storage.local.get(cacheKey);
  if (cached[cacheKey]) {
    const { data, timestamp } = cached[cacheKey];
    if (Date.now() - timestamp < CACHE_TTL_MS) return data;
  }

  const resp = await fetch(`${API_BASE}/trust-card?kind=${kind}&id=${encodeURIComponent(id)}`);
  if (!resp.ok) return null;

  const data = await resp.json();
  await chrome.storage.local.set({ [cacheKey]: { data, timestamp: Date.now() } });
  return data;
}

injectSerp();
