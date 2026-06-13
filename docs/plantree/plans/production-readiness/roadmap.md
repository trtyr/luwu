# Roadmap — Production Readiness

Phased plan. Each phase is independently shippable — the project is better after each phase, not just at the end.

## Done

### Phase 1 — Runtime Safety Net (P0) — `ae21d5d`
Graceful shutdown, LLM client timeouts (120s/10s), TOCTOU atomic try_set_running, silent .ok() → warn! logging.

### Phase 2 — Error Handling Overhaul (P1) — `740d576`
Per-crate thiserror enums (LlmError/ToolError/ApiError), API response body truncation (500 chars), .unwrap() → .expect() cleanup, error sanitization in providers.

### Phase 3 — Retry & Resilience (P1) — `f02524f`
Shared reqwest::Client in AppState (connection pool), hand-rolled exponential backoff retry (429/5xx, 3 attempts, jitter), SSE 30s liveness timeout, JoinSet task tracking with abort on shutdown.

### Phase 4 — Concurrency & I/O (P1) — `42f7faa`
Sync std::fs → tokio::fs in session_manager, SQLite ops → spawn_blocking in search_index, RunningGuard RAII for is_running reset, config field validation (api_key/URL/temperature/max_tokens).

### Phase 5 — Architecture Split (P2) — `bf1249f`
1400-line api.rs god module → types.rs (153 lines) + app.rs (93 lines) + handlers/mod.rs. Storage dead trait removed.

### Phase 6 — Cleanup (P3) — (this commit)
Storage trait deleted, #[tracing::instrument] added to 5 key async functions (run_stream, call_llm, chat_completions, agent_chat, send_with_retry), re-review completed (C+ → B+).

## In Progress

(none)

## Next

### Phase 1 — Runtime Safety Net (P0)

Stop the bleeding. Three changes that prevent immediate production disasters.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 1.1 | Graceful shutdown with signal handling | `main.rs`, `api.rs` | Small | Ctrl-C / SIGTERM drains in-flight requests, logs shutdown, exits cleanly |
| 1.2 | Timeout on LLM HTTP clients | `openai.rs`, `anthropic.rs` | Trivial | `Client::builder().timeout(120s).connect_timeout(10s)` on all providers |
| 1.3 | TOCTOU race fix (atomic check-and-set) | `api.rs`, `session_manager.rs` | Small | Concurrent POST to same session → clean 409, no corruption |
| 1.4 | Replace silent `.ok()` with `warn!` logging | `api.rs` (memory workers) | Trivial | No silent error swallowing on memory worker paths |

**Deliverable:** Server survives Ctrl-C, LLM hang, and concurrent request without data loss or deadlock.
**Topic:** [topics/runtime-resilience.md](../topics/runtime-resilience.md), [topics/concurrency.md](../topics/concurrency.md)

### Phase 2 — Error Handling Overhaul (P1)

Make failures visible. Replace the default `.ok()` / `let _ =` pattern with proper error types.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 2.1 | Define per-crate error enums | All crates `error.rs` | Medium | `thiserror` enums: `ApiError`, `EngineError`, `LlmError`, `ToolError` |
| 2.2 | Audit + replace silent drops in api.rs | `api.rs` | Medium | Zero `let _ = ...ok()` on critical paths; `?` propagation everywhere |
| 2.3 | Audit + replace silent drops in engine.rs | `engine.rs` | Medium | `run_stream` propagates errors; no swallowed Result |
| 2.4 | Audit + replace silent drops in llm providers | `openai.rs`, `anthropic.rs`, `sse.rs` | Medium | All HTTP/JSON errors classified, not flattened to String |
| 2.5 | Audit + replace silent drops in tools | `web_fetch.rs`, `edit.rs`, `grep.rs` | Medium | Tool errors are `ToolError`, not bare `Option::None` |
| 2.6 | Sanitize error response bodies | `openai.rs`, `anthropic.rs` | Trivial | Truncate API response bodies in error strings |

**Deliverable:** fuck-u-code error_handling score from 55 → 80+. No `unwrap()` in production paths.
**Topic:** [topics/error-handling.md](../topics/error-handling.md)

### Phase 3 — Retry & Resilience (P1)

Survive transient failures.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 3.1 | Shared `reqwest::Client` in AppState | `main.rs`, `api.rs`, providers | Small | One client, configured with pool + timeout, passed to all consumers |
| 3.2 | Network error classification enum | `llm.rs` (core) or new `net.rs` | Medium | `RateLimited`, `ServerError(u16)`, `Timeout`, `AuthFailed`, `ConnectionError` |
| 3.3 | Retry layer for LLM API calls | `openai.rs`, `anthropic.rs` | Medium | Exponential backoff + jitter on 429/5xx. Cap 3 attempts. |
| 3.4 | SSE stream liveness timeout | `sse.rs`, `consume_stream` | Small | `stream.next()` wrapped in `timeout(30s)`. Stale stream → error. |
| 3.5 | Fire-and-forget task tracking | `api.rs` | Medium | `JoinSet` for workers; abort on cancel/shutdown |

**Deliverable:** Transient 429/5xx/network blip does not terminate the turn. Network resilience grade D → B-.
**Topic:** [topics/runtime-resilience.md](../topics/runtime-resilience.md)

### Phase 4 — Concurrency & I/O (P1)

Fix blocking patterns that hurt under load.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 4.1 | Move sync file I/O out of async locks | `session_manager.rs` | Small | `tokio::fs::write` or `spawn_blocking` for `persist_session` |
| 4.2 | SQLite ops via `spawn_blocking` | `search_index.rs` | Small | `Mutex<Connection>` ops don't block tokio worker threads |
| 4.3 | Session `is_running` reset guard | `api.rs` | Small | `Drop` guard or `finally` pattern ensures reset even on stream interruption |
| 4.4 | Config field validation | `config.rs` | Small | Non-empty api_key, URL format, temperature range, max_tokens > 0 at `resolve()` time |

**Deliverable:** No sync I/O in async hot paths. Config errors fail fast at startup.
**Topic:** [topics/concurrency.md](../topics/concurrency.md)

### Phase 5 — Architecture: Split api.rs (P2)

Break the god module. Biggest maintainability win.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 5.1 | Extract OpenAI-compat types to `types.rs` | `api.rs` → `types.rs` | Small | ChatRequest/Response/Chunk types in separate module |
| 5.2 | Extract handlers to `handlers/` | `api.rs` → `handlers/` | Medium | One handler file per route group |
| 5.3 | Move worker functions to `luwu-memory` | `api.rs` → `luwu-memory` | Medium | `run_observer_worker` etc. live in memory crate |
| 5.4 | Route workers through `LlmProvider` trait | workers + providers | Medium | No raw `reqwest` in workers; use injected `Arc<dyn LlmProvider>` |
| 5.5 | Wire `AnthropicProvider` via config | `api.rs` | Small | Provider selected by config, not hardcoded |

**Deliverable:** `api.rs` < 300 lines. Architecture grade B+ → A.
**Topic:** [topics/api-refactor.md](../topics/api-refactor.md)

### Phase 6 — Cleanup (P3)

Polish and dead-code removal.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| 6.1 | Decide on `Storage` trait: implement or remove | `storage.rs`, `session_manager.rs` | Small | Either used or deleted, no dead abstraction |
| 6.2 | Add `#[tracing::instrument]` to key async functions | `engine.rs`, `api.rs` | Small | Timing spans on LLM calls, tool execution |
| 6.3 | De-duplicate test_e2e.py | `tests/test_e2e.py` | Small | `pytest.mark.parametrize`, < 10% duplication |
| 6.4 | Re-run `/review` to verify grade improvement | — | — | Overall ≥ B+ |

**Deliverable:** Clean codebase. Review confirms target grades met.

## Deferred

- Runtime tool registration API (`POST /v1/tools`)
- SSE mid-stream partial result delivery
- Fallback provider on failure
- Circuit breaker pattern
- Dockerfile + CI pipeline
- Connection pool per-host limits tuning
