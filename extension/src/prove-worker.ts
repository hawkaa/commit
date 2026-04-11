// Web Worker that runs TLSNotary WASM proving.
// Must run in a Worker (not main thread) because WASM uses Atomics.wait.
// Webpack bundles this as a separate entry point, resolving all spawn worker paths.

import init, { Prover, NotaryServer } from "tlsn-js";
import { NOTARY_URL, PROXY_BASE } from "./config";

let initialized = false;

async function ensureInit(): Promise<void> {
  if (initialized) return;
  await init({ loggingLevel: "Info" });
  initialized = true;
  console.log("[prove-worker] WASM initialized");
}

interface ProveRequest {
  type: "prove";
  repoOwner: string;
  repoName: string;
  requestId: string;
}

self.onmessage = async (e: MessageEvent<ProveRequest>) => {
  const { type, repoOwner, repoName, requestId } = e.data;
  if (type !== "prove") return;

  try {
    await ensureInit();

    const serverDns = "api.github.com";
    const apiUrl = `https://api.github.com/repos/${repoOwner}/${repoName}`;
    const proxyUrl = `${PROXY_BASE}?token=${serverDns}`;

    console.log(`[prove-worker] Starting proof for ${repoOwner}/${repoName}`);
    const startTime = Date.now();

    const notary = NotaryServer.from(NOTARY_URL);
    const prover = new Prover({
      serverDns: serverDns,
      maxRecvData: 4096,
    });

    await prover.setup(await notary.sessionUrl());

    await prover.sendRequest(proxyUrl, {
      url: apiUrl,
      method: "GET",
      headers: {
        Accept: "application/vnd.github.v3+json",
        "User-Agent": "Commit-Extension/0.2.0",
      },
    });

    const transcript = await prover.transcript();
    const commit = {
      sent: [{ start: 0, end: Math.min(200, transcript.sent.length) }],
      recv: [{ start: 0, end: Math.min(500, transcript.recv.length) }],
    };

    const notarization = await prover.notarize(commit);

    const elapsed = Date.now() - startTime;
    console.log(`[prove-worker] Proof generated in ${elapsed}ms`);

    // Send the full attestation for server-side proof_hash derivation.
    // The backend expects transcript_sent as the raw HTTP request text
    // (the revealed portion). Convert from the transcript's native format.
    const raw = transcript.sent;
    const sentBytes = new Uint8Array(
      raw instanceof Uint8Array ? raw : Array.isArray(raw) ? raw : []
    );
    const transcriptSent = new TextDecoder().decode(sentBytes.slice(0, 200));

    self.postMessage({
      type: "result",
      requestId,
      success: true,
      attestation: notarization.attestation,
      transcriptSent: transcriptSent,
      elapsed,
    });
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    console.error("[prove-worker] Error:", message);
    self.postMessage({
      type: "result",
      requestId,
      success: false,
      error: message,
    });
  }
};
