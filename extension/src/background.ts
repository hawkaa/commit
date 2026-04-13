// Commit — Background service worker
// Manages keypair, offscreen document for WASM proving, cache cleanup

import { API_BASE } from "./config";

interface EndorsementMessage {
  type: "START_ENDORSEMENT";
  repoOwner: string;
  repoName: string;
}

interface ProveResult {
  success: boolean;
  attestation?: string;
  transcriptSent?: string;
  elapsed?: number;
  error?: string;
  errorCode?: string;
}

chrome.runtime.onInstalled.addListener(async () => {
  const existing = await chrome.storage.local.get("keypair");
  if (!existing.keypair) {
    const keyPair = await crypto.subtle.generateKey(
      { name: "Ed25519" } as EcKeyGenParams,
      true,
      ["sign", "verify"]
    );
    const publicKey = await crypto.subtle.exportKey("raw", keyPair.publicKey);
    const privateKey = await crypto.subtle.exportKey("pkcs8", keyPair.privateKey);

    await chrome.storage.local.set({
      keypair: {
        publicKey: Array.from(new Uint8Array(publicKey)),
        privateKey: Array.from(new Uint8Array(privateKey)),
        createdAt: new Date().toISOString(),
      },
    });
    console.log("[commit] Keypair generated on install");
  }
});

// Periodic cache cleanup (every 6 hours)
chrome.alarms.create("cache-cleanup", { periodInMinutes: 360 });
chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name !== "cache-cleanup") return;

  const all = await chrome.storage.local.get(null);
  const now = Date.now();
  const ttl = 60 * 60 * 1000; // 1 hour

  for (const [key, value] of Object.entries(all)) {
    if (
      key.startsWith("trust-card:") &&
      (value as { timestamp?: number }).timestamp &&
      now - (value as { timestamp: number }).timestamp > ttl
    ) {
      await chrome.storage.local.remove(key);
    }
  }
});

// === Offscreen Document Management ===

let creatingOffscreen: Promise<void> | null = null;

async function ensureOffscreenDocument(): Promise<void> {
  const existingContexts = await chrome.runtime.getContexts({
    contextTypes: [chrome.runtime.ContextType.OFFSCREEN_DOCUMENT],
    documentUrls: [chrome.runtime.getURL("offscreen.html")],
  });

  if (existingContexts.length > 0) return;

  if (creatingOffscreen) {
    await creatingOffscreen;
    return;
  }

  creatingOffscreen = chrome.offscreen.createDocument({
    url: "offscreen.html",
    reasons: [chrome.offscreen.Reason.WORKERS],
    justification: "TLSNotary WASM proving for ZK endorsements",
  });

  await creatingOffscreen;
  creatingOffscreen = null;
  console.log("[commit] Offscreen document created");
}

// === Message Handling ===

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "START_ENDORSEMENT") {
    handleStartEndorsement(msg as EndorsementMessage)
      .then((result) => sendResponse(result))
      .catch((err: Error) =>
        sendResponse({ success: false, error: err.message })
      );
    return true;
  }
});

const ENDORSEMENT_TIMEOUT_MS = 60000;

async function handleStartEndorsement(
  msg: EndorsementMessage
): Promise<ProveResult> {
  const { repoOwner, repoName } = msg;
  console.log(`[commit] Starting endorsement for ${repoOwner}/${repoName}`);

  // Shared cancellation flag so the flow skips the API call after timeout
  const state = { cancelled: false };

  const timeoutPromise = new Promise<ProveResult>((resolve) =>
    setTimeout(() => {
      state.cancelled = true;
      resolve({ success: false, error: "Timeout", errorCode: "timeout" });
    }, ENDORSEMENT_TIMEOUT_MS)
  );

  const flowPromise = runEndorsementFlow(repoOwner, repoName, state);
  return Promise.race([flowPromise, timeoutPromise]);
}

/**
 * Compute the SHA-256 hash of the user's Ed25519 public key and return it as hex.
 * Returns null if the keypair is not available.
 */
async function getEndorserKeyHash(): Promise<string | null> {
  try {
    const stored = await chrome.storage.local.get("keypair");
    if (!stored.keypair?.publicKey) return null;
    const pubKeyBytes = new Uint8Array(stored.keypair.publicKey);
    const hashBuf = await crypto.subtle.digest("SHA-256", pubKeyBytes);
    return Array.from(new Uint8Array(hashBuf))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
  } catch {
    return null;
  }
}

async function runEndorsementFlow(
  repoOwner: string,
  repoName: string,
  state: { cancelled: boolean }
): Promise<ProveResult> {
  try {
    await ensureOffscreenDocument();
  } catch (err) {
    console.error("[commit] Failed to create offscreen document:", err);
    return {
      success: false,
      error: "Notary offline",
      errorCode: "notary_offline",
    };
  }

  let result: ProveResult;
  try {
    result = await chrome.runtime.sendMessage({
      type: "PROVE_ENDORSEMENT",
      repoOwner,
      repoName,
    });
  } catch (err) {
    console.error("[commit] Proof generation error:", err);
    return {
      success: false,
      error: "Notary offline",
      errorCode: "notary_offline",
    };
  }

  if (!result.success) {
    console.error("[commit] Proof generation failed:", result.error);
    // Check if the error indicates notary connectivity issues
    const errMsg = (result.error ?? "").toLowerCase();
    if (
      errMsg.includes("notary") ||
      errMsg.includes("connect") ||
      errMsg.includes("websocket")
    ) {
      return { ...result, errorCode: "notary_offline" };
    }
    return { ...result, errorCode: "prove_failed" };
  }

  console.log(`[commit] Proof generated in ${result.elapsed}ms`);

  // Skip the API call if the timeout already fired
  if (state.cancelled) {
    console.warn("[commit] Endorsement flow completed after timeout — skipping submission");
    return { success: false, error: "Timeout", errorCode: "timeout" };
  }

  try {
    const endorserKeyHash = await getEndorserKeyHash();
    const body: Record<string, string | undefined> = {
      subject_kind: "github",
      subject_id: `${repoOwner}/${repoName}`,
      category: "usage",
      attestation: result.attestation,
      proof_type: "git_history",
      transcript_sent: result.transcriptSent,
    };
    if (endorserKeyHash) {
      body.endorser_key_hash = endorserKeyHash;
    }

    const resp = await fetch(`${API_BASE}/endorsements`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!resp.ok) {
      const text = await resp.text();
      const errorCode = resp.status === 409 ? "duplicate" : "backend_error";
      return {
        success: false,
        error: `Backend error: ${resp.status} ${text}`,
        errorCode,
      };
    }

    const endorsement = await resp.json();
    console.log(`[commit] Endorsement created: ${endorsement.id}`);

    // Increment local endorsement counter for popup summary.
    // Interim source until the "you endorsed this" revisit indicator lands.
    const { endorsement_count = 0 } = await chrome.storage.local.get("endorsement_count");
    await chrome.storage.local.set({ endorsement_count: (endorsement_count as number) + 1 });

    return {
      success: true,
      attestation: result.attestation,
      elapsed: result.elapsed,
    };
  } catch (err) {
    return {
      success: false,
      error: `Network error: ${(err as Error).message}`,
      errorCode: "network",
    };
  }
}
