// Commit Offscreen Document — TLSNotary WASM proving
// Plain JS (not webpack-bundled) because it depends on UMD globals from tlsn-lib.js.
// The WASM worker chain has internal import paths that webpack cannot safely rewrite,
// so the tlsn-js UMD bundle is loaded via <script> tag in offscreen.html.
//
// UMD exports on window: "default" (init fn), "Prover", "Presentation", "NotaryServer"

/* global Prover, NotaryServer */

const NOTARY_URL = "https://notary.pse.dev/v0.1.0-alpha.12";
const PROXY_BASE = "wss://notary.pse.dev/proxy";
const INIT_TIMEOUT_MS = 30000;
const PROVE_TIMEOUT_MS = 60000;

const tlsnInit = self["default"];
let initialized = false;

function withTimeout(promise, ms, label) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms / 1000}s`)),
      ms
    );
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

async function ensureInit() {
  if (initialized) return;
  if (typeof tlsnInit !== "function") {
    throw new Error(
      "tlsn-lib.js not loaded. Expected self['default'] to be the init function."
    );
  }
  await tlsnInit({ loggingLevel: "Info" });
  initialized = true;
  console.log("[commit-offscreen] WASM initialized");
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "PROVE_ENDORSEMENT") {
    handleProveEndorsement(msg)
      .then((result) => sendResponse({ success: true, ...result }))
      .catch((err) => {
        console.error("[commit-offscreen] Prove failed:", err);
        sendResponse({ success: false, error: err.message || String(err) });
      });
    return true;
  }
});

async function handleProveEndorsement(msg) {
  await withTimeout(ensureInit(), INIT_TIMEOUT_MS, "WASM init");

  const { repoOwner, repoName } = msg;
  const serverDns = "api.github.com";
  const apiUrl = `https://api.github.com/repos/${repoOwner}/${repoName}`;
  const proxyUrl = `${PROXY_BASE}?token=${serverDns}`;

  console.log(
    `[commit-offscreen] Starting proof for ${repoOwner}/${repoName}`
  );
  const startTime = Date.now();

  // Prover.notarize() is the all-in-one static method from tlsn-js.
  // It handles: notary session → MPC-TLS → transcript → attestation.
  const presentationJSON = await withTimeout(
    Prover.notarize({
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
        sent: [{ start: 0, end: 200 }],
        recv: [{ start: 0, end: 500 }],
      },
    }),
    PROVE_TIMEOUT_MS,
    "MPC-TLS proving"
  );

  const elapsed = Date.now() - startTime;
  console.log(`[commit-offscreen] Proof generated in ${elapsed}ms`);

  const attestationHex = presentationJSON?.data?.attestationHex || "";
  const proofHash = await hashString(
    attestationHex || JSON.stringify(presentationJSON)
  );

  return { proofHash, elapsed, serverDns };
}

async function hashString(input) {
  const data = new TextEncoder().encode(input);
  const hash = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
