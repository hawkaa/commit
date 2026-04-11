use axum::http::StatusCode;

use crate::models::ProofType;

/// Validate that the proof transcript's HTTP request matches the claimed subject.
///
/// Parses the HTTP request line from `transcript_sent`, extracts the URL path,
/// and verifies it matches `subject_id`. Only `git_history` is currently supported;
/// other proof types are rejected until their transcript binding is designed.
pub fn validate_transcript_subject(
    transcript_sent: &str,
    proof_type: &ProofType,
    subject_id: &str,
) -> Result<(), StatusCode> {
    match proof_type {
        ProofType::GitHistory => validate_git_history_transcript(transcript_sent, subject_id),
        _ => {
            tracing::warn!(
                "Transcript binding not yet supported for proof type: {}",
                proof_type.as_str()
            );
            Err(StatusCode::BAD_REQUEST)
        }
    }
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

    // Find /repos/ prefix and extract owner/repo
    let repos_prefix = "/repos/";
    let after_repos = path.find(repos_prefix).map(|i| &path[i + repos_prefix.len()..]);
    let after_repos = after_repos.ok_or_else(|| {
        tracing::warn!("Transcript path missing /repos/ prefix: {path}");
        StatusCode::BAD_REQUEST
    })?;

    // Strip query string if present
    let after_repos = after_repos.split('?').next().unwrap_or(after_repos);

    // Split on / to get owner and repo
    let path_parts: Vec<&str> = after_repos.splitn(3, '/').collect();
    if path_parts.len() < 2 || path_parts[0].is_empty() || path_parts[1].is_empty() {
        tracing::warn!("Incomplete /repos/owner/repo path in transcript: {path}");
        return Err(StatusCode::BAD_REQUEST);
    }
    let (transcript_owner, transcript_repo) = (path_parts[0], path_parts[1]);

    // Validate components are clean ASCII (no percent-encoding, null bytes, non-printable)
    if !is_valid_path_component(transcript_owner) || !is_valid_path_component(transcript_repo) {
        tracing::warn!("Invalid characters in transcript path components: {transcript_owner}/{transcript_repo}");
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

/// Validate a path component contains only safe ASCII characters.
/// GitHub owner/repo names: alphanumeric, hyphens, dots, underscores.
fn is_valid_path_component(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_git_history_transcript() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").is_ok());
    }

    #[test]
    fn case_insensitive_match() {
        let transcript = "GET /repos/Owner/Repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").is_ok());
    }

    #[test]
    fn query_parameters_ignored() {
        let transcript = "GET /repos/owner/repo?per_page=1 HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").is_ok());
    }

    #[test]
    fn extra_path_segments_ok() {
        let transcript = "GET /repos/owner/repo/commits HTTP/1.1\r\nHost: api.github.com\r\n";
        assert!(validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").is_ok());
    }

    #[test]
    fn subject_mismatch_rejected() {
        let transcript = "GET /repos/owner/repoA HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repoB").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn empty_transcript_rejected() {
        assert_eq!(
            validate_transcript_subject("", &ProofType::GitHistory, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn no_request_line_rejected() {
        assert_eq!(
            validate_transcript_subject("not-a-request", &ProofType::GitHistory, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn incomplete_path_rejected() {
        let transcript = "GET /repos/owner HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn percent_encoded_rejected() {
        let transcript = "GET /repos/victim%2Frepo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::GitHistory, "victim/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn null_bytes_rejected() {
        let transcript = "GET /repos/owner\0/repo HTTP/1.1\r\nHost: api.github.com\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::GitHistory, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn email_proof_type_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::Email, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn ci_logs_proof_type_rejected() {
        let transcript = "GET /repos/owner/repo HTTP/1.1\r\n";
        assert_eq!(
            validate_transcript_subject(transcript, &ProofType::CiLogs, "owner/repo").unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }
}
