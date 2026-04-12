---
title: "feat: GitHub Actions CI/CD pipeline"
type: feat
status: active
date: 2026-04-12
---

# feat: GitHub Actions CI/CD pipeline

## Overview

Add GitHub Actions CI/CD that runs tests, linting, and format checks on every PR, and auto-deploys the backend and verifier to Fly.io on push to main — gated on all checks passing. This eliminates manual deploys, catches regressions before merge, and ensures main is always deployable.

## Problem Frame

There is no CI/CD. Deploys are manual `fly deploy` from a local machine. No automated test gate exists — a broken commit can land on main and be deployed without anyone noticing until production breaks. The backend has `cargo test` and `cargo clippy -- -D warnings` but nothing enforces them. The extension has a webpack build and ESLint but no automated validation. This is the single largest operational gap for a solo founder: one bad merge with no gate means a broken production site.

## Requirements Trace

- R1. PRs must pass `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` before merge
- R2. PRs must pass extension build (`npm run build`) and lint (`npm run lint`) to catch TypeScript breakage
- R3. Push to `main` must auto-deploy `commit-backend` to Fly.io, gated on all tests passing
- R4. Push to `main` must auto-deploy `commit-verifier` to Fly.io only when `verifier/` files change
- R5. Rust dependency builds must be cached aggressively — the `tlsn-core` git dep makes uncached builds very slow
- R6. Secrets (`FLY_API_TOKEN`) must be stored as GitHub Actions secrets, never in workflow files

## Scope Boundaries

- Extension deployment to Chrome Web Store is NOT included (CWS requires manual review; automated publish is deferred)
- Playwright E2E tests for the extension are NOT run in CI (require a real Chrome instance with extension sideloading — complex to set up, low ROI at current scale)
- Database migrations are NOT applicable (SQLite with `CREATE TABLE IF NOT EXISTS` at startup)
- Branch protection rules are recommended but not enforced by this plan (single maintainer can configure GitHub settings manually)
- Fly.io health checks after deploy are handled by Fly.io's built-in machine health checks, not by the CI pipeline

### Deferred to Separate Tasks

- Extension E2E testing in CI (Playwright + Chrome sideloading)
- Automated Chrome Web Store publishing (CWS API)
- Performance benchmarking in CI (build time tracking, binary size)
- Dependabot or Renovate for dependency updates

## Context & Research

### Relevant Code and Patterns

- `Dockerfile` — multi-stage Rust build on `rust:1.94-alpine`, produces `scratch` image with `commit-backend` binary
- `verifier/Dockerfile` — wraps `ghcr.io/tlsnotary/tlsn/notary-server:v0.1.0-alpha.12` with custom config and entrypoint
- `fly.toml` — backend Fly.io config: app `commit-backend`, region `arn`, port 3000, volume mount
- `verifier/fly.toml` — verifier Fly.io config: app `commit-verifier`, region `arn`, port 7047, 512MB memory
- `Cargo.toml` — edition 2024, Rust 1.94, `tlsn-core` pinned by git rev `f2ff4ba7`, `rusqlite` with `bundled` feature
- `Cargo.lock` — version 4 lockfile (must be present for reproducible builds)
- `extension/package.json` — scripts: `build` (webpack production), `lint` (eslint), `test` (playwright)
- `tests/api.rs`, `tests/score.rs` — integration tests using `axum-test`, `serial_test`
- No `rust-toolchain.toml` exists — Rust version must be pinned explicitly in the workflow
- No `.rustfmt.toml` exists — default `rustfmt` rules apply

### Build Characteristics

- **Uncached Rust build:** ~8-15 minutes. The `tlsn-core` git dependency pulls a large workspace and compiles substantial crypto code. Dev dependencies (`axum-test`) also add time.
- **Cached Rust build (deps only):** ~1-3 minutes for incremental source changes when `target/` and cargo registry are cached.
- **Verifier build:** <30 seconds. Just copies files onto a pre-built Docker image. No compilation.
- **Extension build:** ~15 seconds. Webpack + TypeScript compilation.
- **`serial_test` constraint:** Tests using `#[serial]` manipulate env vars globally and must not run concurrently. `cargo test` runs with the default thread count but `serial_test` handles synchronization internally — no `--test-threads=1` flag needed unless tests are flaky.

## Key Technical Decisions

- **Two workflow files, not one:** A `ci.yml` for PR checks and a `deploy.yml` for main-branch deploys. Separation keeps the CI workflow fast (no deploy steps) and the deploy workflow focused (no duplicate test matrix). The deploy workflow reuses the same test jobs as a gate before deploying.

- **`Swatinem/rust-cache` for dependency caching:** This is the standard GitHub Actions cache for Rust. It caches `~/.cargo/registry`, `~/.cargo/git`, and `target/`. The `tlsn-core` git checkout is cached under `~/.cargo/git/` which is the critical win — without this, every build re-clones the TLSNotary monorepo. Cache key includes `Cargo.lock` hash so cache invalidates on dependency changes.

- **Pin Rust version explicitly in workflow:** No `rust-toolchain.toml` exists. Use `dtolnay/rust-toolchain@stable` with `toolchain: 1.94.0` to match the Docker build and local development. This prevents breakage from rustc version drift.

- **`superfly/flyctl-action` + `fly deploy` for deploys:** The official Fly.io GitHub Action installs `flyctl`. Deploy uses `fly deploy` which builds the Docker image on Fly.io's remote builder (consistent with local deploy workflow). No need to push images to a registry.

- **Path filter for verifier deploys:** Use `paths` filter or `dorny/paths-filter` to only deploy the verifier when files in `verifier/` change. The verifier is a thin wrapper around an upstream image — it changes rarely and should not redeploy on every backend code change.

- **Extension validation in CI but no deploy:** Run `npm ci && npm run build && npm run lint` in the extension directory. This catches TypeScript errors and lint failures early. No deploy step because Chrome Web Store publishing is manual.

- **`cargo fmt` on nightly:** `rustfmt` on stable Rust may lack features. However, since no `.rustfmt.toml` with nightly features exists, stable `rustfmt` is sufficient. Use the same `1.94.0` toolchain with `components: rustfmt, clippy` to keep it simple.

## Implementation Units

- [ ] **Unit 1: CI workflow for pull requests**

**Goal:** Run format check, clippy, tests, and extension validation on every PR so broken code cannot merge.

**Requirements:** R1, R2

**Dependencies:** None

**Files:**
- Create: `.github/workflows/ci.yml`

**Approach:**

The workflow triggers on `pull_request` targeting `main`. It has two jobs that run in parallel:

**Job 1: `rust` (runs on `ubuntu-latest`)**
1. Checkout code
2. Install Rust 1.94.0 via `dtolnay/rust-toolchain@stable` with `toolchain: 1.94.0` and `components: rustfmt, clippy`
3. Cache dependencies via `Swatinem/rust-cache@v2`
4. `cargo fmt --check` — fail fast on formatting issues
5. `cargo clippy -- -D warnings` — fail on any lint warning
6. `cargo test` — run all tests

Steps 4-6 are sequential (fmt is fastest, clippy catches issues before slower test run).

**Job 2: `extension` (runs on `ubuntu-latest`)**
1. Checkout code
2. Install Node.js via `actions/setup-node@v4` with `node-version: 20`
3. `npm ci` in `extension/` (deterministic install from lockfile)
4. `npm run build` in `extension/` (webpack production build)
5. `npm run lint` in `extension/` (ESLint)

If `extension/package-lock.json` does not exist, use `npm install` instead of `npm ci`. Check during implementation.

**Patterns to follow:**
- Standard GitHub Actions workflow syntax
- `Swatinem/rust-cache` default configuration (cache key based on `Cargo.lock`)
- `working-directory: extension` for Node.js steps

**Test scenarios:**
- PR with a formatting issue: `cargo fmt --check` fails, workflow fails, PR is not mergeable
- PR with a clippy warning: `cargo clippy -- -D warnings` fails
- PR with a failing test: `cargo test` fails
- PR with a TypeScript error: `npm run build` fails
- PR with an ESLint violation: `npm run lint` fails
- Clean PR: all steps pass, workflow succeeds

**Verification:**
- Open a test PR with clean code — all checks pass (green)
- Open a test PR with intentional `cargo fmt` violation — rust job fails
- Open a test PR with intentional clippy warning — rust job fails
- Confirm cached builds are significantly faster than uncached (check job duration on second run)

---

- [ ] **Unit 2: Deploy workflow for backend**

**Goal:** Auto-deploy `commit-backend` to Fly.io on every push to `main`, gated on tests passing.

**Requirements:** R3, R5, R6

**Dependencies:** Unit 1 (reuses the same test/lint pattern as a gate)

**Files:**
- Create: `.github/workflows/deploy.yml`

**Approach:**

The workflow triggers on `push` to `main`. It has a test gate job followed by deploy jobs.

**Job 1: `test` (runs on `ubuntu-latest`)**
Same steps as the `rust` job in `ci.yml`: checkout, install Rust 1.94.0, cache, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`. This duplicates the CI checks intentionally — a direct push to main (merge commit) must be validated even if the PR checks passed, because merge conflicts can introduce breakage.

**Job 2: `deploy-backend` (runs on `ubuntu-latest`, `needs: test`)**
1. Checkout code
2. Install flyctl via `superfly/flyctl-action@1.5`
3. `fly deploy --app commit-backend` with env `FLY_API_TOKEN: ${{ secrets.FLY_API_TOKEN }}`

The `needs: test` ensures deploy only runs if tests pass. If `test` fails, `deploy-backend` is skipped.

**Secrets required (set in GitHub repo settings):**
- `FLY_API_TOKEN` — Fly.io API token with deploy access to both `commit-backend` and `commit-verifier` apps

**Patterns to follow:**
- `superfly/flyctl-action` official setup pattern
- `FLY_API_TOKEN` as environment variable (flyctl reads it automatically)
- `needs:` for job dependency

**Test scenarios:**
- Push to main with passing tests: backend deploys successfully
- Push to main with failing tests: deploy is skipped
- `FLY_API_TOKEN` missing: deploy job fails with clear error

**Verification:**
- Merge a PR to main — observe the workflow run: test job passes, deploy-backend job runs, Fly.io deploy succeeds
- Check `fly status --app commit-backend` after deploy — new image is running
- Check `fly logs --app commit-backend` — application starts normally

---

- [ ] **Unit 3: Deploy workflow for verifier (path-filtered)**

**Goal:** Auto-deploy `commit-verifier` to Fly.io only when files in `verifier/` change, avoiding unnecessary redeploys.

**Requirements:** R4, R6

**Dependencies:** Unit 2 (added to the same `deploy.yml` workflow)

**Files:**
- Modify: `.github/workflows/deploy.yml`

**Approach:**

Add a path-detection job and a conditional deploy job to the existing `deploy.yml`:

**Job 3: `changes` (runs on `ubuntu-latest`)**
Uses `dorny/paths-filter@v3` to detect whether any files in `verifier/` changed in the push. Outputs a boolean `verifier` output.

Alternative (simpler, no external action): Use `git diff --name-only HEAD~1` to check for `verifier/` changes. However, `dorny/paths-filter` handles merge commits correctly (compares against the merge base, not just `HEAD~1`), so it's more robust.

**Job 4: `deploy-verifier` (runs on `ubuntu-latest`, `needs: [test, changes]`, `if: needs.changes.outputs.verifier == 'true'`)**
1. Checkout code
2. Install flyctl via `superfly/flyctl-action@1.5`
3. `fly deploy --config verifier/fly.toml --app commit-verifier` with env `FLY_API_TOKEN: ${{ secrets.FLY_API_TOKEN }}`

Note the `--config verifier/fly.toml` flag — flyctl needs to be pointed to the verifier's config since it's not at the repo root. The Dockerfile path is relative to the `fly.toml` location, so `verifier/Dockerfile` is resolved correctly.

**Patterns to follow:**
- `dorny/paths-filter` standard usage for conditional deploys
- Same `superfly/flyctl-action` pattern as backend deploy
- `if:` conditional on job outputs

**Test scenarios:**
- Push to main with changes only in `src/`: verifier deploy is skipped, backend deploys
- Push to main with changes in `verifier/`: both backend and verifier deploy
- Push to main with changes only in `verifier/`: backend deploys (always), verifier deploys (path match)

**Verification:**
- Merge a PR that changes only `src/` — verify `deploy-verifier` job is skipped in the workflow run
- Merge a PR that changes `verifier/config.yaml` — verify `deploy-verifier` job runs and deploys successfully
- Check `fly status --app commit-verifier` after deploy

---

- [ ] **Unit 4: Dependency caching strategy and GitHub secrets setup**

**Goal:** Ensure Rust builds are fast by caching aggressively, and document the required GitHub secrets setup.

**Requirements:** R5, R6

**Dependencies:** Units 1-3

**Files:**
- Modify: `.github/workflows/ci.yml` (caching is configured here)
- Modify: `.github/workflows/deploy.yml` (same caching)

**Approach:**

**Caching configuration (already included in Units 1-2, documented here for completeness):**
- `Swatinem/rust-cache@v2` with default settings. Cache key is based on `Cargo.lock` hash, OS, and Rust version. The cache includes:
  - `~/.cargo/registry/index/` — crates.io index
  - `~/.cargo/registry/cache/` — downloaded crate archives
  - `~/.cargo/git/db/` — git dependency clones (this is the critical one for `tlsn-core`)
  - `target/` — compiled artifacts (debug mode for tests, optimized for release)
- Cache size limit: GitHub Actions provides 10GB per repo. A full Rust `target/` with TLSNotary deps is ~2-4GB. Two caches (CI + deploy) fit comfortably.
- Cache TTL: GitHub evicts caches not accessed in 7 days. Since CI runs on every PR and deploy runs on every merge, caches stay warm.

**Node.js caching:**
- `actions/setup-node@v4` with `cache: 'npm'` and `cache-dependency-path: 'extension/package-lock.json'` caches `~/.npm`. Alternatively, if no lockfile exists, skip npm caching.

**GitHub secrets to configure (manual, one-time):**
1. `FLY_API_TOKEN` — obtain via `fly tokens create deploy -x 999999h` (long-lived deploy token). Scope: both `commit-backend` and `commit-verifier` apps. Set in GitHub repo Settings > Secrets and variables > Actions > New repository secret.

**Verification:**
- First CI run: uncached, observe full build time (~8-15 min for Rust)
- Second CI run (same deps): cached, observe build time drop (~1-3 min)
- Check GitHub Actions cache tab: verify cache entries exist for `rust-cache` and `npm`
- After a `Cargo.lock` change: verify cache misses and a new cache is created

## System-Wide Impact

- **No impact on application code.** This plan adds only workflow files under `.github/workflows/`. No Rust source, extension code, Dockerfiles, or Fly.io configs are modified.
- **Deploy behavior changes from manual to automated.** After this plan lands, pushing to `main` triggers a deploy. The team (solo founder) must be aware that merging a PR = deploying to production. This is intentional — the test gate provides confidence, and the current manual flow already deploys immediately after merge.
- **Build minutes consumption.** GitHub Actions provides 2,000 free minutes/month for private repos (unlimited for public). Rust builds are ~8-15 min uncached, ~1-3 min cached. At ~5-10 PRs/week, expect ~50-100 min/week of CI time. Well within free tier for a public repo.
- **Secret management.** A single `FLY_API_TOKEN` is added to GitHub secrets. This token has deploy access to both Fly.io apps. If the GitHub repo is compromised, the attacker could deploy arbitrary code to Fly.io. Mitigation: the repo is private (or if public, branch protection prevents unauthorized pushes to main). Fly.io deploy tokens can be scoped and rotated.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| `tlsn-core` git dep causes slow uncached builds (10+ min) | `Swatinem/rust-cache` caches `~/.cargo/git/db/` which contains the cloned TLSNotary repo. Cached builds skip the clone entirely. First build after a `Cargo.lock` change will be slow — acceptable. |
| `Swatinem/rust-cache` cache eviction (7-day TTL) | CI runs on every PR, deploy runs on every merge. As long as development is active (at least 1 PR/week), caches stay warm. Stale caches during vacation are acceptable — first build just takes longer. |
| `cargo test` flakiness from `serial_test` env var manipulation | `serial_test` handles synchronization internally. If flakiness appears, add `--test-threads=1` to the `cargo test` command. Monitor first few CI runs. |
| Fly.io remote builder failures | Fly.io remote builders occasionally fail due to capacity. Retry logic: GitHub Actions `retry` step or manual re-run. Not worth automating retry for now — failures are rare. |
| `FLY_API_TOKEN` expiration | Use a long-lived deploy token (`-x 999999h`). Set a calendar reminder to rotate annually. |
| Merge to main with failing tests (force push or admin bypass) | Deploy workflow runs its own test job — even force pushes are gated. Cannot fully prevent admin bypass, but the test gate catches accidental breakage. |
| `npm ci` fails because `extension/package-lock.json` doesn't exist | Check during implementation. If no lockfile, use `npm install` instead. Consider committing a lockfile for reproducibility. |
| GitHub Actions runner has insufficient disk for Rust build + TLSNotary | `ubuntu-latest` runners have 14GB free disk. Rust build with TLSNotary deps uses ~4-6GB. Sufficient. |
