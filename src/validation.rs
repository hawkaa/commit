use axum::http::StatusCode;
use k256::ecdsa::signature::Verifier;

use crate::models::ProofType;

/// Validate that the proof transcript's HTTP request matches the claimed subject.
///
/// Parses the HTTP request line from `transcript_sent`, extracts the URL path,
/// and verifies it matches `subject_id`. For email proofs, `transcript_recv` is
/// required and must contain a `github.com/{owner}/{repo}` URL in the response body.
pub fn validate_transcript_subject(
    transcript_sent: &str,
    transcript_recv: Option<&str>,
    proof_type: &ProofType,
    subject_id: &str,
) -> Result<(), StatusCode> {
    // Reject transcripts with multiple HTTP request lines (pipelining defense)
    validate_single_request(transcript_sent)?;

    match proof_type {
        ProofType::GitHistory => validate_git_history_transcript(transcript_sent, subject_id),
        ProofType::CiLogs => validate_ci_logs_transcript(transcript_sent, subject_id),
        ProofType::Email => {
            let recv = transcript_recv.ok_or_else(|| {
                tracing::warn!("Email proof type requires transcript_recv");
                StatusCode::BAD_REQUEST
            })?;
            validate_email_transcript(recv, subject_id)
        }
        _ => {
            tracing::warn!(
                "Transcript binding not yet supported for proof type: {}",
                proof_type.as_str()
            );
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

/// Reject transcripts containing multiple HTTP request lines (pipelining defense).
///
/// Scans all lines for HTTP method patterns. If more than one request line is
/// found, the transcript is rejected to prevent an attacker from smuggling a
/// second request that targets a different resource.
fn validate_single_request(transcript_sent: &str) -> Result<(), StatusCode> {
    let request_count = transcript_sent
        .lines()
        .filter(|line| {
            matches!(
                line.split_whitespace().next(),
                Some("GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS")
            ) && line
                .split_whitespace()
                .nth(1)
                .is_some_and(|path| path.starts_with('/'))
        })
        .count();

    if request_count > 1 {
        tracing::warn!(
            "Transcript contains {request_count} HTTP request lines — rejecting (pipelining)"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

/// Validate that a git_history transcript contains a request to `/repos/{owner}/{repo}`.
fn validate_git_history_transcript(
    transcript_sent: &str,
    subject_id: &str,
) -> Result<(), StatusCode> {
    // Parse subject_id into owner/repo components
    let subject_parts: Vec<&str> = subject_id.splitn(2, '/').collect();
    if subject_parts.len() != 2 {
        tracing::warn!("Invalid subject_id format: {subject_id}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (expected_owner, expected_repo) = (subject_parts[0], subject_parts[1]);

    // Extract the HTTP request line (first line of the transcript)
    let request_line = transcript_sent
        .lines()
        .next()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| {
            tracing::warn!("Empty or missing transcript_sent");
            StatusCode::BAD_REQUEST
        })?;

    // Parse: "GET /path HTTP/1.1"
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        tracing::warn!("Invalid HTTP request line: {request_line}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = parts[1];

    // Strip query string before checking path prefix — prevents /repos/ in query from matching
    let path_no_query = path.split('?').next().unwrap_or(path);

    // Require path to start with /repos/ (not just contain it anywhere)
    let repos_prefix = "/repos/";
    if !path_no_query.starts_with(repos_prefix) {
        tracing::warn!("Transcript path missing /repos/ prefix: {path}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let after_repos = &path_no_query[repos_prefix.len()..];

    // Split on / to get owner and repo
    let path_parts: Vec<&str> = after_repos.splitn(3, '/').collect();
    if path_parts.len() < 2 || path_parts[0].is_empty() || path_parts[1].is_empty() {
        tracing::warn!("Incomplete /repos/owner/repo path in transcript: {path}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (transcript_owner, transcript_repo) = (path_parts[0], path_parts[1]);

    // Validate components are clean ASCII (no percent-encoding, null bytes, non-printable)
    if !is_valid_path_component(transcript_owner) || !is_valid_path_component(transcript_repo) {
        tracing::warn!(
            "Invalid characters in transcript path components: {transcript_owner}/{transcript_repo}"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Case-insensitive comparison
    if !transcript_owner.eq_ignore_ascii_case(expected_owner)
        || !transcript_repo.eq_ignore_ascii_case(expected_repo)
    {
        tracing::warn!(
            "Transcript subject mismatch: transcript has {transcript_owner}/{transcript_repo}, \
             claimed {expected_owner}/{expected_repo}"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

/// Validate that a ci_logs transcript contains a request to `/repos/{owner}/{repo}/actions/...`.
///
/// Reuses the git_history parsing pattern but additionally requires the path
/// segment after `{repo}` to be `actions`, preventing git_history transcripts
/// from being accepted as ci_logs.
fn validate_ci_logs_transcript(transcript_sent: &str, subject_id: &str) -> Result<(), StatusCode> {
    // Parse subject_id into owner/repo components
    let subject_parts: Vec<&str> = subject_id.splitn(2, '/').collect();
    if subject_parts.len() != 2 {
        tracing::warn!("Invalid subject_id format: {subject_id}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (expected_owner, expected_repo) = (subject_parts[0], subject_parts[1]);

    // Extract the HTTP request line (first line of the transcript)
    let request_line = transcript_sent
        .lines()
        .next()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| {
            tracing::warn!("Empty or missing transcript_sent");
            StatusCode::BAD_REQUEST
        })?;

    // Parse: "GET /path HTTP/1.1"
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        tracing::warn!("Invalid HTTP request line: {request_line}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = parts[1];

    // Strip query string before checking path prefix
    let path_no_query = path.split('?').next().unwrap_or(path);

    // Require path to start with /repos/
    let repos_prefix = "/repos/";
    if !path_no_query.starts_with(repos_prefix) {
        tracing::warn!("Transcript path missing /repos/ prefix: {path}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let after_repos = &path_no_query[repos_prefix.len()..];

    // Split on / to get owner, repo, and next segment
    let path_parts: Vec<&str> = after_repos.splitn(4, '/').collect();
    if path_parts.len() < 3 || path_parts[0].is_empty() || path_parts[1].is_empty() {
        tracing::warn!("Incomplete /repos/owner/repo/actions path in transcript: {path}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (transcript_owner, transcript_repo, next_segment) =
        (path_parts[0], path_parts[1], path_parts[2]);

    // Require the segment after repo to be "actions"
    if next_segment != "actions" {
        tracing::warn!(
            "ci_logs transcript path missing /actions/ segment: got '{next_segment}' in {path}"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate components are clean ASCII
    if !is_valid_path_component(transcript_owner) || !is_valid_path_component(transcript_repo) {
        tracing::warn!(
            "Invalid characters in transcript path components: {transcript_owner}/{transcript_repo}"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Case-insensitive comparison
    if !transcript_owner.eq_ignore_ascii_case(expected_owner)
        || !transcript_repo.eq_ignore_ascii_case(expected_repo)
    {
        tracing::warn!(
            "Transcript subject mismatch: transcript has {transcript_owner}/{transcript_repo}, \
             claimed {expected_owner}/{expected_repo}"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

/// Validate that an email proof's received transcript contains a `github.com/{owner}/{repo}` URL
/// matching the claimed subject.
fn validate_email_transcript(transcript_recv: &str, subject_id: &str) -> Result<(), StatusCode> {
    // Parse subject_id into owner/repo components
    let subject_parts: Vec<&str> = subject_id.splitn(2, '/').collect();
    if subject_parts.len() != 2 {
        tracing::warn!("Invalid subject_id format: {subject_id}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (expected_owner, expected_repo) = (subject_parts[0], subject_parts[1]);

    // Search for github.com/{owner}/{repo} pattern in the recv body
    // Supports both https://github.com/... and bare github.com/...
    let marker = "github.com/";
    for (idx, _) in transcript_recv.match_indices(marker) {
        let after_marker = &transcript_recv[idx + marker.len()..];
        let path_parts: Vec<&str> = after_marker.splitn(3, '/').collect();
        if path_parts.len() >= 2 && !path_parts[0].is_empty() && !path_parts[1].is_empty() {
            // Trim the repo name at any non-path character (whitespace, quote, etc.)
            let owner = path_parts[0]
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.')
                .next()
                .unwrap_or("");
            let repo = path_parts[1]
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.')
                .next()
                .unwrap_or("");

            if !is_valid_path_component(owner) || !is_valid_path_component(repo) {
                continue;
            }

            if owner.eq_ignore_ascii_case(expected_owner)
                && repo.eq_ignore_ascii_case(expected_repo)
            {
                return Ok(());
            }
        }
    }

    tracing::warn!(
        "Email transcript recv does not contain github.com/{expected_owner}/{expected_repo}"
    );
    Err(StatusCode::BAD_REQUEST)
}

/// Validate a path component contains only safe ASCII characters.
/// GitHub owner/repo names: alphanumeric, hyphens, dots, underscores.
fn is_valid_path_component(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Verify that a TLSNotary attestation was signed by the trusted notary.
///
/// Deserializes the attestation from BCS bytes, extracts the header, and
/// verifies the ECDSA-secp256k1 signature against `trusted_key`.
pub fn verify_attestation_signature(
    attestation_bytes: &[u8],
    trusted_key: &k256::ecdsa::VerifyingKey,
) -> Result<(), StatusCode> {
    let attestation: tlsn_core::attestation::Attestation = bcs::from_bytes(attestation_bytes)
        .map_err(|e| {
            tracing::warn!("Attestation BCS deserialization failed: {e}");
            StatusCode::BAD_REQUEST
        })?;

    if attestation.signature.alg != tlsn_core::signing::SignatureAlgId::SECP256K1 {
        tracing::warn!(
            "Unexpected attestation signature algorithm: {}",
            attestation.signature.alg
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    let header_bytes = bcs::to_bytes(&attestation.header).map_err(|e| {
        tracing::warn!("Failed to BCS-serialize attestation header: {e}");
        StatusCode::BAD_REQUEST
    })?;

    let signature =
        k256::ecdsa::Signature::from_slice(&attestation.signature.data).map_err(|e| {
            tracing::warn!("Malformed attestation signature: {e}");
            StatusCode::BAD_REQUEST
        })?;

    trusted_key.verify(&header_bytes, &signature).map_err(|_| {
        tracing::warn!("Attestation signature verification failed — not signed by trusted notary");
        StatusCode::UNAUTHORIZED
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use k256::ecdsa::SigningKey;
    use k256::elliptic_curve::rand_core::OsRng;

    fn random_verifying_key() -> k256::ecdsa::VerifyingKey {
        let signing = SigningKey::random(&mut OsRng);
        *signing.verifying_key()
    }

    #[test]
    fn attestation_garbage_bytes_returns_400() {
        let key = random_verifying_key();
        let result = super::verify_attestation_signature(b"not-valid-bcs", &key);
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn attestation_empty_bytes_returns_400() {
        let key = random_verifying_key();
        let result = super::verify_attestation_signature(&[], &key);
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn attestation_truncated_bytes_returns_400() {
        let key = random_verifying_key();
        // A few random bytes — too short to be a valid Attestation
        let result = super::verify_attestation_signature(&[0x01, 0x02, 0x03, 0x04], &key);
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn valid_git_history_transcript() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .is_ok()
        );
    }

    #[test]
    fn case_insensitive_match() {
        let transcript = "GET /repos/Owner/Repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .is_ok()
        );
    }

    #[test]
    fn query_parameters_ignored() {
        let transcript = "GET /repos/owner/repo?per_page=1 HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .is_ok()
        );
    }

    #[test]
    fn extra_path_segments_ok() {
        let transcript = "GET /repos/owner/repo/commits HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .is_ok()
        );
    }

    #[test]
    fn subject_mismatch_rejected() {
        let transcript = "GET /repos/owner/repoA HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repoB")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn empty_transcript_rejected() {
        assert_eq!(
            validate_transcript_subject("", None, &ProofType::GitHistory, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn repos_in_query_string_rejected() {
        // /repos/ must be at the start of the path, not inside a query parameter
        let transcript = "GET /evil?x=/repos/victim/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "victim/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn no_request_line_rejected() {
        assert_eq!(
            validate_transcript_subject(
                "not-a-request",
                None,
                &ProofType::GitHistory,
                "owner/repo"
            )
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn incomplete_path_rejected() {
        let transcript = "GET /repos/owner HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn percent_encoded_rejected() {
        let transcript = "GET /repos/victim%2Frepo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "victim/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn null_bytes_rejected() {
        let transcript = "GET /repos/owner\0/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn email_proof_type_no_recv_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::Email, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn email_valid_recv_with_https_url() {
        let recv = "HTTP/1.1 200 OK\r\n\r\nCheck out https://github.com/owner/repo for details";
        assert!(
            validate_transcript_subject(
                "GET / HTTP/1.1\r\n",
                Some(recv),
                &ProofType::Email,
                "owner/repo"
            )
            .is_ok()
        );
    }

    #[test]
    fn email_valid_recv_bare_url() {
        let recv = "HTTP/1.1 200 OK\r\n\r\nSee github.com/owner/repo for info";
        assert!(
            validate_transcript_subject(
                "GET / HTTP/1.1\r\n",
                Some(recv),
                &ProofType::Email,
                "owner/repo"
            )
            .is_ok()
        );
    }

    #[test]
    fn email_case_insensitive_recv() {
        let recv = "HTTP/1.1 200 OK\r\n\r\nhttps://github.com/Owner/Repo/issues";
        assert!(
            validate_transcript_subject(
                "GET / HTTP/1.1\r\n",
                Some(recv),
                &ProofType::Email,
                "owner/repo"
            )
            .is_ok()
        );
    }

    #[test]
    fn email_recv_no_matching_url_rejected() {
        let recv = "HTTP/1.1 200 OK\r\n\r\nhttps://github.com/other/repo";
        assert_eq!(
            validate_transcript_subject(
                "GET / HTTP/1.1\r\n",
                Some(recv),
                &ProofType::Email,
                "owner/repo"
            )
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn email_recv_empty_body_rejected() {
        assert_eq!(
            validate_transcript_subject(
                "GET / HTTP/1.1\r\n",
                Some(""),
                &ProofType::Email,
                "owner/repo"
            )
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn ci_logs_valid_actions_path() {
        let transcript = "GET /repos/owner/repo/actions/runs HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repo").is_ok()
        );
    }

    #[test]
    fn ci_logs_case_insensitive() {
        let transcript = "GET /repos/Owner/Repo/actions/runs HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repo").is_ok()
        );
    }

    #[test]
    fn ci_logs_missing_actions_segment_rejected() {
        // A git_history-style path without /actions/ must be rejected for ci_logs
        let transcript = "GET /repos/owner/repo/commits HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn ci_logs_no_segment_after_repo_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn ci_logs_subject_mismatch_rejected() {
        let transcript = "GET /repos/owner/repoA/actions/runs HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repoB")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn ci_logs_query_params_ok() {
        let transcript =
            "GET /repos/owner/repo/actions/runs?per_page=5 HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(
            validate_transcript_subject(transcript, None, &ProofType::CiLogs, "owner/repo").is_ok()
        );
    }

    // --- HTTP pipelining defense tests ---

    #[test]
    fn single_request_passes() {
        assert!(
            validate_single_request("GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n")
                .is_ok()
        );
    }

    #[test]
    fn multiple_requests_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n\r\nGET /repos/evil/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_single_request(transcript).unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn pipelined_post_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n\r\nPOST /repos/owner/evil HTTP/1.1\r\n";
        assert_eq!(
            validate_single_request(transcript).unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn non_request_lines_with_method_words_ok() {
        // Lines that contain HTTP method words but don't match the request line pattern
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nX-Custom: GET something\r\n";
        assert!(validate_single_request(transcript).is_ok());
    }

    #[test]
    fn pipelining_blocks_full_transcript_validation() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n\r\nGET /repos/evil/other HTTP/1.1\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, None, &ProofType::GitHistory, "owner/repo")
                .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn empty_transcript_passes_pipelining_check() {
        assert!(validate_single_request("").is_ok());
    }
}
