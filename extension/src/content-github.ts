// Commit Trust Card — GitHub content script
// Injects trust cards on github.com repo pages
import { API_BASE, CACHE_TTL_MS } from "./config";
import { getEndorsement, type EndorsedEntry } from "./endorsed-cache";

interface EndorsementSummary {
  id: string;
  category: string;
  proof_type: string;
  status: string;
  created_at: string;
}

interface TrustCardData {
  subject: { identifier: string; display_name: string };
  score: {
    score: number | null;
    layer1_only: boolean;
    breakdown: {
      longevity: number;
      community: number;
      maintenance: number;
      endorsements: number;
      proof_strength: number;
      tenure: number;
      network_density: number;
    };
  };
  endorsement_count: number;
  recent_endorsements: EndorsementSummary[];
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
    // Read endorsed-subjects cache (best-effort — null on failure)
    let cached: EndorsedEntry | null = null;
    try {
      cached = await getEndorsement("github", repoId);
    } catch {
      // Cache read failure is a silent miss
    }
    skeleton.replaceWith(createTrustCard(data, cached));
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

function createTrustCard(data: TrustCardData, cachedEndorsement: EndorsedEntry | null): HTMLDivElement {
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
  label.textContent = score.layer1_only === false ? "Commit Score · Public + ZK" : "Commit Score";

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

  if (score.layer1_only === false) {
    const zkLine = document.createElement("div");
    zkLine.className = "commit-card-network";
    const text = document.createTextNode("Score includes ZK endorsements ");
    const tag = document.createElement("span");
    tag.className = "commit-zk-tag";
    tag.textContent = "ZK";
    zkLine.appendChild(text);
    zkLine.appendChild(tag);
    details.appendChild(zkLine);
  }

  // "Add badge" clipboard CTA
  const snippet = `[![Commit Score](${API_BASE}/badge/github/${subject.identifier}.svg)](${API_BASE}/trust/github/${subject.identifier})`;
  const addBadge = document.createElement("span");
  addBadge.className = "commit-add-badge";
  addBadge.textContent = "Add badge";
  addBadge.title = "Copy badge markdown to clipboard";
  let fallbackEl: HTMLElement | null = null;
  let isCopying = false;
  addBadge.addEventListener("click", async () => {
    if (isCopying) return;
    isCopying = true;
    try {
      await navigator.clipboard.writeText(snippet);
      if (fallbackEl) fallbackEl.style.display = "none";
      addBadge.textContent = "Copied!";
      addBadge.classList.add("commit-add-badge--done");
      setTimeout(() => {
        addBadge.textContent = "Add badge";
        addBadge.classList.remove("commit-add-badge--done");
        isCopying = false;
      }, 1500);
    } catch {
      // Fallback: show selectable snippet inline (re-entrant safe)
      if (!fallbackEl) {
        fallbackEl = document.createElement("code");
        fallbackEl.className = "commit-badge-snippet";
        fallbackEl.textContent = snippet;
        addBadge.parentElement?.appendChild(fallbackEl);
      }
      fallbackEl.style.display = "block";
      isCopying = false;
    }
  });
  details.appendChild(addBadge);

  // Endorsement action row: "Endorse" (primary) + "Not for me" (subdued secondary)
  // If the user previously endorsed this subject, show a muted indicator on the
  // active-sentiment side and keep the opposite button interactive for flipping.
  const actionRow = document.createElement("div");
  actionRow.className = "commit-endorse-row";

  const endorseBtn = document.createElement("button");
  endorseBtn.className = "commit-endorse-btn";
  endorseBtn.title = "Create a ZK-verified endorsement for this repo";

  const notForMeBtn = document.createElement("button");
  notForMeBtn.className = "commit-endorse-secondary";
  notForMeBtn.title = "Signal that this repo is not recommended";

  if (cachedEndorsement?.sentiment === "positive") {
    // Muted indicator on primary; secondary stays active for flipping
    endorseBtn.textContent = "Endorsed \u2713";
    endorseBtn.classList.add("commit-endorse-indicator");
    endorseBtn.disabled = true;
    notForMeBtn.textContent = "Not for me";
    notForMeBtn.addEventListener("click", () =>
      startEndorsement(subject.identifier, "negative", notForMeBtn, endorseBtn)
    );
  } else if (cachedEndorsement?.sentiment === "negative") {
    // Muted indicator on secondary; primary stays active for flipping
    notForMeBtn.textContent = "Not for me \u2713";
    notForMeBtn.classList.add("commit-endorse-indicator");
    notForMeBtn.disabled = true;
    endorseBtn.textContent = "Endorse";
    endorseBtn.addEventListener("click", () =>
      startEndorsement(subject.identifier, "positive", endorseBtn, notForMeBtn)
    );
  } else {
    // No cached state — default interactive buttons
    endorseBtn.textContent = "Endorse";
    endorseBtn.addEventListener("click", () =>
      startEndorsement(subject.identifier, "positive", endorseBtn, notForMeBtn)
    );
    notForMeBtn.textContent = "Not for me";
    notForMeBtn.addEventListener("click", () =>
      startEndorsement(subject.identifier, "negative", notForMeBtn, endorseBtn)
    );
  }

  actionRow.appendChild(endorseBtn);
  actionRow.appendChild(notForMeBtn);

  card.appendChild(circle);
  card.appendChild(details);
  card.appendChild(actionRow);
  return card;
}

async function startEndorsement(
  repoId: string,
  sentiment: "positive" | "negative",
  btn: HTMLButtonElement,
  otherBtn: HTMLButtonElement
): Promise<void> {
  const [owner, name] = repoId.split("/");
  btn.disabled = true;
  otherBtn.disabled = true;
  btn.textContent = "Proving...";
  btn.classList.add("commit-endorse-btn--active");

  try {
    const result = await chrome.runtime.sendMessage({
      type: "START_ENDORSEMENT",
      repoOwner: owner,
      repoName: name,
      sentiment,
    });

    if (result.success) {
      btn.classList.remove("commit-endorse-btn--active");

      if (sentiment === "positive") {
        btn.textContent = "Endorsed";
        btn.classList.add("commit-endorse-btn--done");
        otherBtn.textContent = "Not for me";
        otherBtn.classList.remove("commit-endorse-secondary--done");
      } else {
        btn.textContent = "Not for me \u2713";
        btn.classList.add("commit-endorse-secondary--done");
        otherBtn.textContent = "Endorse";
        otherBtn.classList.remove("commit-endorse-btn--done");
      }

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

      // Reset buttons after 3s to allow re-endorsement
      setTimeout(() => {
        if (sentiment === "positive") {
          btn.textContent = "Endorse";
          btn.classList.remove("commit-endorse-btn--done");
        } else {
          btn.textContent = "Not for me";
          btn.classList.remove("commit-endorse-secondary--done");
        }
        btn.disabled = false;
        otherBtn.disabled = false;
      }, 3000);
    } else {
      console.error("[commit] Endorsement failed:", result.error);
      const label = errorCodeToLabel(result.errorCode);
      resetEndorseButton(btn, label, sentiment);
      otherBtn.disabled = false;
    }
  } catch (err) {
    console.error("[commit] Endorsement error:", err);
    resetEndorseButton(btn, "Offline", sentiment);
    otherBtn.disabled = false;
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

function resetEndorseButton(
  btn: HTMLButtonElement,
  label: string,
  sentiment: "positive" | "negative" = "positive"
): void {
  btn.textContent = label;
  btn.disabled = false;
  const defaultLabel = sentiment === "positive" ? "Endorse" : "Not for me";
  setTimeout(() => {
    btn.textContent = defaultLabel;
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

  return data;
}

// Run on page load and GitHub SPA navigation
injectTrustCard();
new MutationObserver(() => injectTrustCard()).observe(document.body, {
  childList: true,
  subtree: true,
});
