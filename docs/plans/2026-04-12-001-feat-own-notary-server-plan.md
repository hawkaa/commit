---
title: "feat: Deploy own TLSNotary notary server to Fly.io"
type: feat
status: active
date: 2026-04-12
---

# feat: Deploy own TLSNotary notary server to Fly.io

## Overview

Deploy the TLSNotary notary server as `commit-verifier` on Fly.io with a persistent signing key, update the extension to use it for notarization, and store the notary's public key in the backend config. This eliminates dependence on the public `notary.pse.dev` for the security-critical signing component and unblocks server-side attestation signature verification (tracked as a separate follow-up).

## Problem Frame

The extension currently uses the public `notary.pse.dev` server for MPC-TLS notarization. This has three problems:

1. **No control over signing key.** The public server's signing key is unknown and could change. Attestation signature verification is impossible without a known, stable public key.
2. **Dependency on external infrastructure.** The public server could go down, rate-limit, or change behavior without notice.
3. **Security follow-ups blocked.** Three loose threads from the P0 proof-binding fix (full attestation verification, webhook fallback deprecation, score integrity) all require controlling the notary server.

## Requirements Trace

- R1. The own notary server must run on Fly.io with a persistent secp256k1 signing key that survives container restarts
- R2. The extension must connect to the own notary for MPC-TLS notarization
- R3. The notary's public key must be available to the backend (via env var) for future attestation verification
- R4. The WebSocket proxy (`PROXY_BASE`) may remain on `notary.pse.dev` for now — the proxy is a stateless byte bridge with no security sensitivity
- R5. Version pinned to `v0.1.0-alpha.12` across all components (extension, server, tlsn-js)
- R6. The extension must continue to work for users who already have it installed (Chrome Web Store update)

## Scope Boundaries

- Attestation signature verification in the backend (separate follow-up, now unblocked)
- Webhook `hash_verification_results` fallback deprecation (separate follow-up)
- Own WebSocket-to-TCP proxy deployment (deferred — `notary.pse.dev/proxy` is adequate)
- Notary server auth/whitelist configuration (not needed yet with <50 users)
- Email/ci_logs proof type transcript binding (separate follow-ups)

### Deferred to Separate Tasks

- Full attestation signature verification: next follow-up item, uses the public key stored in this plan's Unit 3
- Own WebSocket proxy: deploy when `notary.pse.dev` becomes unreliable or before launch

## Context & Research

### Relevant Code and Patterns

- `verifier/fly.toml` — pre-existing Fly.io config (app name, region, image reference, port 7047)
- `fly.toml` — backend Fly.io config (pattern for secrets, env, auto-stop)
- `Dockerfile` — backend multi-stage build pattern (though notary uses a pre-built image)
- `extension/src/config.ts` — `NOTARY_URL` and `PROXY_BASE` constants (single source of truth)
- `extension/src/prove-worker.ts` — `NotaryServer.from(NOTARY_URL)` → `notary.sessionUrl()` → `prover.setup(sessionUrl)` flow
- `extension/src/manifest.json` — `host_permissions` includes `notary.pse.dev`
- `src/lib.rs` — `AppState` struct (db, github)
- `src/main.rs` — env var loading pattern, state construction

### Institutional Learnings

- TLSNotary WASM integration doc (`docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md`): confirms the extension uses two separate WebSocket connections — one to the notary (for MPC-TLS), one to the proxy (for TCP bridging). The proxy connection is independent of the notary server.
- Proof binding security doc (`docs/solutions/security-issues/...`): lists three loose threads directly unblocked by deploying own notary (attestation verification, webhook fallback deprecation, score integrity).
- WASM worker silently hangs on connection failures with a 60-second timeout. Need explicit error handling when testing the new URL.

### External References

- TLSNotary notary server config: YAML format, supports `notarization.private_key_path` for persistent keys. PKCS#8 PEM format, secp256k1 or secp256r1 curves. Env vars use `NS_` prefix with double-underscore nesting.
- `GET /info` endpoint returns `{ version, publicKey (PEM), gitCommitHash }`. The `publicKey` is SPKI PEM format.
- The notary server Docker image has NO `/proxy` endpoint — proxy is only in the separate verifier server from `tlsn-extension`. The public `notary.pse.dev` runs a combined deployment that serves both roles.
- Docker image working directory is `/root/.notary`. Default port 7047.

## Key Technical Decisions

- **Proxy stays on `notary.pse.dev`**: The WebSocket-to-TCP proxy is a stateless byte bridge — no secrets, no signing, no identity. The notary server (which signs attestations with its private key) is the security-critical component. Deploying the notary first and proxy later is the correct risk-reduction ordering. The two connections are independent, so `NOTARY_URL` and `PROXY_BASE` can point to different hosts.

- **Custom Dockerfile wrapping the official image**: The notary server expects its signing key as a file at `notarization.private_key_path`. Fly.io secrets are env vars, not files. A thin wrapper Dockerfile with an entrypoint script writes the env var to a file, then starts the notary server. This is the standard pattern for Docker deployments requiring secrets as files.

- **secp256k1 signing algorithm**: Matches the notary server default and the TLSNotary attestation format used by `tlsn-js` v0.1.0-alpha.12. No reason to deviate.

- **Notary public key stored as env var on backend**: `NOTARY_PUBLIC_KEY` env var (PEM format) loaded at startup and added to `AppState`. The key is fetched once from `GET /info` after deployment and set via `fly secrets set`. This avoids runtime dependency on the notary's `/info` endpoint and gives the backend a pinned key for future attestation verification.

- **No notary auth for now**: The notary server supports API key whitelist and JWT auth, but with <50 users and the server being publicly accessible (the extension needs to reach it), auth adds complexity without security benefit at this scale. Revisit before launch.

- **Extension host_permissions update triggers CWS review**: Adding `commit-verifier.fly.dev` to `host_permissions` requires a Chrome Web Store review. Since the extension is already published, this is a permission change review (typically 1-3 business days). The `notary.pse.dev` permissions should remain since we still use the proxy.

## Open Questions

### Resolved During Planning

- **Should the notary server have a persistent volume?** No. The only state is the signing key, which is injected from a Fly secret at container startup. The notary is stateless otherwise.
- **Which port?** 7047 (notary server default). Fly.io HTTP service terminates TLS, so `tls.enabled: false` is correct.
- **Should we deploy the verifier server instead?** No. The verifier server has a different protocol (`WS /session` registration, not `POST /session`). The `tlsn-js` library's `NotaryServer` class expects the notary server protocol. Deploying the verifier would require changes to the proving flow.
- **Auto-stop machines?** Yes, consistent with the backend pattern. The notary is stateless and starts fast.

### Deferred to Implementation

- Exact `NOTARY_PUBLIC_KEY` PEM string — fetched from `GET /info` after first deployment
- Whether `fly apps create commit-verifier` needs to be run or if `fly launch` handles it
- Memory pressure under MPC-TLS load — monitor and bump from 512MB if needed

## Output Structure

Note: The `verifier/` directory contains the TLSNotary *notary server* (which signs attestations), not the verifier server (which handles transcript verification and webhooks — not deployed in this plan). The directory name predates this distinction.

```
verifier/
  fly.toml          (modify: add cmd override)
  Dockerfile         (new: wraps official image with entrypoint)
  entrypoint.sh      (new: writes signing key from env, starts server)
  config.yaml        (new: notary server config with key path)
```

## Implementation Units

- [ ] **Unit 1: Notary server infra — Dockerfile, config, and entrypoint**

**Goal:** Create the deployment artifacts for running the TLSNotary notary server on Fly.io with a persistent signing key.

**Requirements:** R1, R5

**Dependencies:** None

**Files:**
- Create: `verifier/Dockerfile`
- Create: `verifier/entrypoint.sh`
- Create: `verifier/config.yaml`
- Modify: `verifier/fly.toml`

**Approach:**
- `Dockerfile`: FROM the official notary server image (`ghcr.io/tlsnotary/tlsn/notary-server:v0.1.0-alpha.12`). Copy `config.yaml` and `entrypoint.sh` into the image. Set `entrypoint.sh` as the entrypoint.
- `entrypoint.sh`: Read `NOTARY_SIGNING_KEY` env var (PEM content), write to `/root/.notary/notary.key` with `printf '%s\n'` (preserves newlines from the env var), then exec the notary server binary with `--config /root/.notary/config.yaml`. Fail fast with `echo "FATAL: NOTARY_SIGNING_KEY not set" >&2; exit 1` if the env var is missing — this surfaces clearly in `fly logs`.
- `config.yaml`: Set `notarization.private_key_path: "/root/.notary/notary.key"`, `notarization.signature_algorithm: secp256k1`, `host: "0.0.0.0"`, `port: 7047`, `tls.enabled: false` (Fly terminates TLS), `log.level: "INFO"`. Keep defaults for `max_sent_data` (4096), `max_recv_data` (16384), `timeout` (1800s), `concurrency` (32).
- `fly.toml`: Update `[build]` section from `image = '...'` to local Dockerfile build. Keep everything else (app name, region, port, VM spec).

**Patterns to follow:**
- Backend `Dockerfile` multi-stage pattern (though this is simpler — just wrapping an existing image)
- Backend `fly.toml` `[build]` section (for local Dockerfile builds)

**Test scenarios:**
Test expectation: none — infrastructure-as-code with no behavioral code. Correctness verified by deployment.

**Verification:**
- `docker build -t commit-verifier verifier/` succeeds locally
- The Dockerfile, config, and entrypoint are consistent (paths match, key format is PKCS#8 PEM)
- `fly deploy --app commit-verifier` succeeds
- `curl https://commit-verifier.fly.dev/info` returns JSON with a stable `publicKey` field
- Restarting the machine (`fly machine restart`) does not change the public key

---

- [ ] **Unit 2: Extension — Point to own notary server**

**Goal:** Update the extension to use the own notary server for MPC-TLS notarization while keeping the public proxy.

**Requirements:** R2, R4, R5, R6

**Dependencies:** Unit 1

**Files:**
- Modify: `extension/src/config.ts`
- Modify: `extension/src/manifest.json`

**Approach:**
- In `config.ts`: Change `NOTARY_URL` from `"https://notary.pse.dev/v0.1.0-alpha.12"` to `"https://commit-verifier.fly.dev"` (no version path — the own server serves at root). Keep `PROXY_BASE` unchanged at `"wss://notary.pse.dev/proxy"`.
- In `manifest.json`: Add `"https://commit-verifier.fly.dev/*"` and `"wss://commit-verifier.fly.dev/*"` to `host_permissions`. Keep the existing `notary.pse.dev` entries (still used for proxy).
- Bump extension version if needed for CWS submission.

**Patterns to follow:**
- Existing `config.ts` constant pattern
- Existing `host_permissions` URL patterns in manifest.json

**Test scenarios:**
Test expectation: none — config constants with no logic. Correctness verified by end-to-end sideload test.

**Verification:**
- `npm run build` in `extension/` succeeds
- **Sideload test (before CWS submission):** Load the built extension locally via `chrome://extensions` developer mode. Click "Endorse" on a GitHub repo page → extension connects to `commit-verifier.fly.dev` for notarization and `notary.pse.dev` for proxy → attestation generated → endorsement submitted to backend. This verifies Fly.io WebSocket support for MPC-TLS sessions end-to-end.
- Check browser DevTools network tab: notarization requests go to `commit-verifier.fly.dev`, proxy traffic goes to `notary.pse.dev`
- **Only submit to CWS after sideload test passes.** A broken notary URL in a CWS release takes 1-3 days to fix via another review.

---

- [ ] **Unit 3: Backend — Add notary public key to config and AppState**

**Goal:** Store the notary server's public key in the backend so it's available for future attestation signature verification.

**Requirements:** R3

**Dependencies:** Unit 1 (need the deployed notary to fetch the public key)

**Files:**
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/api.rs`

**Approach:**
- In `lib.rs`: Add `notary_public_key: Option<String>` to `AppState`. Optional because it's only needed once attestation verification is implemented (next follow-up), and tests shouldn't require it.
- In `main.rs`: Load `NOTARY_PUBLIC_KEY` from env var at startup (optional — `std::env::var(...).ok()`). Log the key fingerprint (first 16 chars of hex) at info level if present, warn if absent. Pass to `AppState`.
- Set `NOTARY_PUBLIC_KEY` on the Fly.io backend: `fly secrets set NOTARY_PUBLIC_KEY="$(curl -s https://commit-verifier.fly.dev/info | jq -r .publicKey)" --app commit-backend`
- This unit deliberately does NOT implement verification logic — that's the next follow-up item. This unit only stores the key so the follow-up can use it without another config change.

**Patterns to follow:**
- `GITHUB_TOKEN` optional env var pattern in `main.rs`
- `AppState` field pattern in `lib.rs`

**Test scenarios:**
- Happy path: backend starts without `NOTARY_PUBLIC_KEY` env var → `notary_public_key` is `None`, no error, warn logged
- Happy path: backend starts with `NOTARY_PUBLIC_KEY` set → `notary_public_key` is `Some(pem_string)`, info logged
- Integration: existing API tests pass with the `test_app()` helper updated to include `notary_public_key: None` — the new `AppState` field is required by the struct but `None` means tests don't need a real key

**Verification:**
- `cargo test` passes with no regressions
- `cargo clippy -- -D warnings` clean
- `fly deploy --app commit-backend` succeeds
- Backend logs show the notary public key fingerprint on startup

## System-Wide Impact

- **Interaction graph:** The notary server is used only by the extension (via `tlsn-js` in the offscreen WASM worker). The backend does not communicate with the notary server. The backend's webhook endpoint (`POST /webhook/endorsement`) is unaffected — it accepts webhooks from a verifier server, which is a different component not deployed in this plan.
- **Error propagation:** If the own notary server is down, the extension's WASM worker will hang for 60 seconds (existing behavior with any unreachable notary). No impact on trust card viewing, scoring, or other extension features.
- **State lifecycle risks:** The signing key must survive container restarts. The entrypoint script re-writes it from the Fly secret on every startup, so the key persists as long as the Fly secret exists. No volume-based state.
- **API surface parity:** The `POST /endorsements` and `POST /webhook/endorsement` endpoints are unchanged. The `GET /info` endpoint on the notary is a new surface but it's read-only and public.
- **Unchanged invariants:** All backend API endpoints, Commit Score computation, trust card rendering, badge generation — all unchanged. The extension's content scripts and GitHub/Google injection — unchanged. Only the notarization connection URL changes.
- **Chrome Web Store impact:** Adding `host_permissions` for `commit-verifier.fly.dev` requires a CWS review (typically 1-3 business days). Existing users will receive the update automatically after approval.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| CWS review delays for new host_permissions | Submit extension update early. The extension continues to work with `notary.pse.dev` until the update is approved. Can deploy notary server and backend changes ahead of extension update. |
| 512MB RAM insufficient for MPC-TLS operations | Monitor with `fly machine status`. The public notary handles this fine, so resource requirements are known. Bump to 1GB if needed. |
| Signing key leaked via Fly secret | Fly.io secrets are encrypted at rest and only exposed to the running container as env vars. Standard practice. Rotate by generating new key, updating secret, and re-pinning public key in backend. |
| `notary.pse.dev` proxy goes down | Extension loses endorsement capability but all other features work. Deploy own proxy as a follow-up (the proxy is ~50 lines of WebSocket-to-TCP bridging). |
| Notary server version drift with extension tlsn-js | All pinned to v0.1.0-alpha.12. Do not upgrade one without upgrading all three (extension, notary image, backend attestation parsing). |
| Fly.io auto-stop causes first-request latency | The notary server is a Rust binary with fast cold start (~1-2s). Acceptable for endorsement flow (user is already waiting ~5s for MPC-TLS proving). |
| Fly.io HTTP proxy interferes with MPC-TLS WebSocket | Fly.io HTTP services support WebSocket natively. MPC-TLS sessions are ~5s of bidirectional binary frames — well within normal WebSocket use. Verify with the sideload test in Unit 2 before CWS submission. |
| PEM newlines mangled in Fly secret | Use command substitution (`"$(openssl ...)"`) which preserves newlines. Verify with `fly ssh console` after deploy (see operational notes). |

## Documentation / Operational Notes

- **Signing key generation and secret setup** (preserving PEM newlines):
  ```bash
  fly secrets set NOTARY_SIGNING_KEY="$(openssl ecparam -genkey -name secp256k1 -noout | openssl pkcs8 -topk8 -nocrypt)" --app commit-verifier
  ```
  Verify the key is intact after deployment: `fly ssh console --app commit-verifier -C "head -1 /root/.notary/notary.key"` should show `-----BEGIN PRIVATE KEY-----`
- After Unit 1 deployment: run `curl https://commit-verifier.fly.dev/info` and save the `publicKey` value — needed for Unit 3
- **Key rotation coupling:** When the notary signing key is rotated, the backend's `NOTARY_PUBLIC_KEY` must also be updated. Failure to do so will break attestation verification (once implemented in the follow-up).
- Set `VERIFIER_WEBHOOK_SECRET` on both `commit-backend` and `commit-verifier` Fly apps if webhook flow is used later
- Update CLAUDE.md Phase 2 checklist after all units land

## Sources & References

- CEO plan: `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md`
- P0 security plan: `docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md` (loose threads #3, #5, #6 unblocked)
- TLSNotary WASM integration: `docs/solutions/best-practices/tlsnotary-wasm-chrome-extension-integration-2026-04-11.md`
- TLSNotary notary server: Docker image `ghcr.io/tlsnotary/tlsn/notary-server:v0.1.0-alpha.12`, config via YAML or `NS_` env vars
- TLSNotary notary server `/info` endpoint: returns `{ version, publicKey (SPKI PEM), gitCommitHash }`
