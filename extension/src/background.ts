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
  proofHash?: string;
  elapsed?: number;
  error?: string;
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

async function handleStartEndorsement(
  msg: EndorsementMessage
): Promise<ProveResult> {
  const { repoOwner, repoName } = msg;
  console.log(`[commit] Starting endorsement for ${repoOwner}/${repoName}`);

  await ensureOffscreenDocument();

  const result: ProveResult = await chrome.runtime.sendMessage({
    type: "PROVE_ENDORSEMENT",
    repoOwner,
    repoName,
  });

  if (!result.success) {
    console.error("[commit] Proof generation failed:", result.error);
    return result;
  }

  console.log(`[commit] Proof generated in ${result.elapsed}ms`);

  try {
    const resp = await fetch(`${API_BASE}/endorsements`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        subject_kind: "github",
        subject_id: `${repoOwner}/${repoName}`,
        category: "usage",
        proof_hash: result.proofHash,
        proof_type: "git_history",
      }),
    });

    if (!resp.ok) {
      const text = await resp.text();
      return { success: false, error: `Backend error: ${resp.status} ${text}` };
    }

    const endorsement = await resp.json();
    console.log(`[commit] Endorsement created: ${endorsement.id}`);
    return {
      success: true,
      proofHash: result.proofHash,
      elapsed: result.elapsed,
    };
  } catch (err) {
    return {
      success: false,
      error: `Network error: ${(err as Error).message}`,
    };
  }
}
