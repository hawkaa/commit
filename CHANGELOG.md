# Changelog

All notable changes to Commit will be documented in this file.

## [0.2.0.0] - 2026-04-13

Phase 3 sprint: "Endorse Everywhere + Launch" — parallel-worktree execution of four feature plans. Shipped 4 user-facing features plus the code-level realization of the 2026-04-12 one-network decision. 27 commits across backend + extension.

### Added
- Post-install onboarding page (`extension/src/onboarding.html`) opens on fresh extension install with a primary CTA to visit a GitHub repo. Webpack `CopyWebpackPlugin` entry emits it to `extension/build/onboarding.html` at build time.
- "Get the Commit extension" install CTA on the SSR trust page (`src/routes/trust_page.rs`), positioned between the endorsements card and the badge section. Links to the Chrome Web Store (URL is a placeholder until CWS approval — tagged with a TODO in `CHROME_WEBSTORE_URL`).
- "Add badge" clipboard CTA on the extension-injected GitHub trust card (`extension/src/content-github.ts`, `trust-card.css`). Copies a Markdown snippet with absolute `API_BASE` URLs; falls back to a `user-select: all` code block when `navigator.clipboard.writeText` is unavailable. Re-entrant click guard prevents race during the "Copied!" flash window.
- Orphaned `chrome.storage.local.keyring` cleanup on extension update (`chrome.runtime.onInstalled` `reason === "update"` branch) — removes leftover data from the deprecated personal keyring model.
- `network_query_endpoint_removed` regression test in `tests/api.rs` that asserts `POST /network-query` returns `404` — locks in the one-network model at the API layer.
- Extension popup now shows a local endorsement counter (`chrome.storage.local.endorsement_count`), incremented after successful `POST /endorsements` calls.
- Design-spec-compliant Playwright smoke assertions for the "Add badge" CTA and the onboarding tab open on `reason === "install"`.

### Changed
- **Product model:** "One global network" is now the code-level reality, replacing the personal friend-graph framing. "N endorse this" = N verified humans, not N of a user's friends. ZK anonymity is the trust primitive.
- Extension popup (`extension/src/popup.{ts,html,css}`) rewritten from keyring management to a minimal status card: truncated public key (with copy-to-clipboard) + endorsement count + "About Commit" link.
- `chrome.runtime.onInstalled` listener now accepts the `details` argument. Keypair generation and the onboarding tab open are wrapped in independent `try/catch` blocks so one concern never blocks the other.
- Onboarding page outbound links carry `rel="noopener noreferrer"` (defense-in-depth for extension pages linking to external origins).
- Playwright onboarding detection uses deterministic `context.waitForEvent('page')` + `context.pages()` scan instead of a busy-wait polling loop.
- Phase 3 checklist in `CLAUDE.md` restructured around the "Endorse Everywhere + Launch" scope from the 2026-04-12 CEO plan. Phase 2 hardening backlog marked fully complete.
- Design doc `NetworkMembership` entity explicitly noted as superseded by the one-network decision.

### Fixed
- Endorsement counter storage failure (`chrome.storage.local.set` rejection) no longer masks a successful endorsement as a `Network error` response to the caller. Counter is now best-effort inside its own inner `try/catch`.
- "Add badge" CTA re-entrancy: double-clicking during the 1500ms "Copied!" window no longer queues duplicate clipboard writes or races between the success `setTimeout` and a late-arriving `catch` fallback.
- Stale fallback block on "Add badge" CTA is now hidden when a subsequent copy succeeds (was previously left visible indefinitely).
- Test assertion for `target="_blank"` in the trust page CTA now uses a raw-string delimiter (`r##"..."##`) that correctly matches the full attribute value instead of a prefix-only substring.
- Playwright smoke test now fails loud (`test.skip` with reason) when the GitHub trust card is absent, instead of silently passing with zero coverage of the new `.commit-add-badge` assertion.

### Removed
- `POST /network-query` endpoint (`src/routes/network.rs` deleted, route registration in `src/main.rs` removed, `pub mod network` line dropped from `src/routes/mod.rs`).
- `count_network_endorsements` method in `src/services/db.rs` and the 10 `network_query_*` integration tests in `tests/api.rs` (plus `setup_network_test_data` helper).
- `NETWORK_QUERY` / `KEYRING_ADD` / `KEYRING_REMOVE` message handlers, `keyringMutex`, `handleKeyringAdd`, `handleKeyringRemove`, and `handleNetworkQuery` in the extension service worker.
- `NetworkData` interface, `network_data` field on `TrustCardData`, and the `NETWORK_QUERY` call block in the GitHub content script.
- Personal keyring UI (friend list + "Add to network" form) and associated CSS classes (`.keyring-list`, `.keyring-entry`, `.keyring-add`, `.popup-input`, `.popup-btn--danger`) in the extension popup.

## [0.1.3.0] - 2026-04-12

### Added
- Server-side ECDSA-secp256k1 attestation signature verification using `tlsn-core`, `k256`, and `bcs`
- `verify_attestation_signature()` function in `src/validation.rs` that deserializes TLSNotary attestations from BCS and verifies the notary's signature
- Signature algorithm validation (rejects non-secp256k1 attestations)
- Solution documentation for the verification approach (`docs/solutions/security-issues/`)

### Changed
- `AppState.notary_public_key` upgraded from raw PEM string to parsed `k256::ecdsa::VerifyingKey` (parsed once at startup)
- Webhook `attestation` field is now required (was `Option<String>`)
- `tlsn-core` dependency pinned by commit SHA instead of mutable git tag

### Removed
- `hash_verification_results_with_transcript` fallback function (webhook always requires attestation now)
- Webhook backward-compatibility path for missing attestation data

## [0.1.2.0] - 2026-04-12

### Added
- Own TLSNotary notary server deployed to Fly.io (`commit-verifier.fly.dev`) with persistent secp256k1 signing key
- Notary server Dockerfile, config.yaml, and entrypoint.sh for key injection from Fly secrets
- `NOTARY_PUBLIC_KEY` config in backend AppState for future attestation signature verification
- Extension host_permissions for the own notary server domain

### Changed
- Extension `NOTARY_URL` points to `commit-verifier.fly.dev` instead of public `notary.pse.dev`
- WebSocket proxy (`PROXY_BASE`) remains on `notary.pse.dev` (stateless, deferred)
- Notary server fly.toml switched from image reference to local Dockerfile build

## [0.1.1.0] - 2026-04-11

### Added
- TLSNotary MPC-TLS proving in Chrome extension via offscreen WASM document (~5s proof generation)
- "Endorse" button on GitHub trust cards for ZK-verified endorsements
- Webhook endpoint (POST /webhook/endorsement) for receiving verified proofs from TLSNotary verifier
- Fail-closed webhook authentication with VERIFIER_WEBHOOK_SECRET
- Server name validation per proof type (api.github.com for git_history, Google/Outlook for email)
- Verifier server Fly.io deployment config (verifier/fly.toml)
- 5 webhook endpoint tests (auth, validation, server name, 404)

### Changed
- Extension bumped to v0.2.0 with offscreen permission and wasm-unsafe-eval CSP
- Replaced innerHTML with DOM construction in content script (XSS prevention)
- Corrected ZK documentation: TLSNotary uses MPC-TLS + QuickSilver, not Halo2

## [0.1.0.0] - 2026-04-11

### Added
- L2 attestation registry contract deployed on Base Sepolia testnet
- `attest()` and `attestBatch()` for recording endorsement proof hashes on-chain
- `verify()` for public proof hash verification
- Owner-only access control with `transferOwnership()` escape hatch
- Zero proof hash rejection and MAX_BATCH_SIZE=500 safety guards
- 20 Foundry tests covering all functions, revert paths, and edge cases
- Deployment receipt at `contracts/deployments/base-sepolia.json`
- Foundry project setup (Solidity 0.8.30, forge-std v1.15.0)
