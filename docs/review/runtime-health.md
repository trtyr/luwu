# Runtime Health & Robustness Review — Luwu

> **Scope**: All crates in the luwu workspace (luwu-core, luwu-llm, luwu-memory, luwu-server, luwu-tools).
> **Method**: Static code analysis via codegraph symbol search + manual file reads of all 27 source files.
> **Date**: 2025-07-13

---

## Summary

| Criterion | Score | One-liner |
|---|:---:|---|
| Timeout Handling | **D** | Tools have timeouts; all LLM/HTTP client calls are bare `Client::new()` with zero timeout config. |
| Retry Logic | **F** | No retry, no backoff, no circuit breaker anywhere in the codebase. |
| Logging | **B** | Good tracing setup and coverage; silent error swallowing in workers; full API response bodies leaked in errors. |
| Concurrency Safety | **C+** | Solid per-request isolation and locking; TOCTOU race in agent_chat; sync I/O inside async locks. |
| Configuration Validation | **B-** | Fail-fast with clear messages at startup; missing field-level validation (empty key, URL format, ranges). |
| Resource Cleanup | **D+** | Write-through persistence is good; no shared connection pool; fire-and-forget tasks never cleaned up. |
| Graceful Shutdown | **F** | `axum::serve().await.unwrap()` — no signal handling, no drain, no cleanup. |

**Overall: C-.** The architecture is clean and well-structured, but runtime resilience is minimal. The project works well under ideal conditions (fast LLM, single user, clean shutdown) and has no defense against the failure modes that matter in production: slow/hanging LLM APIs, concurrent requests to the same session, process termination mid-turn, and transient network errors.

---

## 1. Timeout Handling — Grade: D

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Tool-level timeouts | A | `bash.rs:104` — `tokio::time::timeout(Duration::from_secs(30), ...)`, user-configurable. `web_fetch.rs:115-116` — `reqwest::Client::builder().timeout(Duration::from_millis(15000))` + 5MB body cap (`web_fetch.rs:15`). | No change needed. |
| LLM provider HTTP | F | `openai.rs:49` — `client: Client::new()`. No `.timeout()`, no `.connect_timeout()`. LLM API calls (potentially 60s+ for long generations) have no upper bound. If the provider hangs, the agent loop hangs indefinitely. | Configure a reqwest client builder with `.timeout(Duration::from_secs(120))` and `.connect_timeout(Duration::from_secs(10))`. Make these configurable in `ProviderConfig`. |
| Worker HTTP calls | F | `api.rs:959,1008,1102,1174` — all four worker functions (`run_consolidation_writer`, `run_checkpoint_writer`, `run_observer_worker`, `run_reflector_worker`) create bare `reqwest::Client::new()`. These are `tokio::spawn`'d fire-and-forget — a hanging worker task leaks indefinitely. | Same as above. Share a single configured client, or at minimum pass a timeout. |
| Agent loop iterations | D | `engine.rs:358-617` — the loop has `max_iterations` (50) but no per-iteration wall-clock timeout. A single stuck LLM call blocks the entire turn with no recovery. | Wrap each `provider.stream()` + consumption in a `tokio::time::timeout`. On timeout, emit `TurnEvent::Error`. |
| SSE keep-alive | B | `api.rs:519,883,942` — `KeepAlive::new().interval(Duration::from_secs(15))`. Prevents proxy timeouts. | Reasonable. Could be configurable. |

**Why D, not F**: The two tools that do external I/O (bash, web_fetch) have solid timeout implementations with user-configurable values. But the most critical I/O path — LLM API calls — has zero protection.

---

## 2. Retry Logic — Grade: F

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| LLM API retries | F | `openai.rs:86-94` — HTTP send returns `Err` → propagated as `LuwuError::Llm` → engine emits `TurnEvent::Error` → stream ends. No retry on 429, 500, 502, 503, 504. | Add retry with exponential backoff for 5xx and 429 status codes. Cap at 3 attempts. Consider `backon` or `tokio-retry` crate. |
| Worker task retries | F | `api.rs:773-774` — workers are `tokio::spawn`'d and their results are `.await.ok()`'d or silently dropped. A single transient failure permanently loses that observation/reflection. | Log failures at minimum. Consider retry for transient HTTP errors. |
| Circuit breaker | F | No circuit breaker pattern anywhere. A repeatedly failing LLM endpoint will be hit on every request with no backoff. | If retry is added, consider a simple circuit breaker that trips after N consecutive failures. |
| Retry-related dependencies | F | `Cargo.toml` — no `tokio-retry`, `backon`, `backoff`, or similar crate in any workspace member. | Add a retry crate dependency. |

**Why F**: Zero retry infrastructure. Every transient failure (rate limit, network blip, temporary 5xx) is terminal. For an LLM agent framework that makes potentially dozens of API calls per turn, this is a significant reliability gap.

---

## 3. Logging — Grade: B

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Tracing setup | A | `main.rs:16-18` — `tracing_subscriber::fmt().with_env_filter(...).init()`. Default `info`, overridable via `RUST_LOG`. Clean and standard. | No change needed. |
| Lifecycle coverage | A | Turn start (`engine.rs:137`), turn finish (`engine.rs:293`), tool execution (`engine.rs:230`), skill activation (`engine.rs:483`), session recovery (`session_manager.rs:134`), observer/reflector results (`api.rs:1153,1228`). | No change needed. |
| Log levels | A | `info!` for lifecycle, `debug!` for internals (`engine.rs:214,280`), `warn!` for recoverable (`session_manager.rs:92,277`). Matches conventions doc. | No change needed. |
| Secret safety | B | API key is never logged. Bearer auth header is constructed inline, not logged. | Good. |
| Sensitive data in errors | C | `openai.rs:96-101` — on non-2xx, the **full response body** is included in the error string: `format!("OpenAI API error {status}: {text}")`. This error propagates to `TurnEvent::Error` → SSE → client. If the API returns a verbose error with request echo, it could leak information. | Truncate or sanitize error response bodies before propagation. |
| Silent error swallowing | C | `api.rs:774` — `run_consolidation_writer(...).await.ok()` — errors silently dropped. `api.rs:591,829` — `memory.write_checkpoint_raw(...).ok()` — checkpoint write failures silently dropped. `api.rs:722-723` — correction save errors dropped. | Replace `.ok()` with proper `warn!` logging on error paths. |
| Request timing | C | No `tracing::instrument` spans with timing. No duration logging for LLM calls, tool execution, or HTTP requests. Makes performance debugging hard. | Add `#[tracing::instrument]` to key async functions, or manual `Instant::now()` timing for LLM calls. |

**Why B**: The foundation is solid — proper tracing setup, good lifecycle coverage, appropriate levels. The deductions are for silent error swallowing (which hides problems) and full response body leakage in errors.

---

## 4. Concurrency Safety — Grade: C+

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Session isolation | A | `api.rs:684-696` — each `agent_chat` builds a fresh `TurnEngine`, `ToolRegistry`, `MemoryStore`, `CycleState`. No shared mutable state across sessions. Architecture doc confirms this is intentional. | No change needed. |
| SessionManager locking | B+ | `session_manager.rs:51` — `Arc<RwLock<HashMap<...>>>`. `append_messages` performs append + disk-write atomically under a single write lock (`session_manager.rs:214-224`). | Good pattern. |
| TOCTOU race (agent_chat) | D | `api.rs:642` — `state.sessions.get(&id).await` takes a read lock, clones the session, **releases the lock**. `api.rs:661` — checks `session.is_running` on the stale clone. `api.rs:670` — `set_running(&id, true)` takes a **new** write lock. Between `get()` and `set_running()`, another request can pass the same check. Two concurrent POSTs to the same session both start turns. | Combine the check-and-set into a single atomic operation: `set_running` should fail if already running (return `None` or a `Conflict` variant). The caller should treat failure as 409. |
| Sync I/O in async lock | C | `session_manager.rs:268-284` — `persist_session` calls `std::fs::write` (blocking) **while holding the async `RwLock` write guard**. Under load, this blocks all session operations for the duration of disk I/O. | Use `tokio::fs::write` or spawn the write to a blocking thread pool. Alternatively, use a dedicated write-ahead pattern. |
| SQLite Mutex blocking | C | `search_index.rs:24` — `conn: Mutex<Connection>` (std blocking mutex). Called from `MemoryStore` methods that may be invoked from async contexts. On contention, this blocks the tokio worker thread. | Wrap SQLite operations in `tokio::task::spawn_blocking`, or use a connection pool. Low risk in practice (single-session access pattern). |
| Fire-and-forget tasks | C | Multiple `tokio::spawn` in `api.rs`: correction detection (721), message persistence (756), consolidation (773), observer (799,832), reflector (858). None are tracked, none can be cancelled. If the session is cancelled or the server shuts down, these tasks are killed mid-operation. | Track task handles for cancellation. At minimum, use `tokio::task::JoinSet` for workers spawned per request. |
| EventBus broadcast | B | `event.rs` — `tokio::broadcast` channel. Slow subscribers may miss events (broadcast semantics), but this is by design and documented. | No change needed. |

**Why C+**: The fundamental design (fresh state per request, proper RwLock usage) is sound. The TOCTOU race is a real correctness bug that can cause data corruption when two turns overlap on the same session. The sync I/O under async locks is a latent performance issue under load.

---

## 5. Configuration Validation — Grade: B-

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Fail-fast at startup | A | `main.rs:24-37` — `Config::load()` errors exit with `eprintln!` + `exit(1)`. `config.resolve(None)` verified before server starts. Clear, actionable messages including config file path. | No change needed. |
| Error types | A | `config.rs:109-118` — `ConfigError` enum: `Io(PathBuf, io::Error)`, `Parse(PathBuf, toml::Error)`, `NoDefaultProvider`, `ProviderNotFound(String)`. Each with descriptive `#[error("...")]` message. | No change needed. |
| TOML parsing | A | `config.rs:65` — `toml::from_str` with proper error mapping. Malformed TOML is caught and reported with file path. | No change needed. |
| Defaults | B | `config.rs:82` — model defaults to `"gpt-4o-mini"`. `config.rs:88` — base_url defaults to `"https://api.openai.com/v1"`. Reasonable for the common case. | Fine. |
| Missing config file | B | `config.rs:56-58` — if config file doesn't exist, returns `Config::default()` (all fields `None`/empty). This succeeds silently, and the error only surfaces later at `resolve()` with "No default provider configured". Not immediately clear to the user that the config file is missing. | Print a hint when config file is missing: "Config file not found at {path}. See docs for setup." |
| API key validation | C | `config.rs:31` — `api_key: String` — no validation that it's non-empty. An empty api_key would cause a 401 from the LLM provider at runtime, not at startup. | Validate non-empty at `resolve()` time. |
| URL validation | C | `config.rs:33` — `base_url: Option<String>` — no format validation. A malformed URL would cause a confusing reqwest error at runtime. | Validate URL format at `resolve()` time. |
| Numeric range validation | C | `config.rs:34-35` — `temperature: Option<f64>`, `max_tokens: Option<u64>` — no range checks. `temperature: -5.0` or `max_tokens: 0` would be passed through to the API. | Clamp/validate at resolve time: temperature ∈ [0, 2], max_tokens > 0. |
| No runtime config knobs | C | No configurable timeout, retry, pool size, or rate limit settings. Everything is hardcoded constants. | Add runtime-configurable settings for the values identified in sections 1-2. |

**Why B-**: The startup validation story is good (fail-fast, clear messages, proper error types). The gap is in field-level validation — the config trusts its input and pushes problems to runtime where they surface as confusing API errors instead of clear startup failures.

---

## 6. Resource Cleanup — Grade: D+

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Write-through persistence | A | `session_manager.rs:162,219,276` — every session mutation writes to disk synchronously before releasing the lock. No buffered writes to lose on crash. | No change needed. |
| SSE channel lifecycle | B | `engine.rs:317` — mpsc channel created per turn. When the spawned task exits or the receiver is dropped, the channel closes cleanly. SSE stream handler in `api.rs` breaks on `Done`/`Cancelled`/`Error`. | Reasonable. |
| Connection pool proliferation | D | `openai.rs:49` — `Client::new()` per `OpenAiProvider`, which is constructed per request (`api.rs:682`). Workers each create their own `Client::new()` (`api.rs:959,1008,1102,1174`). `web_fetch.rs:115` — new client per fetch. Each `Client::new()` creates an independent connection pool. Under load, this means dozens of connection pools with no sharing and no limit. | Create one shared `reqwest::Client` at startup (in `AppState` or a provider factory) with a configured `PoolMaxIdlePerHost` and `PoolIdleTimeout`. Pass it to all consumers. |
| Fire-and-forget task cleanup | D | `tokio::spawn` calls in `api.rs` (721, 756, 773, 799, 832, 858) — no `JoinHandle` stored, no cancellation. If a session is cancelled via `/cancel`, the spawned memory workers (observer, reflector, consolidation) continue running, consuming API credits and system resources. | Store task handles; abort them on cancel or on stream end. Use `tokio::task::JoinSet`. |
| File handle management | C | `history.rs:48` — `OpenOptions::new().append(true).open(&self.path)` — opens and closes the file on every `append()` call. Under high message volume, this causes repeated open/close overhead. | Keep a persistent file handle in `HistoryLog` (e.g., `BufWriter<File>` with periodic flush). |
| No explicit Drop impls | C | No `impl Drop` found in any crate. Relies on RAII (Connection, File, Channel all Drop correctly). This is fine for Rust, but means no explicit logging or cleanup-on-drop for debugging. | Not a bug, but adding `impl Drop` with debug logging on key resources (SearchIndex, MemoryStore) would help with leak diagnosis. |
| Disk space management | D | No rotation or size limit on session JSON files or history JSONL files. A long-running session with many turns will grow unbounded. Memory consolidation exists (`check_consolidation`) but it's per-request and doesn't enforce hard limits. | Add max file size thresholds with warning logs. Consider rotation for history logs. |

**Why D+**: The write-through persistence model is genuinely good — data won't be lost on crash. But the lack of connection pooling, untracked background tasks, and no disk space management make this fragile under sustained load.

---

## 7. Graceful Shutdown — Grade: F

| Aspect | Score | Evidence | Recommendation |
|---|:---:|---|---|
| Signal handling | F | `main.rs:101-102` — `axum::serve(listener, app).await.unwrap()`. No `with_graceful_shutdown`, no `tokio::signal::ctrl_c()`, no `SIGTERM` handler. Process termination kills everything immediately. | Use `axum::serve(listener, app).with_graceful_shutdown(shutdown_signal())` where `shutdown_signal` listens for `ctrl_c()` and `SIGTERM`. |
| Connection draining | F | No `Shutdown` token propagated to handlers. In-flight SSE streams (`/v1/sessions/{id}/chat`, `/v1/chat/completions`) are killed mid-stream. Clients see a broken connection with no error event. | On shutdown signal: stop accepting new connections, let in-flight requests complete (with a timeout), then exit. |
| Session state cleanup | F | If killed during `agent_chat`, the session's `is_running` flag stays `true` forever (it's only reset to `false` at `api.rs:879,938` — inside the stream handler that gets killed). On restart, `load_from_disk` correctly resets `is_running: false` (`session_manager.rs:126`), so this recovers on restart. But within the same process lifetime, the session is permanently locked. | The `set_running(false)` call should be in a cleanup path that runs even if the stream is interrupted (e.g., a `Drop` guard or `finally`-style pattern). |
| Background task cleanup | F | Fire-and-forget `tokio::spawn` tasks (memory workers, consolidation) are killed mid-operation. In-flight HTTP requests to the LLM API are dropped — the API call is charged but the result is lost. | Track spawned tasks; abort them on shutdown. |
| Error handling on serve | F | `main.rs:102` — `.unwrap()` on `axum::serve().await`. If the server errors (port conflict, socket error), this panics with an unhelpful message. | Replace with proper error handling: `match axum::serve(...).await { Ok(()) => {}, Err(e) => { eprintln!("Server error: {e}"); std::process::exit(1); } }`. |
| Startup retry | F | `main.rs:101` — `TcpListener::bind(addr).await.unwrap()` — if port 51740 is in use, the process panics. No retry, no helpful message (though `main.rs:86` prints the address). | Handle bind error gracefully with a message like "Port 51740 is already in use. Is another luwu instance running?" |

**Why F**: There is no graceful shutdown whatsoever. This is the single most impactful gap in the codebase. Every `docker stop`, `kill`, Ctrl-C, or deploy kills the process with no cleanup. For a server that manages long-running agent turns, makes LLM API calls, and persists session state, this is a significant reliability risk.

**Note**: The session `is_running` recovery on restart (`session_manager.rs:126`) is a saving grace — sessions won't be permanently locked across restarts. But within a single process lifetime, a killed turn leaves the session in a bad state.

---

## Priority Recommendations

Ranked by impact × effort:

| Priority | Issue | Effort | Impact |
|:---:|---|---|---|
| **P0** | Add graceful shutdown with signal handling + connection drain | Small | Critical — prevents data loss and resource leaks on every restart |
| **P0** | Add timeout to LLM provider HTTP client | Trivial | Critical — prevents indefinite hangs on slow/unresponsive LLM APIs |
| **P1** | Fix TOCTOU race in `agent_chat` (atomic check-and-set) | Small | High — prevents concurrent turn corruption |
| **P1** | Add retry with backoff for LLM API transient failures | Medium | High — dramatically improves reliability under real-world conditions |
| **P1** | Track and cancel fire-and-forget tasks on session cancel/shutdown | Medium | High — prevents resource leaks and wasted API calls |
| **P2** | Share a single configured `reqwest::Client` across all consumers | Small | Medium — reduces connection proliferation |
| **P2** | Replace silent `.ok()` error swallowing with `warn!` logging | Small | Medium — improves observability and debugging |
| **P2** | Add field-level config validation (empty key, URL format, ranges) | Small | Medium — improves startup UX |
| **P3** | Sanitize LLM API error response bodies before propagation | Trivial | Low — prevents potential information leakage |
| **P3** | Move sync file I/O out of async locks (`persist_session`) | Small | Low — only matters under high concurrency |
| **P3** | Add request timing instrumentation (`tracing::instrument`) | Small | Low — improves performance debugging |
