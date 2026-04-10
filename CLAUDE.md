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
- **ZK proofs:** TLSNotary (endorsement proofs). Halo2 reserved for aggregation only.

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

## Testing

```bash
cargo test
cargo clippy -- -D warnings
```
