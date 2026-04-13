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

chrome.runtime.onInstalled.addListener(async (details) => {
  // Generate keypair if missing (runs on install and update)
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

  // Open onboarding tab only on fresh install (not on update or chrome_update)
  if (details.reason === "install") {
    try {
      await chrome.tabs.create({
        url: chrome.runtime.getURL("onboarding.html"),
      });
      console.log("[commit] Onboarding tab opened");
    } catch (err) {
      console.error("[commit] Failed to open onboarding tab:", err);
    }
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

interface KeyringEntry {
  publicKeyHex: string;
  label: string;
  addedAt: string;
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "START_ENDORSEMENT") {
    handleStartEndorsement(msg as EndorsementMessage)
      .then((result) => sendResponse(result))
      .catch((err: Error) =>
        sendResponse({ success: false, error: err.message })
      );
    return true;
  }

  if (msg.type === "KEYRING_ADD") {
    handleKeyringAdd(msg.publicKeyHex, msg.label)
      .then((result) => sendResponse(result))
      .catch(() => sendResponse({ success: false }));
    return true;
  }

  if (msg.type === "KEYRING_REMOVE") {
    handleKeyringRemove(msg.publicKeyHex)
      .then((result) => sendResponse(result))
      .catch(() => sendResponse({ success: false }));
    return true;
  }

  if (msg.type === "NETWORK_QUERY") {
    handleNetworkQuery(msg.subjectKind, msg.subjectId)
      .then((result) => sendResponse(result))
      .catch(() => sendResponse(null));
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

// === Keyring Management ===

// Serialize keyring mutations to prevent read-modify-write race conditions
let keyringMutex: Promise<void> = Promise.resolve();

async function handleKeyringAdd(
  publicKeyHex: string,
  label: string
): Promise<{ success: boolean }> {
  const hex = publicKeyHex.toLowerCase();
  if (hex.length !== 64 || !/^[0-9a-f]+$/.test(hex)) {
    return { success: false };
  }

  return new Promise((resolve) => {
    keyringMutex = keyringMutex.then(async () => {
      const stored = await chrome.storage.local.get("keyring");
      const keyring: KeyringEntry[] = stored.keyring || [];

      // Prevent duplicates
      if (keyring.some((e) => e.publicKeyHex === hex)) {
        resolve({ success: false });
        return;
      }

      keyring.push({
        publicKeyHex: hex,
        label: label || "Unknown",
        addedAt: new Date().toISOString(),
      });

      await chrome.storage.local.set({ keyring });
      console.log(`[commit] Added key to keyring: ${hex.slice(0, 8)}...`);
      resolve({ success: true });
    });
  });
}

async function handleKeyringRemove(
  publicKeyHex: string
): Promise<{ success: boolean }> {
  const hex = publicKeyHex.toLowerCase();

  return new Promise((resolve) => {
    keyringMutex = keyringMutex.then(async () => {
      const stored = await chrome.storage.local.get("keyring");
      const keyring: KeyringEntry[] = stored.keyring || [];

      const filtered = keyring.filter((e) => e.publicKeyHex !== hex);
      const removed = filtered.length < keyring.length;
      await chrome.storage.local.set({ keyring: filtered });
      console.log(`[commit] Removed key from keyring: ${hex.slice(0, 8)}...`);
      resolve({ success: removed });
    });
  });
}

// === Network Query ===

async function handleNetworkQuery(
  subjectKind: string,
  subjectId: string
): Promise<{ network: number; total: number } | null> {
  const stored = await chrome.storage.local.get("keyring");
  const keyring: KeyringEntry[] = stored.keyring || [];

  // Skip query if keyring is empty
  if (keyring.length === 0) return null;

  // Hash each contact's public key (the backend stores hashes, not raw keys)
  const keyHashes: string[] = [];
  for (const entry of keyring) {
    const chunks = entry.publicKeyHex.match(/.{2}/g);
    if (!chunks) continue;
    const bytes = new Uint8Array(
      chunks.map((b) => parseInt(b, 16))
    );
    const hashBuf = await crypto.subtle.digest("SHA-256", bytes);
    const hex = Array.from(new Uint8Array(hashBuf))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    keyHashes.push(hex);
  }

  try {
    const resp = await fetch(`${API_BASE}/network-query`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        kind: subjectKind,
        id: subjectId,
        key_hashes: keyHashes,
      }),
    });

    if (!resp.ok) return null;

    const data = await resp.json();
    return {
      network: data.network_endorsement_count,
      total: data.total_endorsement_count,
    };
  } catch {
    return null;
  }
}
