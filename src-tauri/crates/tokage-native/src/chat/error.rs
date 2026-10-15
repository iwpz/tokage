//! Turn runtime error classification (docs/01 §9, P4.m3a).

use std::sync::OnceLock;
use regex::Regex;
use tokage_core::{event::TurnErrorKind, provider::ProviderId};

/// Classifies runtime error text and exit code into a standardized `TurnErrorKind`.
pub fn classify_error(
    provider: Option<ProviderId>,
    message: &str,
    status_code: Option<i32>,
) -> TurnErrorKind {
    let lower = message.to_ascii_lowercase();

    // 1. Check for Spawn / executable failures
    if lower.contains("failed to start")
        || lower.contains("failed to spawn")
        || lower.contains("no such file or directory")
        || lower.contains("executable not found")
        || lower.contains("command not found")
        || lower.contains("binary not found")
    {
        return TurnErrorKind::SpawnFailed;
    }

    // 2. Check for AuthRequired
    static AUTH_RE: OnceLock<Regex> = OnceLock::new();
    let auth_re = AUTH_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:not\s+logged\s+in|please\s+log\s*in|login\s+required|authentication\s+required|unauthorized|401\s+unauthorized|invalid\s+(?:api\s+)?key|api\s+key\s+is\s+invalid|missing\s+(?:api\s+)?key|no\s+api\s+key|token\s+(?:has\s+)?expired|(?:oauth\s+)?token\s+(?:has\s+been\s+)?revoked|failed\s+to\s+authenticate|auth(?:entication)?\s+error|auth(?:entication)?\s+failed|needs?\s+authentication|please\s+authenticate|run\s+`[^`]*login`)\b").unwrap()
    });
    if auth_re.is_match(&lower) {
        return TurnErrorKind::AuthRequired;
    }

    // 3. Check for RateLimited (using quota matcher logic or rate limit regex)
    if super::quota::match_quota(provider, message).is_some()
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests")
        || lower.contains("429")
        || lower.contains("quota exceeded")
        || lower.contains("usage limit")
        || lower.contains("insufficient balance")
        || lower.contains("insufficient credits")
        || lower.contains("402 payment required")
    {
        return TurnErrorKind::RateLimited;
    }

    // 4. Check for Timeout
    if lower.contains("timed out")
        || lower.contains("timeout exceeded")
        || lower.contains("idle timeout")
        || lower.contains("hard timeout")
        || lower.contains("exit timeout")
    {
        return TurnErrorKind::Timeout;
    }

    // 5. Check for ParseError
    if lower.contains("unrecognized format")
        || lower.contains("failed to parse")
        || lower.contains("parse error")
        || lower.contains("json parse error")
    {
        return TurnErrorKind::ParseError;
    }

    // 6. NonZeroExit
    if let Some(code) = status_code {
        if code != 0 {
            return TurnErrorKind::NonZeroExit;
        }
    }

    TurnErrorKind::NonZeroExit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_auth_required() {
        assert_eq!(
            classify_error(Some(ProviderId::ClaudeCode), "Not logged in. Please run `claude login`.", Some(1)),
            TurnErrorKind::AuthRequired
        );
        assert_eq!(
            classify_error(Some(ProviderId::CodexCli), "Error 401 Unauthorized: Invalid API key", Some(1)),
            TurnErrorKind::AuthRequired
        );
        assert_eq!(
            classify_error(Some(ProviderId::Aider), "Missing API key: please set DEEPSEEK_API_KEY", Some(1)),
            TurnErrorKind::AuthRequired
        );
    }

    #[test]
    fn cursor_invalid_key_is_auth_failure() {
        assert_eq!(classify_error(Some(ProviderId::CursorCli),
            "Warning: The provided API key is invalid. The API key was loaded from the CURSOR_API_KEY environment variable.", Some(1)),
            TurnErrorKind::AuthRequired);
    }

    #[test]
    fn revoked_oauth_is_auth_failure_without_a_login_hint() {
        assert_eq!(classify_error(Some(ProviderId::ClaudeCode), "Failed to authenticate: OAuth token revoked", Some(1)), TurnErrorKind::AuthRequired);
        assert_eq!(classify_error(Some(ProviderId::CodexCli), "refresh token has been revoked", Some(1)), TurnErrorKind::AuthRequired);
    }

    #[test]
    fn test_classify_rate_limited() {
        assert_eq!(
            classify_error(Some(ProviderId::ClaudeCode), "Hit your 5-hour limit · resets 3pm", Some(1)),
            TurnErrorKind::RateLimited
        );
        assert_eq!(
            classify_error(Some(ProviderId::CodexCli), "You've hit your usage limit. Try again in 2h 15m.", Some(1)),
            TurnErrorKind::RateLimited
        );
        assert_eq!(
            classify_error(None, "HTTP 429 Too Many Requests", Some(1)),
            TurnErrorKind::RateLimited
        );
    }

    #[test]
    fn test_classify_timeout() {
        assert_eq!(
            classify_error(None, "Command timed out (idle timeout exceeded)", None),
            TurnErrorKind::Timeout
        );
    }

    #[test]
    fn test_classify_spawn_failed() {
        assert_eq!(
            classify_error(None, "Failed to start claude: No such file or directory (os error 2)", None),
            TurnErrorKind::SpawnFailed
        );
    }

    #[test]
    fn test_classify_parse_error() {
        assert_eq!(
            classify_error(None, "Failed to parse child JSON output stream: unexpected token", Some(0)),
            TurnErrorKind::ParseError
        );
    }
}
