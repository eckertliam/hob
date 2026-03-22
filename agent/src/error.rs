//! Error classification and retry logic for API errors.

use std::fmt;
use std::time::Duration;

/// The kind of API error, used to decide retry/compaction/bail.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiErrorKind {
    ContextOverflow,
    RateLimit,
    Auth,
    Overloaded,
    ServerError,
    Unknown,
}

/// A classified API error carrying the kind, message, and optional retry hint.
#[derive(Debug, Clone)]
pub struct ClassifiedError {
    pub kind: ApiErrorKind,
    pub message: String,
    pub retry_after: Option<Duration>,
    pub status: Option<u16>,
}

impl fmt::Display for ClassifiedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ClassifiedError {}

/// Classify an HTTP error response into an ApiErrorKind.
pub fn classify(status: u16, body: &str) -> ClassifiedError {
    let lower = body.to_lowercase();

    let kind = if status == 401 || status == 403 {
        ApiErrorKind::Auth
    } else if status == 429 || lower.contains("rate_limit") || lower.contains("too many requests")
    {
        ApiErrorKind::RateLimit
    } else if status == 503
        || lower.contains("overloaded")
        || lower.contains("unavailable")
        || lower.contains("exhausted")
    {
        ApiErrorKind::Overloaded
    } else if lower.contains("prompt is too long")
        || lower.contains("exceeds the context window")
        || lower.contains("token limit")
        || lower.contains("context length exceeded")
    {
        ApiErrorKind::ContextOverflow
    } else if status >= 500 {
        ApiErrorKind::ServerError
    } else {
        ApiErrorKind::Unknown
    };

    ClassifiedError {
        kind,
        message: if body.len() > 500 {
            format!("HTTP {status}: {}...", &body[..500])
        } else {
            format!("HTTP {status}: {body}")
        },
        retry_after: None,
        status: Some(status),
    }
}

/// Whether this error kind is worth retrying.
pub fn is_retryable(kind: &ApiErrorKind) -> bool {
    matches!(
        kind,
        ApiErrorKind::RateLimit | ApiErrorKind::Overloaded | ApiErrorKind::ServerError
    )
}

/// Compute retry delay with exponential backoff.
///
/// Without a Retry-After header: 2s × 2^(attempt-1), capped at 30s.
/// With a Retry-After header: use it directly.
const INITIAL_DELAY_MS: u64 = 2000;
const MAX_DELAY_MS: u64 = 30_000;

pub fn retry_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra.min(Duration::from_secs(120));
    }
    let delay = INITIAL_DELAY_MS.saturating_mul(1u64.wrapping_shl(attempt.saturating_sub(1)));
    Duration::from_millis(delay.min(MAX_DELAY_MS))
}

/// Try to parse a Retry-After header value (seconds as integer or float).
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    // Try integer seconds first
    if let Ok(secs) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // Try float seconds
    if let Ok(secs) = value.trim().parse::<f64>() {
        return Some(Duration::from_secs_f64(secs));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rate_limit_429() {
        let e = classify(429, "rate limit exceeded");
        assert_eq!(e.kind, ApiErrorKind::RateLimit);
    }

    #[test]
    fn test_classify_auth_401() {
        let e = classify(401, "unauthorized");
        assert_eq!(e.kind, ApiErrorKind::Auth);
    }

    #[test]
    fn test_classify_auth_403() {
        let e = classify(403, "forbidden");
        assert_eq!(e.kind, ApiErrorKind::Auth);
    }

    #[test]
    fn test_classify_overloaded_503() {
        let e = classify(503, "service unavailable");
        assert_eq!(e.kind, ApiErrorKind::Overloaded);
    }

    #[test]
    fn test_classify_context_overflow_by_body() {
        let e = classify(400, "prompt is too long: 250000 tokens");
        assert_eq!(e.kind, ApiErrorKind::ContextOverflow);
    }

    #[test]
    fn test_classify_server_error_500() {
        let e = classify(500, "internal server error");
        assert_eq!(e.kind, ApiErrorKind::ServerError);
    }

    #[test]
    fn test_classify_unknown() {
        let e = classify(400, "bad request: missing model");
        assert_eq!(e.kind, ApiErrorKind::Unknown);
    }

    #[test]
    fn test_retry_delay_exponential() {
        assert_eq!(retry_delay(1, None), Duration::from_millis(2000));
        assert_eq!(retry_delay(2, None), Duration::from_millis(4000));
        assert_eq!(retry_delay(3, None), Duration::from_millis(8000));
        assert_eq!(retry_delay(4, None), Duration::from_millis(16000));
        assert_eq!(retry_delay(5, None), Duration::from_millis(30000));
        assert_eq!(retry_delay(6, None), Duration::from_millis(30000));
    }

    #[test]
    fn test_retry_delay_respects_header() {
        let d = retry_delay(1, Some(Duration::from_secs(10)));
        assert_eq!(d, Duration::from_secs(10));
    }

    #[test]
    fn test_retry_delay_caps_header() {
        let d = retry_delay(1, Some(Duration::from_secs(300)));
        assert_eq!(d, Duration::from_secs(120));
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable(&ApiErrorKind::RateLimit));
        assert!(is_retryable(&ApiErrorKind::Overloaded));
        assert!(is_retryable(&ApiErrorKind::ServerError));
        assert!(!is_retryable(&ApiErrorKind::Auth));
        assert!(!is_retryable(&ApiErrorKind::ContextOverflow));
        assert!(!is_retryable(&ApiErrorKind::Unknown));
    }

    #[test]
    fn test_parse_retry_after_integer() {
        assert_eq!(parse_retry_after("5"), Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_retry_after_float() {
        assert_eq!(
            parse_retry_after("2.5"),
            Some(Duration::from_secs_f64(2.5))
        );
    }

    #[test]
    fn test_parse_retry_after_invalid() {
        assert_eq!(parse_retry_after("not-a-number"), None);
    }
}
