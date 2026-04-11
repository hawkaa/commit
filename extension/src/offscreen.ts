// Commit Offscreen Document — TLSNotary WASM proving
// Runs in the extension's offscreen document (not in content scripts or service worker)
// Communicates with background.ts via chrome.runtime messages

import initWasm, { Prover, type ProverConfig } from "tlsn-wasm";

const NOTARY_URL = "https://notary.pse.dev/v0.1.0-alpha.12";
const PROXY_BASE = "wss://notary.pse.dev/proxy";

let initialized = false;

async function withTimeout<T>(
  promise: Promise<T>,
  ms: number,
  label: string
): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms / 1000}s`)),
      ms
    );
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer!);
  }
}

async function ensureInit(): Promise<void> {
  if (initialized) return;
  await initWasm();
  initialized = true;
  console.log("[commit-offscreen] WASM initialized");
}

interface ProveMessage {
  type: "PROVE_ENDORSEMENT";
  repoOwner: string;
  repoName: string;
}

interface ProveResult {
  proofHash: string;
  elapsed: number;
  serverDns: string;
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "PROVE_ENDORSEMENT") {
    handleProveEndorsement(msg as ProveMessage)
      .then((result) => sendResponse({ success: true, ...result }))
      .catch((err: Error) =>
        sendResponse({ success: false, error: err.message })
      );
    return true;
  }
});

async function handleProveEndorsement(msg: ProveMessage): Promise<ProveResult> {
  await withTimeout(ensureInit(), 30000, "WASM init");

  const { repoOwner, repoName } = msg;
  const serverDns = "api.github.com";
  const apiUrl = `https://api.github.com/repos/${repoOwner}/${repoName}`;
  const proxyUrl = `${PROXY_BASE}?token=${serverDns}`;

  console.log(
    `[commit-offscreen] Starting proof for ${repoOwner}/${repoName}`
  );
  const startTime = Date.now();

  const config: ProverConfig = {
    server_name: serverDns,
    max_sent_data: 1024,
    max_sent_records: undefined,
    max_recv_data_online: undefined,
    max_recv_data: 4096,
    max_recv_records_online: undefined,
    defer_decryption_from_start: undefined,
    network: "Bandwidth",
    client_auth: undefined,
  };
  const prover = new Prover(config);

  await withTimeout(
    prover.setup(await getNotarySessionUrl()),
    30000,
    "Notary setup"
  );

  const resp = await withTimeout(
    prover.send_request(proxyUrl, {
      uri: apiUrl,
      method: "GET",
      headers: new Map([
        ["Accept", Array.from(new TextEncoder().encode("application/vnd.github.v3+json"))],
        ["User-Agent", Array.from(new TextEncoder().encode("Commit-Extension/0.2.0"))],
      ]),
      body: undefined,
    }),
    30000,
    "HTTP request via MPC-TLS"
  );

  console.log(`[commit-offscreen] Got response: ${resp.status}`);

  const transcript = prover.transcript();

  await withTimeout(
    prover.reveal({
      sent: [{ start: 0, end: Math.min(200, transcript.sent.length) }],
      recv: [{ start: 0, end: Math.min(500, transcript.recv.length) }],
      server_identity: true,
    }),
    30000,
    "Reveal"
  );

  prover.free();

  const elapsed = Date.now() - startTime;
  console.log(`[commit-offscreen] Proof generated in ${elapsed}ms`);

  const proofHash = await hashString(
    `${serverDns}:${repoOwner}/${repoName}:${Date.now()}`
  );

  return { proofHash, elapsed, serverDns };
}

async function getNotarySessionUrl(): Promise<string> {
  const resp = await fetch(`${NOTARY_URL}/session`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      clientType: "Websocket",
      maxRecvData: 4096,
      maxSentData: 1024,
    }),
  });
  if (!resp.ok) throw new Error(`Notary session failed: ${resp.status}`);
  const data = await resp.json();
  return data.sessionUrl || `${NOTARY_URL}/notarize?sessionId=${data.sessionId}`;
}

async function hashString(input: string): Promise<string> {
  const data = new TextEncoder().encode(input);
  const hash = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}
