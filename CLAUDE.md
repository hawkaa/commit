# Commit

Behavioral trust layer that surfaces ZK-verified commitment signals alongside search results and GitHub repos.

## Design Documents

Read these before making any implementation decisions:

- **Design doc:** `~/.gstack/projects/commit/hakon-unknown-design-20260410-131531.md`
  - Note: the `NetworkMembership` entity in this doc is **superseded** by the one-network decision (2026-04-12). Ignore the personal key-sharing model.
- **CEO plan (original):** `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md`
- **CEO plan (Phase 3):** `~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md` — one-network model, scope decisions, implementation notes
- **Test plan (original):** `~/.gstack/projects/commit/hakon-unknown-eng-review-test-plan-20260410-133500.md`
- **Test plan (Phase 3):** `~/.gstack/projects/commit/hakon-main-eng-review-test-plan-20260412-210049.md`
- **Design audit:** `~/.gstack/projects/commit/designs/design-audit-20260412/design-audit-commit-backend.md` — 8 findings, all deferred to Phase 3
- **Design system:** `DESIGN.md` (in project root)
- **Documented solutions:** `docs/solutions/` — past problems and best practices with YAML frontmatter (`module`, `tags`, `problem_type`), relevant when implementing or debugging in documented areas

## Stack

- **Backend:** Rust (axum, rusqlite, reqwest). Deploy to Fly.io.
- **Extension:** Chrome Manifest V3. Content scripts for GitHub + Google SERP.
- **Database:** SQLite (operational) + Ethereum L2 (attestation hashes).
- **ZK proofs:** TLSNotary MPC-TLS + QuickSilver (endorsement proofs). WASM in offscreen document.

## Key Decisions

- Subject entity is polymorphic (github_repo, npm_package, business, service). Phase 1 implements github_repo + business only.
- Commit Score (0-100) is the brand primitive. Score-first trust card hierarchy.
- Ed25519 keypair auth. No accounts, no PII.
- Access gate OPEN in Phase 1-2. Deferred beyond Phase 3 launch (too few users to gate).
- Seed endorsements from founder to bootstrap cold start.
- Click score → navigates to trust card page (commit.dev/trust/...).
- **ONE network model** (2026-04-12): Commit is one global network, not personal friend graphs. "N endorse this" = N verified humans, not N of your friends. ZK anonymity is the trust primitive, not social proximity. The design doc's `NetworkMembership` personal key-sharing model is superseded. The personal keyring code (popup keyring UI, `POST /network-query`, `NETWORK_QUERY` handler) should be removed in Phase 3.

## Design System

Always read DESIGN.md before making any visual or UI decisions.
All font choices, colors, spacing, and aesthetic direction are defined there.
Do not deviate without explicit user approval.

## Phase 1 Progress

### Phase 1a — Core (weeks 1-2)
- [x] Rust backend (axum + SQLite)
- [x] GET /trust-card (GitHub repos, caching, Commit Score)
- [x] GET /badge/{kind}/{id}.svg (color-coded SVG)
- [x] GET /trust/{kind}/{id} (SSR trust card page)
- [x] POST /endorsements + GET /endorsements
- [x] Commit Score algorithm (Layer 1)
- [x] Chrome extension: content script for GitHub
- [x] Chrome extension: content script for Google SERP
- [x] Chrome extension: keypair generation
- [x] Deploy to Fly.io
- [x] Submit to Chrome Web Store

### Phase 1b — Growth surfaces (weeks 2-3)
- [x] Trust card page (SSR)
- [x] Badge SVG endpoint
- [x] MCP server (thin wrapper over trust-card API)
- [x] OG meta tags on trust card pages (social previews)
- [x] L2 contract deployment (Base Sepolia: `0x08AE2e7fd94130645725Afc69e9BE2140f2395d7`)

### Infrastructure
- [x] GitHub Actions CI/CD: auto-deploy backend + verifier to Fly.io on push to main (test-gated)

### Phase 2 — TLSNotary + Endorsements (weeks 3-6)
- [x] TLSNotary research spike (MPC-TLS, ~5s proving time benchmarked)
- [x] Extension offscreen WASM integration (tlsn-wasm, offscreen.html/js)
- [x] Backend webhook endpoint (POST /webhook/endorsement)
- [x] Own Notary server (Docker image + Fly.io, using public notary.pse.dev for PoC) — see `docs/plans/2026-04-12-001-feat-own-notary-server-plan.md`
- [x] ZK-verified endorsement flow end-to-end — see `docs/plans/2026-04-12-004-feat-e2e-endorsement-flow-plan.md`
- [x] P0: Bind proof_hash to cryptographic attestation (currently hashes attacker-controlled payload fields) — see `docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md`
- [x] P0: Bind session.data subject to proof transcript (proof for repo A can currently endorse repo B) — see same plan
- [x] Follow-up from P0 plan: email proof type transcript binding — see `docs/plans/2026-04-12-005-fix-security-hardening-batch-plan.md`
- [x] Follow-up from P0 plan: ci_logs proof type transcript binding — see same plan
- [x] Follow-up from P0 plan: full attestation signature verification (requires own notary server)
- [x] Follow-up from P0 plan: attestation nonce-based replay prevention / rate limiting — see same plan
- [x] Follow-up from P0 plan: deprecate webhook hash_verification_results fallback
- [x] Follow-up from P0 plan: score integrity without device binding (weight pending_attestation lower) — see same plan
- [x] Follow-up from P0 plan: validate single HTTP request line in revealed transcript (pipelining defense) — see same plan
- [x] Network keyring + key sharing — see `docs/plans/2026-04-12-006-feat-network-keyring-key-sharing-plan.md`
- [x] L2 attestation for endorsements — see `docs/plans/2026-04-12-007-feat-l2-attestation-submission-plan.md`
- [x] Commit Score v2 (Layer 1 + Layer 2) — see `docs/plans/2026-04-12-008-feat-commit-score-v2-plan.md`

### Phase 3 — Endorse Everywhere + Launch (weeks 6-9)
See CEO plan: `~/.gstack/projects/commit/ceo-plans/2026-04-12-phase3-one-network-endorsements.md`
- [x] SERP card: add endorsement count + endorse button (parity with GitHub card)
- [x] Trust page: add "Get extension" CTA (growth loop)
- [x] Remove dead keyring code (popup keyring UI, POST /network-query, NETWORK_QUERY handler)
- [x] "Not for me" negative endorsement signal (sentiment field, upsert, score impact)
- [x] "You endorsed this" revisit indicator (local cache)
- [x] "Add badge to README" CTA on GitHub trust cards (clipboard)
- [x] Post-install onboarding page (closes growth loop conversion cliff)
- [x] Design fixes: absolute badge URLs, install CTA, focus-visible, score animation (8 findings from design audit)
- [ ] Replace `CHROME_WEBSTORE_URL` placeholder in `src/routes/trust_page.rs` after Chrome Web Store approval
- [ ] Seed endorsements from founder
- [ ] Launch: HN, crypto Twitter, Rust community

## Testing

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

All three gates must pass before pushing to `main`. CI (`.github/workflows/deploy.yml`) runs `cargo fmt --check` as part of the "Rust checks" job, and a failure there blocks the Fly.io auto-deploy. Run `cargo fmt` locally to fix formatting drift.
