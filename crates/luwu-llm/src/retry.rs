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
