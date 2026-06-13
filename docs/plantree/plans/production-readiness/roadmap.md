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
LlmProvider::complete() non-streaming method added. All 3 memory workers (consolidation/observer/reflector) now route through Arc<dyn LlmProvider> + model: String. Zero hardcoded "MiniMax-M3" strings. Zero raw reqwest in workers. run_checkpoint_writer deleted (dead code). workers.rs 307 → 199 lines (-35%).

### Deep Overhaul A3 — Error Cleanup — `0ae242c`
memory_search.rs 3x .unwrap() → .expect(). Dead code warnings resolved (anthropic.rs unused import removed, error.rs documented + #![allow(dead_code)], store.rs root field annotated). **0 errors, 0 warnings** — first clean build ever.

## In Progress

(none)

## Next

### B1 — Service Layer Extraction (P2)

agent.rs is 332 lines — the only "fat" handler left. Business logic (cycle management, memory worker orchestration, correction detection, message persistence) lives inside the HTTP handler. Extract to a service layer so handlers are thin (extract request → call service → format SSE response).

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| B1.1 | Extract agent business logic to `services/agent_service.rs` | `handlers/agent.rs` → new `services/` | Medium | agent.rs < 150 lines, service holds TurnEngine + memory orchestration |
| B1.2 | Extract chat business logic to `services/chat_service.rs` | `handlers/chat.rs` → `services/` | Small | chat.rs < 100 lines, service holds provider + engine |
| B1.3 | Wire services through AppState | `app.rs`, `main.rs` | Small | Services constructed at startup, injected via state |

**Deliverable:** Handlers are thin transport layer. Business logic is testable without HTTP. Clean layer separation.

### B2 — Provider Factory (P2)

AnthropicProvider is fully implemented but never wired. Both chat.rs and agent.rs hardcode `OpenAiProvider`. Add a provider factory that selects by config — this is what the microkernel design was supposed to have.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| B2.1 | Provider factory in app.rs | `app.rs` | Small | `create_provider(config) -> Arc<dyn LlmProvider>` matching provider name to OpenAi/Anthropic |
| B2.2 | Wire factory into chat + agent handlers | `chat.rs`, `agent.rs` | Trivial | No hardcoded `OpenAiProvider` in handlers |

**Deliverable:** AnthropicProvider usable via config. Adding a new provider = implement trait + register in factory.

### B3 — CI Pipeline (P2)

No CI exists. Every build is manual. A GitHub Actions workflow ensures builds don't regress.

| # | Task | Files | Effort | Acceptance |
|---|------|-------|--------|------------|
| B3.1 | GitHub Actions workflow | `.github/workflows/ci.yml` | Small | cargo build + cargo test + cargo clippy on push/PR |
| B3.2 | Add clippy config | `clippy.toml` or workspace lint flags | Trivial | `cargo clippy` passes with zero warnings |

**Deliverable:** Push triggers CI. Merge requires green build.

## Deferred

- Runtime tool registration API (`POST /v1/tools`)
- SSE mid-stream partial result delivery
- Fallback provider on failure
- Circuit breaker pattern
- Connection pool per-host limits tuning

## Not Doing

- **Dockerfile** — user does not use Docker. No container packaging needed.
