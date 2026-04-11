# Commit

Behavioral trust layer that surfaces ZK-verified commitment signals alongside search results and GitHub repos.

## Design Documents

Read these before making any implementation decisions:

- **Design doc:** `~/.gstack/projects/commit/hakon-unknown-design-20260410-131531.md`
- **CEO plan:** `~/.gstack/projects/commit/ceo-plans/2026-04-10-commit-trust-network.md`
- **Test plan:** `~/.gstack/projects/commit/hakon-unknown-eng-review-test-plan-20260410-133500.md`
- **Design system:** `DESIGN.md` (in project root)

## Stack

- **Backend:** Rust (axum, rusqlite, reqwest). Deploy to Fly.io.
- **Extension:** Chrome Manifest V3. Content scripts for GitHub + Google SERP.
- **Database:** SQLite (operational) + Ethereum L2 (attestation hashes).
- **ZK proofs:** TLSNotary MPC-TLS + QuickSilver (endorsement proofs). WASM in offscreen document.

## Key Decisions

- Subject entity is polymorphic (github_repo, npm_package, business, service). Phase 1 implements github_repo + business only.
- Commit Score (0-100) is the brand primitive. Score-first trust card hierarchy.
- Ed25519 keypair auth. No accounts, no PII.
- Access gate OPEN in Phase 1. Activates Phase 3.
- Seed endorsements from founder to bootstrap cold start.
- Click score → navigates to trust card page (commit.dev/trust/...).

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

### Phase 2 — TLSNotary + Endorsements (weeks 3-6)
- [x] TLSNotary research spike (MPC-TLS, ~5s proving time benchmarked)
- [x] Extension offscreen WASM integration (tlsn-wasm, offscreen.html/js)
- [x] Backend webhook endpoint (POST /webhook/endorsement)
- [ ] Own Notary server (Docker image + Fly.io, using public notary.pse.dev for PoC)
- [ ] ZK-verified endorsement flow end-to-end
- [ ] P0: Bind proof_hash to cryptographic attestation (currently hashes attacker-controlled payload fields)
- [ ] P0: Bind session.data subject to proof transcript (proof for repo A can currently endorse repo B)
- [ ] Network keyring + key sharing
- [ ] L2 attestation for endorsements
- [ ] Commit Score v2 (Layer 1 + Layer 2)

### Phase 3 — Network + Launch (weeks 6-9)
- [ ] "N in your network endorse this" display
- [ ] Access gate activation
- [ ] Seed endorsements from founder
- [ ] Launch: HN, crypto Twitter, Rust community

## Testing

```bash
cargo test
cargo clippy -- -D warnings
```
