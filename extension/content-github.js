// Commit Trust Card — GitHub content script
// Injects trust cards on github.com repo pages

const API_BASE = "https://commit-backend.fly.dev";
const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour

async function injectTrustCard() {
  const repoId = getRepoIdentifier();
  if (!repoId) return;

  const container = findInjectionPoint();
  if (!container) return;
  if (document.querySelector(".commit-trust-card")) return; // Already injected

  // Show skeleton loading state
  const skeleton = createSkeleton();
  container.appendChild(skeleton);

  try {
    const data = await fetchTrustCard("github", repoId);
    if (!data || !data.score.score) {
      skeleton.remove(); // No data = no card
      return;
    }
    skeleton.replaceWith(createTrustCard(data));
  } catch {
    skeleton.remove(); // Error = no card (degraded mode)
  }
}

function getRepoIdentifier() {
  const path = window.location.pathname.split("/").filter(Boolean);
  if (path.length < 2) return null;
  // Only inject on repo root, not on files/issues/etc
  if (path.length > 2 && !["tree", "blob", "pulls", "issues", "actions"].includes(path[2])) return null;
  return `${path[0]}/${path[1]}`;
}

function findInjectionPoint() {
  // GitHub repo page: inject after the repo description
  return document.querySelector("[class*='BorderGrid-row']:first-child .BorderGrid-cell") ||
         document.querySelector(".repository-content") ||
         document.querySelector("#repo-content-pjax-container");
}

function createSkeleton() {
  const el = document.createElement("div");
  el.className = "commit-trust-card commit-trust-card--dark commit-trust-card--loading";
  el.setAttribute("aria-label", "Loading Commit Score...");
  return el;
}

function createTrustCard(data) {
  const { subject, score } = data;
  const scoreValue = score.score;
  const tier = scoreValue > 70 ? "high" : scoreValue > 40 ? "mid" : "none";

  const card = document.createElement("div");
  card.className = "commit-trust-card commit-trust-card--dark";

  const circle = document.createElement("div");
  circle.className = `commit-score-circle commit-score-circle--${tier}`;
  circle.textContent = scoreValue;
  circle.setAttribute("aria-label", `Commit Score ${scoreValue} out of 100`);
  circle.style.cursor = "pointer";
  circle.addEventListener("click", () => {
    window.open(`https://commit-backend.fly.dev/trust/github/${subject.identifier}`, "_blank");
  });

  const details = document.createElement("div");
  details.className = "commit-card-details";
  details.innerHTML = `
    <div class="commit-card-label">Commit Score</div>
    <div class="commit-card-signals">${formatSignals(score.breakdown)}</div>
  `;

  card.appendChild(circle);
  card.appendChild(details);
  return card;
}

function formatSignals(breakdown) {
  const parts = [];
  if (breakdown.longevity > 0) parts.push(`${Math.round(breakdown.longevity / 3)}yr active`);
  if (breakdown.community > 0) parts.push(`${Math.round(breakdown.community / 0.5)} contributors`);
  if (breakdown.maintenance > 0) parts.push(breakdown.maintenance > 8 ? "actively maintained" : "maintained");
  return parts.join(" · ") || "Public data";
}

async function fetchTrustCard(kind, id) {
  // Check extension-side cache first
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

// Run on page load and GitHub SPA navigation
injectTrustCard();
new MutationObserver(() => injectTrustCard()).observe(document.body, { childList: true, subtree: true });
