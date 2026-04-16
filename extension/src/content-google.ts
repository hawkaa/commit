// Commit Trust Card — Google SERP content script
// Injects compact trust cards next to search results that match known subjects
// Note: "Not for me" negative endorsement is intentionally NOT shown on SERP cards.
// Too little context on SERP for a negative signal (CEO decision, Phase 3).
import { API_BASE, CACHE_TTL_MS } from "./config";
import { getEndorsement } from "./endorsed-cache";

interface SerpTrustCardData {
  subject: { identifier: string };
  score: { score: number | null };
}

async function injectSerp(): Promise<void> {
  const results = document.querySelectorAll("#search .g");
  for (const result of results) {
    if (result.querySelector(".commit-trust-card")) continue;

    const link = result.querySelector("a[href]") as HTMLAnchorElement | null;
    if (!link) continue;

    const repoId = extractGithubRepo(link.href);
    if (!repoId) continue;

    try {
      const data = await fetchTrustCard("github", repoId);
      if (!data || !data.score.score) continue;

      // Read endorsed-subjects cache (best-effort — null on failure)
      let cached: import("./endorsed-cache").EndorsedEntry | null = null;
      try {
        cached = await getEndorsement("github", repoId);
      } catch {
        // Cache read failure is a silent miss
      }
      const card = createSerpCard(data, cached);
      const snippet = result.querySelector("[data-sncf], .VwiC3b, .IsZvec");
      if (snippet && snippet.parentNode) {
        snippet.parentNode.insertBefore(card, snippet.nextSibling);
      }
    } catch {
      // Degraded mode: no card on error
    }
  }
}

function extractGithubRepo(url: string): string | null {
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

function createSerpCard(
  data: SerpTrustCardData,
  cachedEndorsement: import("./endorsed-cache").EndorsedEntry | null
): HTMLDivElement {
  const { subject, score } = data;
  const scoreValue = score.score!;
  const tier = scoreValue > 70 ? "high" : scoreValue > 40 ? "mid" : "none";

  const card = document.createElement("div");
  card.className = "commit-trust-card commit-trust-card--light";
  card.style.marginTop = "8px";

  const circle = document.createElement("div");
  circle.className = `commit-score-circle commit-score-circle--compact commit-score-circle--${tier}`;
  circle.textContent = String(scoreValue);
  circle.style.cursor = "pointer";
  circle.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    window.open(
      `${API_BASE}/trust/github/${subject.identifier}`,
      "_blank"
    );
  });

  const meta = document.createElement("span");
  meta.style.cssText = "font-size: 11px; color: #70757a; margin-left: 8px;";

  const strong = document.createElement("strong");
  strong.style.color = "#1a1a2e";
  strong.textContent = `Commit Score ${scoreValue}`;
  meta.appendChild(strong);

  // Show muted revisit indicator next to score if cached
  if (cachedEndorsement) {
    const indicator = document.createElement("span");
    indicator.className = "commit-endorse-indicator";
    indicator.style.cssText = "font-size: 10px; color: #888; margin-left: 8px;";
    indicator.textContent =
      cachedEndorsement.sentiment === "positive"
        ? "Endorsed \u2713"
        : "Not for me \u2713";
    meta.appendChild(indicator);
  }

  card.appendChild(circle);
  card.appendChild(meta);
  return card;
}

async function fetchTrustCard(
  kind: string,
  id: string
): Promise<SerpTrustCardData | null> {
  const cacheKey = `trust-card:${kind}:${id}`;
  const cached = await chrome.storage.local.get(cacheKey);
  if (cached[cacheKey]) {
    const { data, timestamp } = cached[cacheKey] as {
      data: SerpTrustCardData;
      timestamp: number;
    };
    if (Date.now() - timestamp < CACHE_TTL_MS) return data;
  }

  const resp = await fetch(
    `${API_BASE}/trust-card?kind=${kind}&id=${encodeURIComponent(id)}`
  );
  if (!resp.ok) return null;

  const data: SerpTrustCardData = await resp.json();
  await chrome.storage.local.set({
    [cacheKey]: { data, timestamp: Date.now() },
  });
  return data;
}

injectSerp();
