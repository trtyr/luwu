# Risk Hotspots

Prioritized systemic issues from the [review](../../review/summary.md). Each entry has evidence (file:line) and a recommended fix.

## P0 — Prevent immediate production failures

### 1. No graceful shutdown
- **Evidence:** `main.rs:101-102` — `axum::serve().await.unwrap()`, no signal handling, no drain
- **Impact:** Every Ctrl-C / kill / deploy kills in-flight SSE streams, LLM calls, memory workers mid-operation. Session `is_running` stays locked until restart.
- **Fix:** `axum::serve(...).with_graceful_shutdown(shutdown_signal())` + task tracking for cleanup. ~20 lines.

### 2. No timeout on LLM HTTP clients
- **Evidence:** `openai.rs:49`, `anthropic.rs:48` — bare `Client::new()`, no `.timeout()` or `.connect_timeout()`
- **Impact:** A slow/hanging LLM endpoint blocks the entire agent loop indefinitely with no recovery.
- **Fix:** `Client::builder().timeout(120s).connect_timeout(10s)`. Trivial — one builder chain per provider.

### 3. TOCTOU race in agent_chat
- **Evidence:** `api.rs:642-670` — read lock → clone → release → check `is_running` → new write lock to set. Two concurrent POSTs to the same session both pass the check.
- **Impact:** Concurrent turns on the same session → data corruption.
- **Fix:** Atomic check-and-set: `set_running` returns `Result`, caller treats `Err` as 409.

## P1 — Significant reliability gaps

### 4. Error handling is systemic failure (55/100 avg)
- **Evidence:** `api.rs` 81.6% errors ignored, `engine.rs` 94.7%, `openai.rs` 100%, `anthropic.rs` 88.9%, `edit.rs` 100%, `web_fetch.rs` 75%
- **Impact:** Failures become invisible. `let _ = result.ok()` is the default pattern across every crate.
- **Fix:** Per-crate error enums (`thiserror`), replace 100+ silent drops with `?` propagation + `tracing::warn!`.

### 5. No retry on transient failures (F)
- **Evidence:** Zero retry code anywhere. No `tokio-retry` / `backon` dependency. One 429/5xx terminates the entire turn.
- **Fix:** Add retry layer with exponential backoff + jitter for 5xx and 429. Cap at 3 attempts.

### 6. Fire-and-forget tasks never cleaned up
- **Evidence:** `api.rs:721,756,773,799,832,858` — `tokio::spawn` with no `JoinHandle`, no cancellation.
- **Impact:** On cancel/shutdown, memory workers continue running, wasting API credits and system resources.
- **Fix:** Track task handles via `JoinSet`; abort on cancel/shutdown.

### 7. Sync I/O inside async locks
- **Evidence:** `session_manager.rs:268-284` — `std::fs::write` while holding async `RwLock` write guard. `search_index.rs:24` — `Mutex<Connection>` (std blocking).
- **Impact:** Under load, blocks all session/DB operations for the duration of disk I/O.
- **Fix:** `tokio::fs::write` or `spawn_blocking`. Connection pool for SQLite.

## P2 — Maintainability and architecture

### 8. api.rs god module (1380 lines, 6 responsibilities)
- **Evidence:** `api.rs` bundles HTTP routing + OpenAI-compat types + agent orchestration + memory-worker spawning + cycle management + raw HTTP LLM calls.
- **Fix:** Split into `handlers.rs` + `types.rs`; move 4 worker functions to `luwu-memory`.

### 9. Workers bypass LlmProvider trait
- **Evidence:** `api.rs:959,1008,1102,1174` — 4× raw `reqwest::Client` + hardcoded `"MiniMax-M3"` model.
- **Fix:** Route through `Arc<dyn LlmProvider>`. Make model configurable.

### 10. No connection pool reuse
- **Evidence:** `Client::new()` per request (providers), per call (workers), per fetch (web_fetch).
- **Fix:** One shared `reqwest::Client` in `AppState`, passed to all consumers.

### 11. Dead abstractions
- **Evidence:** `Storage` trait defined (`storage.rs:14`) but never implemented. `AnthropicProvider` fully written (`anthropic.rs`, 436 lines) but never wired in `agent_chat`.
- **Fix:** Implement or remove `Storage`. Wire provider selection through config.

## P3 — Polish

### 12. Config validation gaps
- **Evidence:** `config.rs:31` — no non-empty check on `api_key`. No URL format validation. No range check on `temperature`/`max_tokens`.
- **Fix:** Validate at `resolve()` time.

### 13. Sensitive data in errors
- **Evidence:** `openai.rs:96-101` — full LLM API response body included in error string, propagated to client.
- **Fix:** Truncate/sanitize before propagation.

### 14. Test duplication (34.5%)
- **Evidence:** `tests/test_e2e.py` — 20/58 functions duplicate provider adaptation logic.
- **Fix:** `pytest.mark.parametrize`, extract shared fixtures.
