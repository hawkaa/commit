---
title: "fix: Security hardening batch — transcript binding, replay prevention, score integrity"
type: fix
status: active
date: 2026-04-12
---

# fix: Security hardening batch — transcript binding, replay prevention, score integrity

## Overview

The P0 fix (`docs/plans/2026-04-11-001-fix-proof-binding-security-plan.md`) bound proof_hash to cryptographic attestation and added transcript-subject binding for `git_history` proofs. It explicitly deferred five follow-up items as "Loose Threads." This plan addresses all five: enabling `ci_logs` and `email` transcript binding, defending against HTTP pipelining in transcripts, adding rate limiting to prevent endorsement spam via cheap session generation, and weighting `pending_attestation` endorsements lower in Commit Score until Ed25519 request signing exists.

## Problem Frame

After the P0 fix, the remaining attack surfaces are:

1. **Unbound proof types** — `ci_logs` and `email` proof types return 400. They cannot be used for endorsements until transcript binding is designed. `ci_logs` is structurally similar to `git_history` (same GitHub API domain, predictable URL pattern). `email` requires a fundamentally different binding strategy since the transcript comes from a mail server, not a GitHub API endpoint.

2. **HTTP pipelining bypass** — The transcript parser extracts only the first HTTP request line. In HTTP/1.1 keep-alive, a single TLS session can contain multiple requests. An attacker could craft a session that starts with `GET /repos/victim/legit-repo` and then pipelines a second request to a different endpoint. The 200-byte revealed window and single-request extension behavior mitigate this, but the parser should enforce exactly one request line.

3. **Cheap session replay** — `UNIQUE(proof_hash)` prevents exact attestation replay, but generating a new TLSNotary session for the same repo takes ~5 seconds. An attacker can spam endorsements by repeatedly proving the same repo. There is no per-device or per-key rate limiting.

4. **Unauthenticated endorsement weight** — `POST /endorsements` has no caller authentication. Any client with a valid attestation and transcript can submit. Until Ed25519 keypair signing is added, `pending_attestation` endorsements have the same implicit weight as `verified` ones in any future Layer 2 scoring, which overstates their trustworthiness.

## Requirements Trace

- R1. `ci_logs` proof type must validate transcript URL against `/repos/{owner}/{repo}/actions/runs/{id}/logs` pattern
- R2. `email` proof type must extract a GitHub repo reference from the email transcript and validate it against the claimed subject
- R3. The transcript parser must reject transcripts containing more than one HTTP request line
- R4. Endorsement creation must be rate-limited per subject (or per subject + proof_type) to prevent spam from cheap session generation
- R5. `pending_attestation` endorsements must contribute less to Commit Score than `verified` ones
- R6. All changes must pass existing tests with no regressions
- R7. New validation logic must have unit tests covering happy path, mismatch, and edge cases

## Scope Boundaries

- Ed25519 request signing from extension keypair — separate Phase 2 work, not in this plan
- Network keyring, key sharing, L2 attestation — Phase 3
- Notary server deployment — separate plan (`docs/plans/2026-04-12-001-feat-own-notary-server-plan.md`)
- Attestation signature verification — separate plan (`docs/plans/2026-04-12-002-feat-attestation-signature-verification-plan.md`), already implemented
- Changes to the Chrome extension — this plan is backend-only

## Context & Research

### Relevant Code and Patterns

- `src/validation.rs` — `validate_transcript_subject()` dispatches by `ProofType`; `validate_git_history_transcript()` is the reference implementation for structural URL parsing; `is_valid_path_component()` validates ASCII path segments
- `src/routes/webhook.rs` — `receive_endorsement_webhook()` validates `server_name` per proof type, calls `validate_transcript_subject()`, creates endorsement with `verified` status
- `src/routes/endorsement.rs` — `submit_endorsement()` calls `validate_transcript_subject()`, creates endorsement with `pending_attestation` status
- `src/services/db.rs` — `create_endorsement()` with `UNIQUE(proof_hash)` index, `get_endorsement_count()` counts non-failed endorsements, `map_db_error()` maps unique constraint violations to 409
- `src/services/score.rs` — `score_github_repo()` currently Layer 1 only; `ScoreBreakdown` has Layer 2 fields (`endorsements`, `proof_strength`) all set to 0.0
- `src/models/signal.rs` — `compute_score()` blends L1*0.3 + L2*0.7 when `has_layer2` is true; `ScoreBreakdown` has `proof_strength` field
- `src/models/endorsement.rs` — `ProofType` enum with `GitHistory`, `Email`, `CiLogs` variants
- `tests/api.rs` — Integration tests for webhook and endorsement endpoints including transcript binding and replay prevention
- `tests/score.rs` — Unit tests for `compute_score` with L1 and L2 weighting

### GitHub Actions API URL Patterns

The CI logs endpoint follows: `GET /repos/{owner}/{repo}/actions/runs/{run_id}/logs HTTP/1.1`. The `run_id` is a numeric identifier. The path always starts with `/repos/{owner}/{repo}/actions/` which gives us the same structural extraction as `git_history` plus validation that the path continues into the Actions namespace.

### Email Transcript Binding Challenge

Email proofs use TLSNotary against `mail.google.com` or `*.outlook.com`. The transcript is an HTTPS request to a mail API, not a GitHub API. The repo identity is not in the URL — it would be in the email body content. This means binding requires:
- Parsing the response body (recv transcript) to find a GitHub repo URL or reference
- Or: requiring the email subject/body to contain a specific format (e.g., a GitHub notification email whose URL contains the repo)

GitHub notification emails contain URLs like `https://github.com/{owner}/{repo}/...` in the body. This is the most viable binding strategy for Phase 1.

## Key Technical Decisions

- **ci_logs reuses git_history's structural parsing**: The `/repos/{owner}/{repo}` prefix extraction is identical. The only difference is validating that the path continues with `/actions/` to confirm it is actually a CI logs request, not a git history request misclassified as `ci_logs`. This prevents cross-proof-type substitution.

- **Email binding uses recv transcript, not sent**: For email proofs, the sent transcript is an HTTP request to a mail API (e.g., `GET /mail/u/0/...`). The repo identity lives in the response body. The validator must inspect `transcript_recv` for a `github.com/{owner}/{repo}` URL pattern. This requires `validate_transcript_subject` to accept an optional `transcript_recv` parameter for proof types that need response-body binding.

- **Email binding is defense-in-depth, not proof-of-endorsement**: An email containing a GitHub repo URL proves the user received an email mentioning that repo. It does not prove they endorse it. The endorsement category and intent come from the user's explicit action in the extension. The transcript binding prevents claiming an email about repo A as proof for repo B.

- **Pipelining defense validates line count, not connection state**: We cannot inspect the TLS session structure from the transcript alone. Instead, we count HTTP request lines (lines matching `^(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS) /`) in the revealed sent transcript. If more than one is found, reject. This is simple, testable, and covers the attack vector.

- **Rate limiting uses a sliding window per subject**: A new `endorsement_rate_check` query counts endorsements for a given `subject_id` created within the last N minutes. If the count exceeds a threshold, the endpoint returns 429. This is simpler than nonce tracking and catches the spam scenario (many cheap sessions for the same repo). The window and threshold are configurable constants (e.g., 5 endorsements per 60 minutes per subject).

- **Score weighting uses status-aware counting**: Instead of changing the score algorithm's structure, the `endorsements` Layer 2 field is computed by weighting each endorsement by status: `verified` = 1.0, `pending_attestation` = 0.3. This means 10 pending endorsements contribute the same score as 3 verified ones. The `proof_strength` field uses the same weighting. This is simple, backward-compatible, and naturally converges to full weight once Ed25519 signing or attestation verification upgrades endorsements to `verified`.

## Implementation Units

- [ ] **Unit 1: ci_logs transcript binding**

**Goal:** Enable `ci_logs` proof type to pass transcript validation by parsing the GitHub Actions API URL pattern, reusing the structural parsing from `git_history`.

**Requirements:** R1, R6, R7

**Dependencies:** None

**Files:**
- Modify: `src/validation.rs`
- Test: `src/validation.rs` (unit tests)
- Test: `tests/api.rs` (integration test for webhook + endorsement with `ci_logs`)

**Approach:**
- Add `validate_ci_logs_transcript()` function in `src/validation.rs`. It follows the same pattern as `validate_git_history_transcript()`: parse the HTTP request line, extract the path, require it starts with `/repos/`, extract `{owner}/{repo}` from the first two path segments after `/repos/`, validate path components with `is_valid_path_component()`, case-insensitive comparison against `subject_id`.
- Additional validation: after extracting owner/repo, verify the path continues with `/actions/` (i.e., `path_parts[2]` starts with `actions`). This prevents a `git_history` transcript from being submitted as `ci_logs` proof type. Without this check, proof types are interchangeable which weakens the proof semantics.
- Update the `match proof_type` in `validate_transcript_subject()` to dispatch `ProofType::CiLogs` to `validate_ci_logs_transcript()` instead of the catch-all 400.
- Update the `webhook_email_proof_type_blocked` test name/comment to clarify only email is still blocked.

**Patterns to follow:**
- `validate_git_history_transcript()` structure: parse request line, extract path, validate components, compare
- `is_valid_path_component()` for character validation
- `tracing::warn!` for security-relevant rejections

**Test scenarios:**
- Happy path: `GET /repos/owner/repo/actions/runs/12345/logs HTTP/1.1` with subject `owner/repo` passes
- Happy path: case-insensitive match on owner/repo components
- Happy path: query parameters in URL are stripped before validation
- Error: path is `/repos/owner/repo/commits` (git_history URL, not CI logs) with `ci_logs` proof type rejects (missing `/actions/` segment)
- Error: path is `/repos/owner` (incomplete) rejects
- Error: subject mismatch (`owner/repoA` in transcript, `owner/repoB` claimed) rejects
- Error: percent-encoded path components reject
- Error: empty transcript rejects
- Integration: webhook with `ci_logs` proof type + valid transcript + `server_name: api.github.com` creates endorsement
- Integration: `POST /endorsements` with `ci_logs` proof type + valid transcript passes validation

**Verification:**
- `cargo test` passes with no regressions
- `cargo clippy -- -D warnings` clean
- Existing `ci_logs_proof_type_rejected` test in `validation.rs` is replaced by happy-path test

---

- [ ] **Unit 2: Email transcript binding**

**Goal:** Enable `email` proof type by validating that the email transcript's response body contains a GitHub repo URL matching the claimed subject.

**Requirements:** R2, R6, R7

**Dependencies:** None (parallel with Unit 1)

**Files:**
- Modify: `src/validation.rs` — add `validate_email_transcript()`; update `validate_transcript_subject()` signature to accept optional recv transcript
- Modify: `src/routes/endorsement.rs` — add `transcript_recv` field to `SubmitEndorsementRequest` (optional, required only for email proofs)
- Modify: `src/routes/webhook.rs` — pass `transcript.recv` to validation
- Test: `src/validation.rs` (unit tests)
- Test: `tests/api.rs` (integration tests)

**Approach:**
- Extend `validate_transcript_subject()` signature to: `validate_transcript_subject(transcript_sent: &str, transcript_recv: Option<&str>, proof_type: &ProofType, subject_id: &str)`. For `git_history` and `ci_logs`, `transcript_recv` is ignored. For `email`, it is required (return 400 if `None`).
- `validate_email_transcript()` receives `transcript_recv` and `subject_id`. It searches the response body for a URL pattern: `github.com/{owner}/{repo}` (with or without `https://` prefix, with optional trailing path segments). Extract `{owner}/{repo}` from the first match, validate components with `is_valid_path_component()`, case-insensitive comparison against `subject_id`.
- The sent transcript is also validated: confirm the request targets a known mail provider domain. The `server_name` validation in `webhook.rs` already checks `.google.com` or `.outlook.com`, but the sent transcript itself should have a `Host:` header matching. For the initial implementation, trust the `server_name` check and focus recv binding on the repo URL.
- Reject if no `github.com/{owner}/{repo}` URL is found in recv transcript. Reject if the extracted owner/repo doesn't match the claimed subject_id.
- In `endorsement.rs`: add `transcript_recv: Option<String>` to `SubmitEndorsementRequest`. Pass it through to validation. For email proof type, return 400 if `transcript_recv` is `None`.
- In `webhook.rs`: `RedactedTranscript.recv` is already `Option<String>`. Pass `recv.as_deref()` to validation.

**Patterns to follow:**
- `validate_git_history_transcript()` structure for the validation function
- `is_valid_path_component()` for extracted components
- Existing `Option<String>` field pattern in `RedactedTranscript`

**Test scenarios:**
- Happy path: recv contains `https://github.com/owner/repo/pull/42` with subject `owner/repo` passes
- Happy path: recv contains `github.com/Owner/Repo` (no scheme, mixed case) with subject `owner/repo` passes
- Happy path: recv contains multiple GitHub URLs but first match is correct passes
- Error: recv contains `github.com/owner/repoA` but subject is `owner/repoB` rejects
- Error: recv contains no `github.com/` URL rejects
- Error: recv is None/empty for email proof type rejects
- Error: extracted path components contain percent-encoding rejects
- Edge case: recv contains `github.com/owner/repo` embedded in a longer URL (e.g., `https://github.com/owner/repo/issues/1#comment`) still extracts correctly
- Integration: webhook with email proof type + valid recv transcript creates endorsement
- Integration: `POST /endorsements` with email proof type + valid transcript_recv passes
- Regression: `git_history` and `ci_logs` calls with `transcript_recv: None` still work (recv is ignored)

**Verification:**
- `cargo test` passes; existing email-blocked tests updated to test the new binding
- `cargo clippy -- -D warnings` clean
- `webhook_email_proof_type_blocked` test in `tests/api.rs` becomes a happy-path test

---

- [ ] **Unit 3: HTTP pipelining defense**

**Goal:** Reject transcripts that contain more than one HTTP request line, preventing an attacker from piggybacking a benign request with a targeted one in a single TLS session.

**Requirements:** R3, R6, R7

**Dependencies:** None (parallel with Units 1-2)

**Files:**
- Modify: `src/validation.rs` — add `validate_single_request()` helper, call it from `validate_transcript_subject()` before dispatching to proof-type-specific validation

**Approach:**
- Add `validate_single_request(transcript_sent: &str) -> Result<(), StatusCode>`. This function scans all lines (not just the first) for HTTP request line patterns: lines matching `^(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\s+/`. Count matches. If count > 1, log a warning and return 400.
- Call `validate_single_request()` at the top of `validate_transcript_subject()`, before the `match proof_type` dispatch. This way all proof types benefit from the defense.
- The check is conservative: it only looks for standard HTTP methods followed by a space and `/`. This won't false-positive on response headers or body content that happen to start with "GET " because the sent transcript only contains data sent by the client (HTTP requests), not received data (responses).
- Edge case: if the revealed transcript is very short (< 16 bytes), it may not contain a full request line. The existing proof-type validators already reject incomplete transcripts, so `validate_single_request` does not need to handle this — it only fires if there's more than one match.

**Patterns to follow:**
- Line-by-line scanning with `.lines()` iterator
- `tracing::warn!` for security-relevant rejections

**Test scenarios:**
- Happy path: single `GET /repos/owner/repo HTTP/1.1\r\n...` passes (1 request line)
- Error: two request lines `GET /repos/owner/repo HTTP/1.1\r\nHost: ...\r\n\r\nGET /repos/attacker/evil HTTP/1.1\r\n` rejects
- Error: POST followed by GET in same transcript rejects
- Edge case: body content that coincidentally contains "GET /" on a new line in sent transcript — this is unlikely in practice (sent transcript is HTTP request headers, not response bodies), but if it occurs, rejecting is the safe default
- Edge case: single request line with keep-alive header but no second request passes (keep-alive header alone is not a second request)

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- Existing transcript tests still pass (all have single request lines)

---

- [ ] **Unit 4: Rate limiting for endorsement spam prevention**

**Goal:** Prevent endorsement spam by rate-limiting endorsement creation per subject within a sliding time window.

**Requirements:** R4, R6, R7

**Dependencies:** None (parallel with Units 1-3)

**Files:**
- Modify: `src/services/db.rs` — add `count_recent_endorsements()` query
- Modify: `src/routes/endorsement.rs` — add rate check before `create_endorsement()`
- Modify: `src/routes/webhook.rs` — add rate check before `create_endorsement()`
- Test: `tests/api.rs`

**Approach:**
- Add `count_recent_endorsements(subject_id: &Uuid, window_minutes: i64) -> Result<u32>` to `Database`. Query: `SELECT COUNT(*) FROM endorsements WHERE subject_id = ? AND created_at > datetime('now', '-' || ? || ' minutes')`. This counts all endorsements (any status) within the window.
- Define constants in a new section at the top of `src/routes/endorsement.rs` (or a shared config module): `RATE_LIMIT_WINDOW_MINUTES: i64 = 60` and `RATE_LIMIT_MAX_ENDORSEMENTS: u32 = 5`. These values mean: max 5 endorsements per subject per 60-minute window.
- In `submit_endorsement()`: after finding the subject but before `create_endorsement()`, call `count_recent_endorsements()`. If count >= threshold, return `StatusCode::TOO_MANY_REQUESTS` (429).
- In `receive_endorsement_webhook()`: same check. The webhook path is authenticated, but the notary server could be tricked into replaying sessions rapidly, so the rate limit applies to both paths.
- The rate limit is per-subject, not per-caller. This is intentional: the threat is many endorsements for the same repo inflating its score, regardless of which device submits them. Per-caller rate limiting requires Ed25519 identity, which is deferred.
- The constants should be generous enough to not block legitimate usage during Phase 1 (where endorsement volume is low) but tight enough to make spam expensive in wall-clock time: 5 endorsements per subject per hour means an attacker needs 12 hours to generate 60 endorsements for one repo.

**Patterns to follow:**
- `get_endorsement_count()` query pattern in `src/services/db.rs`
- `map_db_error()` for rusqlite error mapping
- `StatusCode::TOO_MANY_REQUESTS` (429) for rate limit responses

**Test scenarios:**
- Happy path: first endorsement for a subject passes rate check
- Happy path: 5 endorsements within the window (at the limit) all succeed
- Error: 6th endorsement within the window returns 429
- Edge case: endorsements outside the window don't count (test with subjects that have old endorsements)
- Integration: webhook path also enforces the rate limit
- Regression: existing happy-path tests still pass (they create at most 1-2 endorsements per subject)

**Verification:**
- `cargo test` passes
- `cargo clippy -- -D warnings` clean
- Rate limit constants are documented with rationale in code comments

---

- [ ] **Unit 5: Score integrity — weight pending_attestation endorsements lower**

**Goal:** Reduce the score contribution of `pending_attestation` endorsements relative to `verified` ones, so unauthenticated submissions have less impact on Commit Score until Ed25519 signing is added.

**Requirements:** R5, R6, R7

**Dependencies:** None (parallel with Units 1-4, but logically the last to implement since it affects scoring)

**Files:**
- Modify: `src/services/db.rs` — add `get_endorsement_counts_by_status()` query
- Modify: `src/services/score.rs` — use status-weighted endorsement counts for Layer 2 fields
- Test: `tests/score.rs`

**Approach:**
- Add `get_endorsement_counts_by_status(subject_id: &Uuid) -> Result<(u32, u32)>` to `Database`. Returns `(verified_count, pending_count)`. Query: `SELECT status, COUNT(*) FROM endorsements WHERE subject_id = ? AND status != 'failed' GROUP BY status`.
- In `score_github_repo()` (or a new `score_with_endorsements()` function): accept endorsement counts. Compute the `endorsements` Layer 2 field as: `min((verified * 1.0 + pending * 0.3) * 5.0, 30.0)`. This follows the existing formula `min(count * 5, 30)` from the `ScoreBreakdown` doc but applies status weighting.
- Compute `proof_strength` as: if total > 0, `(verified * 1.0 + pending * 0.3) / total * 15.0`, else 0.0. This reflects the proportion of high-confidence proofs.
- The `has_layer2` flag should be set to `true` when there are any non-failed endorsements. Currently it is always `false` because Layer 2 fields are all 0.0. Once endorsement counts are wired in, the score will blend L1 and L2.
- Note: this is a preparatory change. The trust card currently doesn't fetch endorsement counts for scoring. The wiring from trust card route to endorsement-aware scoring is a follow-up (or can be included if straightforward). At minimum, the scoring functions and tests should exist so the logic is ready.
- The weight constants (`VERIFIED_WEIGHT = 1.0`, `PENDING_WEIGHT = 0.3`) should be defined as named constants with doc comments explaining the rationale.

**Patterns to follow:**
- `get_endorsement_count()` existing query pattern
- `ScoreBreakdown` field assignments in `score_github_repo()`
- `compute_score(&breakdown, has_layer2)` call pattern

**Test scenarios:**
- 10 verified endorsements: `endorsements` field = `min(10 * 5, 30)` = 30.0
- 10 pending endorsements: `endorsements` field = `min(3.0 * 5, 30)` = 15.0
- 5 verified + 5 pending: `endorsements` field = `min((5 + 1.5) * 5, 30)` = 30.0 (capped)
- 0 endorsements: `endorsements` field = 0.0, `has_layer2` = false, score is Layer 1 only
- `proof_strength` with all verified: 15.0
- `proof_strength` with all pending: `0.3/1.0 * 15` = 4.5
- `proof_strength` with mixed: proportional
- Regression: existing L1-only score tests unchanged (they don't pass endorsement data)

**Verification:**
- `cargo test` passes including new score weighting tests
- `cargo clippy -- -D warnings` clean
- Score values are deterministic and documented in test assertions

## System-Wide Impact

- **`validate_transcript_subject()` signature change** (Unit 2): Adding `transcript_recv: Option<&str>` parameter changes the function signature. All call sites (`endorsement.rs`, `webhook.rs`, and unit tests in `validation.rs`) must be updated. The `git_history` and `ci_logs` paths ignore the new parameter, so the change is backward-compatible in behavior.

- **API surface**: No breaking changes to external API. `transcript_recv` is added as an optional field to `SubmitEndorsementRequest` — existing clients that don't send it continue to work for `git_history` and `ci_logs`. Only `email` requires it. The 429 status code is new but clients should already handle non-2xx responses.

- **Score computation**: Unit 5 introduces Layer 2 score blending. When endorsements exist, the score formula changes from `L1/40*100` to `L1*0.3 + L2*0.7`. This will change displayed scores for repos that have endorsements. Since Phase 1 has very few endorsements, the impact is minimal. The score for repos with zero endorsements is unchanged.

- **Database queries**: Unit 4 adds a time-windowed count query. This runs on every endorsement submission. The `idx_endorsements_subject` index covers the `WHERE subject_id = ?` filter. The `created_at` filter is a string comparison on the datetime column, which SQLite handles efficiently. For Phase 1 volumes (< 1000 endorsements), this is not a performance concern.

- **Error propagation**: Pipelining defense (Unit 3) adds a new 400 rejection path at the top of `validate_transcript_subject()`. Existing valid transcripts (single request line) are unaffected. Rate limiting (Unit 4) adds a 429 path. Both are terminal — no cascading effects.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Email recv transcript may not always contain a `github.com` URL (e.g., plain-text emails, non-GitHub notification emails) | Document that email proofs must originate from GitHub notification emails. The extension UI should guide users to prove a GitHub notification email specifically. Non-matching emails are rejected with 400 and a descriptive log. |
| GitHub changes notification email format | The URL pattern `github.com/{owner}/{repo}` is deeply embedded in GitHub's platform. Changes are unlikely and would break many integrations beyond ours. Document the pattern as a known maintenance point. |
| Rate limit constants too aggressive for legitimate multi-endorser scenarios | 5 per subject per hour is generous for Phase 1 (individual users endorsing repos). If legitimate usage patterns emerge that exceed this, increase the constants. The values are named constants, easy to tune. |
| Pipelining check false-positives on legitimate transcripts containing HTTP method strings | The sent transcript contains only HTTP request data. Response bodies are in recv. The only scenario where the sent transcript contains "GET /" on a new line is HTTP pipelining, which is exactly what we want to reject. |
| Score weighting change surprises users with different scores | Phase 1 has minimal endorsement data. The score change only manifests when endorsements exist. Document the weighting in the score tooltip or trust card explanation. |
| `validate_transcript_subject` signature change breaks callers | Mechanical refactor — add `None` for recv parameter at all existing call sites for `git_history`/`ci_logs`. Compiler will catch any missed call sites. |
