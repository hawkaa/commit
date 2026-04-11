---
title: Integrating TLSNotary WASM into Chrome Manifest V3 Extensions
date: 2026-04-11
category: best-practices
module: extension
problem_type: best_practice
component: tooling
severity: high
applies_when:
  - Adding TLSNotary MPC-TLS proving to a Chrome extension
  - Loading any WASM that uses Atomics.wait and spawns sub-workers
  - Integrating wasm-bindgen output with a non-trivial worker chain into MV3
tags:
  - tlsnotary
  - wasm
  - chrome-extension
  - manifest-v3
  - offscreen-document
  - web-worker
  - webpack
  - atomics-wait
---

# Integrating TLSNotary WASM into Chrome Manifest V3 Extensions

## Context

TLSNotary's MPC-TLS proving runs in browser WASM (~5s for a 1KB request). The WASM binary uses `Atomics.wait` for thread synchronization and spawns sub-workers for its thread pool. Chrome Manifest V3 extensions have strict execution contexts: service workers can't run WASM, content scripts use the page's CSP, and offscreen documents block `Atomics.wait` on their main thread.

Getting TLSNotary WASM to actually work in this environment required solving five interdependent problems that are not documented anywhere in the TLSNotary or Chrome extension docs. Each bug was silent or misleading (hangs, empty errors, wrong function names) and only surfaced at runtime in the extension context.

This was discovered while building Commit, a behavioral trust layer that uses ZK-verified endorsement signals. (auto memory [claude])

## Guidance

### Architecture: Three-layer message relay

The working architecture has three layers:

```
Content Script (github.com)
  → chrome.runtime.sendMessage("START_ENDORSEMENT")
    → Background Service Worker
      → chrome.offscreen.createDocument("offscreen.html")
        → Offscreen Document (offscreen-bundle.js)
          → new Worker("prove-worker.js")
            → WASM init + MPC-TLS proving
              → postMessage(result) back up the chain
```

The offscreen document's main thread **cannot** run the WASM directly. It must create a Web Worker. The worker is where `Atomics.wait` is allowed.

### Build system: webpack with CopyWebpackPlugin

The TLSNotary WASM package has internal import paths that webpack cannot safely rewrite. The solution is a hybrid approach:

1. **webpack bundles** `prove-worker.ts` as a separate entry point (imports `tlsn-js` which webpack resolves)
2. **CopyWebpackPlugin** copies the WASM binary and worker files verbatim at the exact paths the runtime expects
3. **offscreen-bundle.js** is plain JS (not webpack-bundled) that creates the worker and relays Chrome messages

```js
// webpack.config.js — key patterns
module.exports = {
  entry: {
    background: "src/background.ts",
    "content-github": "src/content-github.ts",
    "prove-worker": "src/prove-worker.ts", // WASM worker entry
  },
  plugins: [
    new CopyWebpackPlugin({
      patterns: [
        // offscreen relay (plain JS, not bundled)
        { from: "src/offscreen-bundle.js", to: "offscreen.js" },
        // Hashed WASM binary (referenced by lib.js as n.p+"96d03...wasm")
        { from: "node_modules/tlsn-js/build/96d038089797746d7695.wasm", to: "96d038089797746d7695.wasm" },
        // Hashed spawn worker (referenced by lib.js as n.p+"a6de6...js")
        { from: "node_modules/tlsn-js/build/a6de6b189c13ad309102.js", to: "a6de6b189c13ad309102.js" },
        // Raw wasm-bindgen files (referenced by spawn worker via import("../../../tlsn_wasm.js"))
        { from: "node_modules/tlsn-wasm/tlsn_wasm_bg.wasm", to: "tlsn_wasm_bg.wasm" },
        { from: "node_modules/tlsn-wasm/tlsn_wasm.js", to: "tlsn_wasm.js" },
        { from: "node_modules/tlsn-wasm/snippets", to: "snippets" },
        // spawn.js also needed at root (sub-workers request /spawn.js)
        { from: "node_modules/tlsn-wasm/snippets/web-spawn-0303048270a97ee1/js/spawn.js", to: "spawn.js" },
      ],
    }),
  ],
};
```

### Manifest requirements

```json
{
  "permissions": ["offscreen"],
  "content_security_policy": {
    "extension_pages": "script-src 'self' 'wasm-unsafe-eval'; object-src 'self';"
  }
}
```

### Error handling: timeouts are mandatory

The WASM worker chain has multiple failure points that hang silently. Every async boundary needs a timeout:

```js
// offscreen-bundle.js pattern
function sendToWorker(msg) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() =>
      reject(new Error("Proving timed out after 60s")), 60000);
    pendingRequests.set(requestId, {
      resolve: (r) => { clearTimeout(timer); resolve(r); },
      reject: (e) => { clearTimeout(timer); reject(e); },
    });
    getWorker().postMessage({ ...msg, requestId });
  });
}
```

And reset the worker reference on crash:

```js
worker.onerror = (e) => {
  for (const [id, { reject }] of pendingRequests) {
    reject(new Error("Worker crashed: " + e.message));
    pendingRequests.delete(id);
  }
  worker = null; // Critical: prevents dead worker from hanging future requests
};
```

## Why This Matters

Without this knowledge, every attempt to integrate TLSNotary WASM into a Chrome extension will hit the same sequence of 5 silent failures:

1. **`init is not defined`** — ES module `export default` becomes `"default"` in UMD. No error at load time, fails at call time.
2. **`Failed to fetch dynamically imported module`** — spawn worker can't find `tlsn_wasm.js` because the path is hardcoded relative.
3. **`Failed to fetch` for WASM binary** — webpack hashed name vs wasm-bindgen original name mismatch.
4. **`Atomics.wait cannot be called in this context`** — silent hang on main thread with no useful error until you add your own timeout.
5. **Thread pool init hangs** — spawn.js needed at root level in addition to the snippets path. Only visible in server request logs.

Each bug takes 30-60 minutes to diagnose because the errors are either silent (hang) or misleading (generic fetch failure). This document saves ~4 hours of debugging.

## When to Apply

- Integrating TLSNotary (tlsn-js or tlsn-wasm) into any Chrome MV3 extension
- Loading any WASM module that uses `Atomics.wait` and `SharedArrayBuffer` in an extension offscreen document
- Using wasm-bindgen output with spawn workers in a non-webpack-dev-server context
- Any Chrome extension that needs multi-threaded WASM execution

## Examples

### Before: raw JS approach (broken)

```html
<!-- offscreen.html — this does NOT work -->
<script src="tlsn/tlsn.js"></script>
<script>
  // Bug 1: init doesn't exist, it's self["default"]
  await init({ loggingLevel: "Info" });
  // Bug 4: Atomics.wait blocked on main thread — hangs forever
  await Prover.notarize({ ... });
</script>
```

### After: webpack + Web Worker approach (working)

```html
<!-- offscreen.html -->
<script src="offscreen.js"></script>
```

```js
// offscreen.js (plain JS relay, not webpack-bundled)
const worker = new Worker("prove-worker.js");
worker.onmessage = (e) => { /* resolve pending request */ };
worker.onerror = (e) => { worker = null; /* reject all pending */ };

chrome.runtime.onMessage.addListener((msg, _, sendResponse) => {
  if (msg.type === "PROVE_ENDORSEMENT") {
    sendToWorker(msg).then(r => sendResponse(r)).catch(e => sendResponse({ error: e.message }));
    return true;
  }
});
```

```typescript
// prove-worker.ts (webpack-bundled entry point)
import init, { Prover, NotaryServer } from "tlsn-js";

let initialized = false;
async function ensureInit() {
  if (initialized) return;
  await init({ loggingLevel: "Info" }); // Atomics.wait works in a Worker
  initialized = true;
}

self.onmessage = async (e) => {
  await ensureInit();
  const notary = NotaryServer.from(NOTARY_URL);
  const prover = new Prover({ serverDns, maxRecvData: 4096 });
  await prover.setup(await notary.sessionUrl());
  // ... proving logic
};
```

### Key technical detail: why webpack over Vite

Webpack is required because:
- `experiments: { asyncWebAssembly: true }` for native WASM module support
- Native worker bundling via `new Worker(new URL('./worker', import.meta.url))`
- Vite's WASM support uses `?init` suffix, incompatible with tlsn-wasm
- esbuild doesn't support WASM modules at all
- The upstream `tlsn-extension` repo uses webpack (the only proven config)

### Key technical detail: TLSNotary does NOT use Halo2

TLSNotary uses **MPC-TLS + QuickSilver** (VOLE-based interactive ZK), not Halo2. Several design documents and blog posts incorrectly reference Halo2. The research spike confirmed this. Benchmark: ~5s proving time for a 1KB request/response in headless Chromium. (session history)

## Related

- [TLSNotary Extension Monorepo](https://github.com/tlsnotary/tlsn-extension) — reference webpack config for MV3 WASM integration
- [tlsn-js (deprecated)](https://github.com/tlsnotary/tlsn-js) — still usable for alpha.12 UMD bundle
- `extension/webpack.config.js` — this project's implementation of the patterns above
- `extension/src/prove-worker.ts` — the Web Worker that runs WASM
- `extension/src/offscreen-bundle.js` — the message relay pattern
