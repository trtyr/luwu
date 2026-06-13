//! Retry layer for LLM API calls.
//!
//! Provides exponential backoff with jitter for transient HTTP failures.
//! Retries on 429 (rate limited), 500/502/503/504 (server errors), and
//! connection/timeout errors. Respects the `Retry-After` header on 429.

use std::time::Duration;

use reqwest::{RequestBuilder, Response};
use tracing::debug;

use crate::error::{LlmError, truncate_body};

/// Maximum number of attempts (initial + retries).
const MAX_ATTEMPTS: u32 = 3;

/// Initial backoff delay in seconds.
const INITIAL_DELAY_SECS: u64 = 1;

/// Maximum backoff delay in seconds (cap).
const MAX_DELAY_SECS: u64 = 4;

/// Pseudo-random jitter in milliseconds (0..500).
fn jitter_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 500)
        .unwrap_or(0)
}

/// Compute backoff delay for the given attempt (1-indexed).
fn backoff_delay(attempt: u32, retry_after: Option<u64>) -> Duration {
    let base = retry_after.unwrap_or_else(|| {
        INITIAL_DELAY_SECS
            .saturating_mul(1 << (attempt - 1))
            .min(MAX_DELAY_SECS)
    });
    Duration::from_secs(base) + Duration::from_millis(jitter_ms())
}

/// Check if an HTTP status code is retryable.
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

/// Send a request with exponential backoff retry on transient failures.
///
/// The `RequestBuilder` must be cloneable (i.e., its body must be reusable).
/// This is the case for all JSON-bodied LLM API requests.
#[tracing::instrument(skip(request), fields(attempt))]
pub async fn send_with_retry(request: &RequestBuilder) -> Result<Response, LlmError> {
    let mut attempt = 0;

    loop {
        attempt += 1;

        // Clone the request so the original survives for potential retries.
        let req = request
            .try_clone()
            .ok_or_else(|| LlmError::Stream("request body not cloneable for retry".into()))?;

        match req.send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),

            Ok(resp) => {
                let status = resp.status().as_u16();

                if !is_retryable_status(status) || attempt >= MAX_ATTEMPTS {
                    let text = resp.text().await.unwrap_or_default();
                    return Err(LlmError::Status {
                        status,
                        body: truncate_body(&text, 500),
                    });
                }

                // Respect Retry-After header (seconds) on 429.
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                let delay = backoff_delay(attempt, retry_after);
                debug!(attempt, status, ?delay, "Retrying after server error");
                tokio::time::sleep(delay).await;
            }

            Err(e) => {
                // Only retry on connection or timeout errors.
                let retryable = (e.is_connect() || e.is_timeout()) && attempt < MAX_ATTEMPTS;
                if !retryable {
                    return Err(LlmError::Http(e));
                }

                let delay = backoff_delay(attempt, None);
                debug!(attempt, error = %e, ?delay, "Retrying after connection error");
                tokio::time::sleep(delay).await;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ─── is_retryable_status ──────────────────────────────

    #[test]
    fn retryable_statuses() {
        for code in [429, 500, 502, 503, 504] {
            assert!(is_retryable_status(code), "{code} should be retryable");
        }
    }

    #[test]
    fn non_retryable_statuses() {
        for code in [200, 400, 401, 403, 404, 301, 201] {
            assert!(!is_retryable_status(code), "{code} should NOT be retryable");
        }
    }

    // ─── backoff_delay ────────────────────────────────────

    #[test]
    fn backoff_exponential_schedule() {
        // attempt 1 → 1s, attempt 2 → 2s, attempt 3 → 4s (capped).
        // jitter adds 0..500ms but as_secs() truncates, so secs are stable.
        assert_eq!(backoff_delay(1, None).as_secs(), 1);
        assert_eq!(backoff_delay(2, None).as_secs(), 2);
        assert_eq!(backoff_delay(3, None).as_secs(), 4);
    }

    #[test]
    fn backoff_caps_at_max() {
        // Large attempt would be 1<<9 = 512s, should cap at 4s.
        assert_eq!(backoff_delay(10, None).as_secs(), 4);
    }

    #[test]
    fn backoff_respects_retry_after() {
        let d = backoff_delay(1, Some(10));
        assert_eq!(d.as_secs(), 10); // 10s base + 0..500ms jitter
    }

    // ─── jitter ───────────────────────────────────────────

    #[test]
    fn jitter_is_bounded() {
        for _ in 0..200 {
            let j = jitter_ms();
            assert!(j < 500, "jitter {j} must be < 500");
        }
    }
}
