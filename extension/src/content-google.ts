// Commit Trust Card — Google SERP content script
// Injects compact trust cards next to search results that match known subjects
// Note: "Not for me" negative endorsement is intentionally NOT shown on SERP cards.
// Too little context on SERP for a negative signal (CEO decision, Phase 3).
import { API_BASE, CACHE_TTL_MS } from "./config";
import { getEndorsement, type EndorsedEntry } from "./endorsed-cache";

interface SerpTrustCardData {
  subject: { identifier: string };
  score: { score: number | null };
  endorsement_count?: number; // optional for stale cache compat
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
      let cached: EndorsedEntry | null = null;
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
  cachedEndorsement: EndorsedEntry | null
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

  // Show endorsement count inline after score
  const endorsementCount = data.endorsement_count ?? 0;
  if (endorsementCount > 0) {
    const countSpan = document.createElement("span");
    countSpan.style.cssText = "margin-left: 6px;";
    countSpan.textContent = `· ${endorsementCount} endorsement${endorsementCount === 1 ? "" : "s"}`;
    meta.appendChild(countSpan);
  }

  // Compact endorse action: text link (positive only — no "Not for me" on SERP)
  // Also serves as revisit indicator when endorsement is cached
  const endorseBtn = document.createElement("button");
  endorseBtn.className = "commit-endorse-btn commit-endorse-btn--compact";
  endorseBtn.style.cssText =
    "background: none; border: none; font-size: 11px; cursor: pointer; margin-left: 8px; padding: 0; color: #7c3aed; font-family: inherit;";

  if (cachedEndorsement?.sentiment === "positive") {
    endorseBtn.textContent = "Endorsed \u2713";
    endorseBtn.classList.add("commit-endorse-indicator");
    endorseBtn.style.color = "#888";
    endorseBtn.style.cursor = "default";
    endorseBtn.disabled = true;
  } else if (cachedEndorsement?.sentiment === "negative") {
    // Read-only indicator: negative sentiment is set from the GitHub card (SERP has no
    // "Not for me" button per CEO decision). The shared endorsed-cache means a negative
    // endorsement made on GitHub surfaces here as a muted indicator.
    endorseBtn.textContent = "Not for me \u2713";
    endorseBtn.classList.add("commit-endorse-indicator");
    endorseBtn.style.color = "#888";
    endorseBtn.style.cursor = "default";
    endorseBtn.disabled = true;
  } else {
    endorseBtn.textContent = "Endorse";
    endorseBtn.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      startSerpEndorsement(subject.identifier, endorseBtn);
    });
  }

  card.appendChild(circle);
  card.appendChild(meta);
  card.appendChild(endorseBtn);
  return card;
}

interface EndorsementResult {
  success: boolean;
  errorCode?: string;
  error?: string;
}

async function startSerpEndorsement(
  repoId: string,
  btn: HTMLButtonElement
): Promise<void> {
  const [owner, name] = repoId.split("/");
  btn.disabled = true;
  btn.textContent = "Proving...";
  btn.style.color = "#7c3aed";

  try {
    const result = (await chrome.runtime.sendMessage({
      type: "START_ENDORSEMENT",
      repoOwner: owner,
      repoName: name,
      sentiment: "positive",
    })) as EndorsementResult | undefined;

    if (result?.success) {
      // Permanent disabled state — SERP has no sentiment flip, so re-endorsing is not useful
      btn.textContent = "Endorsed \u2713";
      btn.style.color = "#888";
      btn.style.cursor = "default";
      btn.classList.add("commit-endorse-indicator");

      // Clear trust card cache so next page load fetches fresh count
      try {
        const cacheKey = `trust-card:github:${repoId}`;
        await chrome.storage.local.remove(cacheKey);
      } catch {
        // Cache clear failure is non-critical — entry expires via TTL
      }
    } else {
      const label = errorCodeToLabel(result?.errorCode);
      resetSerpBtn(btn, label);
    }
  } catch {
    resetSerpBtn(btn, "Offline");
  }
}

function resetSerpBtn(btn: HTMLButtonElement, label: string): void {
  btn.textContent = label;
  btn.style.color = "#dc2626";
  setTimeout(() => {
    if (!btn.isConnected) return;
    btn.textContent = "Endorse";
    btn.style.color = "#7c3aed";
    btn.disabled = false;
  }, 3000);
}

function errorCodeToLabel(code?: string): string {
  switch (code) {
    case "notary_offline":
      return "Notary offline";
    case "timeout":
      return "Timed out";
    case "duplicate":
      return "Already endorsed";
    case "network":
      return "Offline";
    case "backend_error":
    case "prove_failed":
    default:
      return "Failed";
  }
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
