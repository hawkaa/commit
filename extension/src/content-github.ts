// Commit Trust Card — GitHub content script
// Injects trust cards on github.com repo pages
export {}; // Make this a module to avoid global scope conflicts

const API_BASE = "https://commit-backend.fly.dev";
const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour

interface TrustCardData {
  subject: { identifier: string; display_name: string };
  score: {
    score: number | null;
    breakdown: { longevity: number; community: number; maintenance: number };
  };
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
    } else {
      console.error("[commit] Endorsement failed:", result.error);
      resetEndorseButton(btn, "Failed");
    }
  } catch (err) {
    console.error("[commit] Endorsement error:", err);
    resetEndorseButton(btn, "Error");
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
  if (cached[cacheKey]) {
    const { data, timestamp } = cached[cacheKey] as {
      data: TrustCardData;
      timestamp: number;
    };
    if (Date.now() - timestamp < CACHE_TTL_MS) return data;
  }

  const resp = await fetch(
    `${API_BASE}/trust-card?kind=${kind}&id=${encodeURIComponent(id)}`
  );
  if (!resp.ok) return null;

  const data: TrustCardData = await resp.json();
  await chrome.storage.local.set({
    [cacheKey]: { data, timestamp: Date.now() },
  });
  return data;
}

// Run on page load and GitHub SPA navigation
injectTrustCard();
new MutationObserver(() => injectTrustCard()).observe(document.body, {
  childList: true,
  subtree: true,
});
