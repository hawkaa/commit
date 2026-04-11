// Commit Offscreen Document — TLSNotary WASM proving
// Runs in the extension's offscreen document (not in content scripts or service worker)
// Communicates with background.js via chrome.runtime messages

const NOTARY_URL = "https://notary.pse.dev/v0.1.0-alpha.12";
const PROXY_BASE = "wss://notary.pse.dev/proxy";

let initialized = false;

async function ensureInit() {
  if (initialized) return;
  // tlsn.js (UMD) attaches exports to `this` (window)
  // init() sets up the WASM module
  await init({ loggingLevel: "Info" });
  initialized = true;
  console.log("[commit-offscreen] WASM initialized");
}

// Listen for messages from background.js
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "PROVE_ENDORSEMENT") {
    handleProveEndorsement(msg)
      .then((result) => sendResponse({ success: true, ...result }))
      .catch((err) => sendResponse({ success: false, error: err.message }));
    return true; // Keep channel open for async response
  }

});

async function handleProveEndorsement(msg) {
  await ensureInit();

  const { repoOwner, repoName } = msg;
  const serverDns = "api.github.com";
  const apiUrl = `https://api.github.com/repos/${repoOwner}/${repoName}`;
  const proxyUrl = `${PROXY_BASE}?token=${serverDns}`;

  console.log(`[commit-offscreen] Starting proof for ${repoOwner}/${repoName}`);
  const startTime = Date.now();

  // Use the simplified Prover.notarize() static method
  // This does: setup → send request → get transcript → notarize
  const presentationJSON = await Prover.notarize({
    url: apiUrl,
    notaryUrl: NOTARY_URL,
    websocketProxyUrl: proxyUrl,
    method: "GET",
    headers: {
      Accept: "application/vnd.github.v3+json",
      "User-Agent": "Commit-Extension/0.2.0",
    },
    maxRecvData: 4096,
    maxSentData: 1024,
    commit: {
      sent: [{ start: 0, end: 200 }], // Request line + headers
      recv: [{ start: 0, end: 500 }], // Response status + partial body
    },
  });

  const elapsed = Date.now() - startTime;
  console.log(`[commit-offscreen] Proof generated in ${elapsed}ms`);

  // Extract attestation hash from the presentation
  const attestationHex = presentationJSON?.data?.attestationHex || "";
  const proofHash = await hashString(attestationHex || JSON.stringify(presentationJSON));

  return {
    proofHash,
    elapsed,
    serverDns,
    presentationJSON,
  };
}

async function hashString(input) {
  const data = new TextEncoder().encode(input);
  const hash = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
