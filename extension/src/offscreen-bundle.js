// Commit Offscreen Document — message relay to prove-worker
// The offscreen document's main thread handles Chrome message passing.
// WASM proving runs in a Web Worker (required: Atomics.wait is blocked on main thread).
// The worker is a webpack-bundled entry point with all WASM paths resolved.

const PROVE_TIMEOUT_MS = 60000;
let worker = null;
let pendingRequests = new Map();

function getWorker() {
  if (worker) return worker;
  worker = new Worker("prove-worker.js");
  worker.onmessage = (e) => {
    const { type, requestId, ...rest } = e.data;
    if (type === "result" && pendingRequests.has(requestId)) {
      const { resolve } = pendingRequests.get(requestId);
      pendingRequests.delete(requestId);
      resolve(rest);
    }
  };
  worker.onerror = (e) => {
    console.error("[offscreen] Worker error:", e.message);
    for (const [id, { reject }] of pendingRequests) {
      reject(new Error("Worker crashed: " + e.message));
      pendingRequests.delete(id);
    }
  };
  return worker;
}

function sendToWorker(msg) {
  return new Promise((resolve, reject) => {
    const requestId = crypto.randomUUID();
    const timer = setTimeout(() => {
      pendingRequests.delete(requestId);
      reject(new Error(`Proving timed out after ${PROVE_TIMEOUT_MS / 1000}s`));
    }, PROVE_TIMEOUT_MS);

    pendingRequests.set(requestId, {
      resolve: (result) => {
        clearTimeout(timer);
        resolve(result);
      },
      reject: (err) => {
        clearTimeout(timer);
        reject(err);
      },
    });

    getWorker().postMessage({ ...msg, requestId });
  });
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg.type === "PROVE_ENDORSEMENT") {
    sendToWorker({
      type: "prove",
      repoOwner: msg.repoOwner,
      repoName: msg.repoName,
    })
      .then((result) => sendResponse({ success: true, ...result }))
      .catch((err) => {
        console.error("[offscreen] Prove failed:", err);
        sendResponse({ success: false, error: err.message || String(err) });
      });
    return true;
  }
});
