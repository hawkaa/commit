// Commit Trust Card — GitHub content script
// Injects trust cards on github.com repo pages
import { API_BASE, CACHE_TTL_MS } from "./config";

interface EndorsementSummary {
  id: string;
  category: string;
  proof_type: string;
  status: string;
  created_at: string;
}

interface NetworkData {
  network: number;
  total: number;
}

interface TrustCardData {
  subject: { identifier: string; display_name: string };
  score: {
    score: number | null;
    breakdown: { longevity: number; community: number; maintenance: number };
  };
  endorsement_count: number;
  recent_endorsements: EndorsementSummary[];
  network_data?: NetworkData | null;
}

async function injectTrustCard(): Promise<void> {
  const repoId = getRepoIdentifier();
  if (!repoId) return;

  const container = findInjectionPoint();
  if (!container) return;
  if (document.querySelector(".commit-trust-card")) return;

  const skeleton = createSkeleton();
  container.appendChild(skeleton);

  try {
    const data = await fetchTrustCard("github", repoId);
    if (!data || !data.score.score) {
      skeleton.remove();
      return;
    }
    skeleton.replaceWith(createTrustCard(data));
  } catch {
    skeleton.remove();
  }
}

function getRepoIdentifier(): string | null {
  const path = window.location.pathname.split("/").filter(Boolean);
  if (path.length < 2) return null;
  if (
    path.length > 2 &&
    !["tree", "blob", "pulls", "issues", "actions"].includes(path[2])
  )
    return null;
  return `${path[0]}/${path[1]}`;
}

function findInjectionPoint(): Element | null {
  return (
    document.querySelector(
      "[class*='BorderGrid-row']:first-child .BorderGrid-cell"
    ) ||
    document.querySelector(".repository-content") ||
    document.querySelector("#repo-content-pjax-container")
  );
}

function createSkeleton(): HTMLDivElement {
  const el = document.createElement("div");
  el.className =
    "commit-trust-card commit-trust-card--dark commit-trust-card--loading";
  el.setAttribute("aria-label", "Loading Commit Score...");
  return el;
}

function createTrustCard(data: TrustCardData): HTMLDivElement {
  const { subject, score } = data;
  const scoreValue = score.score!;
  const tier = scoreValue > 70 ? "high" : scoreValue > 40 ? "mid" : "none";

  const card = document.createElement("div");
  card.className = "commit-trust-card commit-trust-card--dark";

  const circle = document.createElement("div");
  circle.className = `commit-score-circle commit-score-circle--${tier}`;
  circle.textContent = String(scoreValue);
  circle.setAttribute("aria-label", `Commit Score ${scoreValue} out of 100`);
  circle.style.cursor = "pointer";
  circle.addEventListener("click", () => {
    window.open(
      `${API_BASE}/trust/github/${subject.identifier}`,
      "_blank"
    );
  });

  const details = document.createElement("div");
  details.className = "commit-card-details";

  const label = document.createElement("div");
  label.className = "commit-card-label";
  label.textContent = "Commit Score";

  const signals = document.createElement("div");
  signals.className = "commit-card-signals";
  signals.textContent = formatSignals(score.breakdown);

  details.appendChild(label);
  details.appendChild(signals);

  if (data.endorsement_count > 0) {
    const network = document.createElement("div");
    network.className = "commit-card-network";
    network.textContent = `${data.endorsement_count} ZK endorsement${data.endorsement_count === 1 ? "" : "s"}`;
    details.appendChild(network);
  }

  const endorseBtn = document.createElement("button");
  endorseBtn.className = "commit-endorse-btn";
  endorseBtn.textContent = "Endorse";
  endorseBtn.title = "Create a ZK-verified endorsement for this repo";
  endorseBtn.addEventListener("click", () =>
    startEndorsement(subject.identifier, endorseBtn)
  );

  card.appendChild(circle);
  card.appendChild(details);
  card.appendChild(endorseBtn);
  return card;
}

async function startEndorsement(
  repoId: string,
  btn: HTMLButtonElement
): Promise<void> {
  const [owner, name] = repoId.split("/");
  btn.disabled = true;
  btn.textContent = "Proving...";
  btn.classList.add("commit-endorse-btn--active");

  try {
    const result = await chrome.runtime.sendMessage({
      type: "START_ENDORSEMENT",
      repoOwner: owner,
      repoName: name,
    });

    if (result.success) {
      btn.textContent = "Endorsed";
      btn.classList.remove("commit-endorse-btn--active");
      btn.classList.add("commit-endorse-btn--done");

      // Optimistically increment displayed count
      const card = btn.closest(".commit-trust-card");
      if (card) {
        let networkEl = card.querySelector(".commit-card-network");
        if (networkEl) {
          const match = networkEl.textContent?.match(/^(\d+)/);
          const current = match ? parseInt(match[1], 10) : 0;
          const next = current + 1;
          networkEl.textContent = `${next} ZK endorsement${next === 1 ? "" : "s"}`;
        } else {
          networkEl = document.createElement("div");
          networkEl.className = "commit-card-network";
          networkEl.textContent = "1 ZK endorsement";
          card.querySelector(".commit-card-details")?.appendChild(networkEl);
        }
      }

      // Clear trust card cache for this repo
      const cacheKey = `trust-card:github:${repoId}`;
      await chrome.storage.local.remove(cacheKey);

      // Reset button after 3s to allow re-endorsement
      setTimeout(() => {
        btn.textContent = "Endorse";
        btn.classList.remove("commit-endorse-btn--done");
        btn.disabled = false;
      }, 3000);
    } else {
      console.error("[commit] Endorsement failed:", result.error);
      const label = errorCodeToLabel(result.errorCode);
      resetEndorseButton(btn, label);
    }
  } catch (err) {
    console.error("[commit] Endorsement error:", err);
    resetEndorseButton(btn, "Offline");
  }
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

function resetEndorseButton(btn: HTMLButtonElement, label: string): void {
  btn.textContent = label;
  btn.disabled = false;
  setTimeout(() => {
    btn.textContent = "Endorse";
    btn.classList.remove("commit-endorse-btn--active");
  }, 3000);
}

function formatSignals(breakdown: TrustCardData["score"]["breakdown"]): string {
  const parts: string[] = [];
  if (breakdown.longevity > 0)
    parts.push(`${Math.round(breakdown.longevity / 3)}yr active`);
  if (breakdown.community > 0)
    parts.push(`${Math.round(breakdown.community / 0.5)} contributors`);
  if (breakdown.maintenance > 0)
    parts.push(
      breakdown.maintenance > 8 ? "actively maintained" : "maintained"
    );
  return parts.join(" · ") || "Public data";
}

async function fetchTrustCard(
  kind: string,
  id: string
): Promise<TrustCardData | null> {
  const cacheKey = `trust-card:${kind}:${id}`;
  const cached = await chrome.storage.local.get(cacheKey);
  let data: TrustCardData | null = null;

  if (cached[cacheKey]) {
    const entry = cached[cacheKey] as {
      data: TrustCardData;
      timestamp: number;
    };
    if (Date.now() - entry.timestamp < CACHE_TTL_MS) {
      data = entry.data;
    }
  }

  if (!data) {
    const resp = await fetch(
      `${API_BASE}/trust-card?kind=${kind}&id=${encodeURIComponent(id)}`
    );
    if (!resp.ok) return null;

    data = (await resp.json()) as TrustCardData;
    await chrome.storage.local.set({
      [cacheKey]: { data, timestamp: Date.now() },
    });
  }

  // Query network endorsements fresh every time (not cached — depends on user's keyring)
  try {
    const networkData = await chrome.runtime.sendMessage({
      type: "NETWORK_QUERY",
      subjectKind: kind,
      subjectId: id,
    });
    if (networkData && data) {
      data.network_data = networkData as NetworkData;
    }
  } catch {
    // Network query is non-critical; proceed without it
  }

  return data;
}

// Run on page load and GitHub SPA navigation
injectTrustCard();
new MutationObserver(() => injectTrustCard()).observe(document.body, {
  childList: true,
  subtree: true,
});
