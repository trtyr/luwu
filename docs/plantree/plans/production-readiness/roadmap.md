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

### Phase 6 — Cleanup (P3) — `911b0c8`
Storage trait deleted, #[tracing::instrument] added to 5 key async functions, re-review completed (C+ → B+).

### Deep Overhaul A1 — Handler Split — `667421b`
handlers/mod.rs 1156 lines → 7 per-feature modules (health/chat/sessions/agent/skills/memory_ops/workers). mod.rs is 27 lines of pure routing declarations.

### Deep Overhaul A2 — Worker Trait Routing — `d4f3a69`
LlmProvider::complete() non-streaming method added. All 3 memory workers (consolidation/observer/reflector) now route through Arc<dyn LlmProvider> + model: String. Zero hardcoded "MiniMax-M3" strings. Zero raw reqwest in workers. workers.rs 307 → 199 lines (-35%).

### Deep Overhaul A3 — Error Cleanup — `0ae242c`
memory_search.rs 3x .unwrap() → .expect(). Dead code warnings resolved. **0 errors, 0 warnings** — first clean build ever.

### B2 — Provider Factory — `8897c62`
Provider factory in app.rs: `create_provider(&ResolvedConfig, Client) → Arc<dyn LlmProvider>`. AnthropicProvider now wireable via config. Zero hardcoded OpenAiProvider in handlers. Adding new provider = impl trait + match arm.

### B3 — CI Pipeline — `4d53f70`
GitHub Actions workflow (build + test + clippy -D warnings + fmt check). 19 clippy warnings auto-fixed. Workspace-wide cargo fmt. **0 clippy warnings, fmt check passes.**

### B1 — Service Layer Extraction — `bf69115`
AgentService extracted from agent.rs handler: correction detection, engine execution, cycle management, memory worker dispatch, message persistence. Handler is now thin HTTP transport (327→167 lines, -49%). AgentEvent enum separates domain events from transport formatting. Both streaming + non-streaming paths preserved.

### C1+C2 — Unit Tests (core + llm) — `475b738`
Session_manager: 12 tests (CRUD, try_set_running concurrent race, RunningGuard RAII Drop, cancel, append_messages, persistence roundtrip). Tool_registry: 8 tests (register/get/execute/clone). Retry: 6 tests (is_retryable_status, exponential backoff schedule, cap, Retry-After, jitter bounds). Error: 7 tests (truncate_body, Display, From<LlmError>).

### C1 — Tool Registry Tests — `364f722`
ToolRegistry register/get/execute/clone with EchoTool mock. 8 tests covering empty registry, multiple register, duplicate replace, definitions, execute unknown→error, execute calls tool, clone shares Arc.

### C3 — Server Integration Tests — `8a1174b`
luwu-server now has lib.rs (binary+library dual target). 13 handler integration tests via axum oneshot: health/models/sessions CRUD lifecycle/skills/stats/404 paths. No real LLM calls — pure infrastructure paths.

### C4 — Stats Endpoint + Tracing — `7ad8db3`
GET /v1/stats returns {sessions:{total,running}, workers:{active}}. AgentService::new + run instrumented with tracing spans. Stats handler has 2 tests (empty counts, reflects created session). **Workspace total: 87 tests, 0 clippy warnings.**

## In Progress

(none)

## Next

(none — all planned phases and hardening complete)

## Deferred

- Runtime tool registration API (`POST /v1/tools`)
- SSE mid-stream partial result delivery
- Fallback provider on failure
- Circuit breaker pattern
- Connection pool per-host limits tuning

## Not Doing

- **Dockerfile** — user does not use Docker. No container packaging needed.
