# Network Resilience Review — luwu

> Evaluated against 7 criteria. Source analyzed: `crates/luwu-llm/src/{openai,anthropic,sse}.rs`,
> `crates/luwu-server/src/{api,main,config}.rs`, `crates/luwu-tools/src/web_fetch.rs`.

---

## Summary

| # | Criterion | Score |
|---|-----------|-------|
| 1 | Reconnection mechanism | **F** |
| 2 | Timeout handling | **D** |
| 3 | Proxy resilience | **F** |
| 4 | Persistent connection health | **D** |
| 5 | Network error classification | **F** |
| 6 | Graceful degradation | **C** |
| 7 | Connection pool management | **C** |

**Overall: D** — The project has functional outbound HTTP and SSE streaming but
almost no network resilience patterns. A single transient failure (DNS hiccup,
TCP reset, provider rate-limit) terminates the entire agent turn with no retry,
no backoff, and no error differentiation. Only `web_fetch` and the axum SSE
keep-alive show awareness of network fragility.

---

## 1. Reconnection Mechanism — F

| Field | Detail |
|-------|--------|
| **Score** | F |
| **Evidence** | `openai.rs:131-138` and `anthropic.rs:122-129` — SSE `consume_stream` encounters a stream error, sends `LlmEvent::Error`, then **`break`**s immediately. No reconnection attempt. `openai.rs:86-94` and `anthropic.rs:78-87` — the initial `client.post(...).send().await` fails → `map_err` → return `Err`, no retry wrapper. `codegraph query 'retry'` returns zero results. No `backoff`, `exponential`, or `reconnect` identifier exists anywhere in the codebase. |
| **Recommendation** | Add a retry layer for transient HTTP failures (5xx, 429, connection errors) with exponential backoff + jitter. For SSE streams specifically, implement resume logic: on mid-stream disconnect, reconnect with the last received event ID (OpenAI's `stream_options` doesn't support this natively, but the agent loop could re-issue the full request if the turn hasn't completed). At minimum, wrap the initial connection in 2-3 retries with 1s/2s/4s backoff. Consider the `backon` or `tokio-retry` crate. |

---

## 2. Timeout Handling — D

| Field | Detail |
|-------|--------|
| **Score** | D |
| **Evidence** | **LLM providers** — `openai.rs:49` `Client::new()` and `anthropic.rs:48` `Client::new()`: plain constructor, **no `.timeout()` or `.connect_timeout()`**. A slow or hung LLM endpoint will block indefinitely. `openai.rs:64-68` `.send().await` has no per-request timeout override. **Memory workers** — `api.rs:959` (`run_consolidation_writer`), `api.rs:1008` (`run_checkpoint_writer`), `api.rs:1102` (`run_observer_worker`): all use `reqwest::Client::new()` with no timeout. These fire `tokio::spawn`ed LLM calls that can hang forever in the background. **Inbound server** — `main.rs:101-102`: `TcpListener::bind` + `axum::serve` with no timeout configuration. **Exception (good)** — `web_fetch.rs:115-123`: uses `reqwest::Client::builder().timeout(Duration::from_millis(timeout_ms))` with default 15s, plus a 5MB response-size cap (`web_fetch.rs:15, 155`). This is the only network component with proper timeout config. |
| **Recommendation** | Build a shared `reqwest::Client` with `.connect_timeout(10s).timeout(300s)` for LLM provider calls (LLM inference can be slow, but should not be infinite). Use a shorter timeout (30-60s) for the memory worker calls. Add `axum` request-level timeouts via `tower::timeout` middleware. |

---

## 3. Proxy Resilience — F

| Field | Detail |
|-------|--------|
| **Score** | F |
| **Evidence** | `codegraph query 'proxy'` returns zero results. No `proxy`, `socks`, or proxy-related string anywhere in the codebase. `config.rs:29-36` (`ProviderConfig`) has no proxy field. reqwest will implicitly honor `HTTP_PROXY`/`HTTPS_PROXY` environment variables (built-in behavior), but there is no explicit proxy configuration, no proxy switching, no proxy failure handling, and no way to configure a proxy per-provider in `config.toml`. |
| **Recommendation** | Add optional `proxy` field to `ProviderConfig`. When set, build the `reqwest::Client` with `.proxy(reqwest::Proxy::all(&url)?)`. At minimum, document that `HTTPS_PROXY` env vars work via reqwest defaults. This matters for users in regions where LLM APIs require a proxy. |

---

## 4. Persistent Connection Health — D

| Field | Detail |
|-------|--------|
| **Score** | D |
| **Evidence** | **Good** — axum SSE streams have `KeepAlive`: `api.rs:518-520` (`chat_completions`) and `api.rs:882-884` (`agent_chat`) both use `KeepAlive::new().interval(Duration::from_secs(15))`. This sends comment frames to the client every 15s to prevent idle-connection drops. **Missing** — the SSE parser (`sse.rs:34-71`) has no read timeout on `stream.next().await` (line 49). If the LLM provider stalls mid-stream (sends headers, then goes silent), `parse_sse_stream` will block forever on `stream.next()`. There is no heartbeat or liveness check on the **inbound** SSE connection from the LLM provider. **Ignored pings** — `anthropic.rs:248`: `// message_stop, ping, etc. — ignore.` Anthropic sends `event: ping` as a keep-alive signal; the code correctly ignores the content but does not use the ping arrival as a liveness signal (i.e., it doesn't reset a "last activity" timer). |
| **Recommendation** | Wrap `stream.next()` in `tokio::time::timeout(30s)` inside `parse_sse_stream` or `consume_stream`. If no data arrives within 30s, treat the connection as stale and propagate an error. For Anthropic, use incoming `ping` events to reset the staleness timer. Consider an SSE-layer idle timeout in the provider trait. |

---

## 5. Network Error Classification — F

| Field | Detail |
|-------|--------|
| **Score** | F |
| **Evidence** | Every network error is flattened into a string: `openai.rs:94` → `LuwuError::Llm(format!("OpenAI request failed: {e}"))`; `openai.rs:131-136` → `LuwuError::Llm(format!("SSE stream error: {e}"))`; `anthropic.rs:87, 123-127` — identical pattern. HTTP status errors (`openai.rs:96-102`, `anthropic.rs:89-95`) capture the status code in the string but don't classify it. There is no enum variant for `RateLimited(429)`, `AuthFailed(401/403)`, `ServerError(5xx)`, `NetworkTimeout`, `ConnectionReset`, etc. All errors are `LuwuError::Llm(String)`. The consumer (engine, then SSE layer) receives a raw string and can only forward it to the client. |
| **Recommendation** | Introduce a `NetworkError` enum with variants: `Timeout`, `ConnectionReset`, `DnsFailed`, `RateLimited(RetryAfter)`, `AuthError`, `ServerError(u16)`, `BadRequest`. Map `reqwest::Error::is_timeout()`, `is_connect()`, `is_request()` and HTTP status codes into these variants. This enables the retry layer (criterion 1) to decide which errors are retryable. |

---

## 6. Graceful Degradation — C

| Field | Detail |
|-------|--------|
| **Score** | C |
| **Evidence** | **Good** — `api.rs:502-510` (`TurnEvent::Error { message }` → SSE error chunk → break): when the agent turn fails, the client receives a structured error event and the SSE stream closes cleanly. Session running state is always reset on exit (`api.rs:879`, `api.rs:938`). `api.rs:661-667`: concurrent-turn detection returns HTTP 409. Memory workers swallow errors with `.ok()` (`api.rs:774, 800-801, 858-864`), so a memory write failure doesn't crash the agent turn — but failures are silently discarded with no logging. **Missing** — no fallback to an alternative provider or model on failure. No circuit breaker to stop hammering a failing endpoint. No partial-result delivery (if the LLM sends 500 tokens then drops, the partial text is lost because `consume_stream` breaks on error without flushing accumulated deltas as a partial result). |
| **Recommendation** | On SSE mid-stream error, emit any accumulated text as a `LlmEvent::TextDelta` before the `Error` event so partial responses reach the client. Add `tracing::warn!` for memory worker failures. Consider an optional fallback provider in config. |

---

## 7. Connection Pool Management — C

| Field | Detail |
|-------|--------|
| **Score** | C |
| **Evidence** | **Provider clients** — `openai.rs:35` stores `client: Client` as a struct field, so within a single `OpenAiProvider` instance, the reqwest connection pool is reused across calls. Same for `anthropic.rs:34`. **But** — `api.rs:337` (`chat_completions`) and `api.rs:682` (`agent_chat`) create a **new `OpenAiProvider` per HTTP request**: `let provider = OpenAiProvider::with_base_url(...)`. Since `with_base_url` calls `Client::new()` (`openai.rs:49`), each inbound request gets a fresh client with an empty connection pool. No connection reuse across requests. **Worse** — `api.rs:959, 1008, 1102`: every memory worker function creates its own `reqwest::Client::new()` on every call. These fire as `tokio::spawn`ed background tasks, each burning a new TLS handshake and fresh pool. **Exception (good)** — `web_fetch.rs:115-123` builds a client with `.timeout()` + `.user_agent()`, but still per-invocation. |
| **Recommendation** | Create one shared `reqwest::Client` in `AppState` (or a `ClientFactory` keyed by base_url) and pass it to providers and workers. reqwest's `Client` is `Clone` (cheap — `Arc` internally) and designed for reuse. This gives connection pooling, TLS session resumption, and HTTP/2 multiplexing for free. Also set `.pool_idle_timeout` and `.pool_max_idle_per_host` explicitly. |

---

## Notable: Two reqwest Major Versions

`Cargo.toml` pulls in reqwest **0.12** (direct dep) and reqwest **0.13** (via `kawat`).
This means two separate connection pools, two TLS stacks, and potential version-conflict
headaches if both ever need to talk to the same endpoint. Not a direct resilience bug,
but worth noting for future dependency consolidation.

---

*Generated by network resilience review. No source files were modified.*
