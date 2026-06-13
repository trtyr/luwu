# Topic: Runtime Resilience

## Problem

Three F grades from the review:
- **Graceful shutdown: F** — `main.rs:101-102`, no signal handling
- **Retry logic: F** — zero retry code, no retry dependency
- **Timeout on LLM: D** — bare `Client::new()`, no timeout config

## Phase 1 Tasks (P0)

### Graceful shutdown

```
// main.rs — replace axum::serve(listener, app).await.unwrap()
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(|e| { eprintln!("Server error: {e}"); })?;
```

`shutdown_signal()`: listen for `ctrl_c()` + `SIGTERM` (unix).

Need to also: propagate a `CancellationToken` to in-flight handlers so they can drain SSE streams. `tokio_util::sync::CancellationToken` is the standard approach.

### LLM client timeout

```rust
// openai.rs:49 — replace Client::new()
Client::builder()
    .timeout(Duration::from_secs(120))
    .connect_timeout(Duration::from_secs(10))
    .build()
    .expect("failed to build HTTP client")
```

Same pattern in `anthropic.rs:48`. Also apply to the 4 worker functions in `api.rs`.

### Replace silent .ok() with warn! logging

`api.rs:774,800,858` — memory worker errors silently dropped. Replace with `if let Err(e) = ... { tracing::warn!(?e, "worker failed"); }`.

## Phase 3 Tasks (P1)

### Shared reqwest::Client

Currently `Client::new()` per request (providers, workers, web_fetch). Create one client in `AppState`:

```rust
// In AppState or similar
pub fn build_http_client() -> reqwest::Client {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .expect("HTTP client")
}
```

Pass `reqwest::Client` (it's `Clone` — cheap `Arc` internally) to all consumers.

### Retry layer

Wraps LLM API calls. Retry on: 429 (rate limited), 500/502/503/504 (server error), connection errors.

```text
attempt 1: immediate
attempt 2: wait 1s + jitter
attempt 3: wait 2s + jitter
attempt 4 (cap): wait 4s + jitter
→ fail
```

Respect `Retry-After` header on 429 if present.

**Dependency:** Needs error classification (Phase 3.2) to know which errors are retryable. See [Q1](../open-questions.md#q1-retry-crate-choice--tokio-retry-vs-backon-vs-hand-rolled).

### SSE stream liveness

`sse.rs:49` — `stream.next().await` has no timeout. If LLM stalls mid-stream, parser blocks forever.

Wrap in `tokio::time::timeout(Duration::from_secs(30), stream.next())`. On timeout → `LlmEvent::Error`.

For Anthropic: use incoming `ping` events to reset the timer.

### Fire-and-forget task tracking

Replace bare `tokio::spawn` with `JoinSet`:

```rust
let mut worker_set = tokio::task::JoinSet::new();
worker_set.spawn(async move { run_observer_worker(...).await });
// On cancel: worker_set.abort_all();
// On shutdown: worker_set.shutdown().await;
```

## Constraints

- Graceful shutdown must not hang if LLM API is unresponsive — needs a drain timeout (e.g., 30s max wait).
- Retry must not double-charge API credits for non-idempotent calls — but LLM completions are effectively idempotent (same input → acceptable to retry).
