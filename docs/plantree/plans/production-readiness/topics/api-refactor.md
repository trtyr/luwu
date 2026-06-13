# Topic: api.rs Refactor

## Problem

`api.rs` = **1380 lines, 6 responsibilities:**
1. HTTP routing (axum routes)
2. OpenAI-compatible types (ChatRequest, ChatResponse, ChatChunk, etc.)
3. Agent orchestration (agent_chat — 312 lines)
4. Memory worker spawning (4× raw HTTP LLM calls)
5. CycleState management
6. Raw HTTP LLM calls (bypasses LlmProvider trait)

## Target Structure

```
luwu-server/src/
├── main.rs          # entry, config, server setup (~100 lines)
├── config.rs        # config loading (existing, no change)
├── app.rs           # AppState, router builder, middleware
├── types.rs         # OpenAI-compat types (ChatRequest/Response/Chunk)
├── error.rs         # ApiError → IntoResponse
├── handlers/
│   ├── mod.rs       # re-exports
│   ├── health.rs    # GET /health
│   ├── models.rs    # GET /v1/models
│   ├── chat.rs      # POST /v1/chat/completions
│   ├── sessions.rs  # GET/POST/DELETE /v1/sessions
│   ├── agent.rs     # POST /v1/sessions/{id}/chat + cancel
│   └── skills.rs    # GET /v1/skills
└── coordinator.rs   # Agent turn orchestration (extracted from agent_chat)
```

Worker functions move to `luwu-memory`:

```
luwu-memory/src/
├── ... (existing files)
└── workers/
    ├── mod.rs
    ├── observer.rs      # run_observer_worker
    ├── reflector.rs     # run_reflector_worker
    ├── checkpoint.rs     # run_checkpoint_writer
    └── consolidation.rs  # run_consolidation_writer
```

## Worker Trait Routing (Phase 5.4)

**Current:** 4 workers each construct `reqwest::Client` + hand-roll HTTP POST + hardcode `"MiniMax-M3"`.

**Fix:** Workers accept `Arc<dyn LlmProvider>` + config-resolved model name:

```rust
// Before (api.rs:1102-1118)
let client = reqwest::Client::new();
let resp = client.post(&format!("{}/chat/completions", base_url))
    .bearer_auth(&api_key)
    .json(&json!({"model": "MiniMax-M3", ...}))
    .send().await?;

// After (luwu-memory/src/workers/observer.rs)
pub async fn run_observer(
    provider: &dyn LlmProvider,
    model: &str,
    prompt: &str,
) -> Result<String, LlmError> {
    let req = LlmRequest { model: model.into(), messages: vec![...], .. };
    // Use provider.stream() or a new provider.complete() method
    provider.complete(req).await
}
```

## Provider Selection (Phase 5.5)

**Current:** `agent_chat` hardcodes `OpenAiProvider::with_base_url(...)` at `api.rs:682-683`.

**Fix:** Resolve provider from config:

```rust
fn build_provider(config: &ResolvedConfig) -> Arc<dyn LlmProvider> {
    match config.default_provider_type.as_str() {
        "openai" => Arc::new(OpenAiProvider::with_base_url(...)),
        "anthropic" => Arc::new(AnthropicProvider::with_base_url(...)),
        other => panic!("Unknown provider type: {other}"),
    }
}
```

Stored in `AppState`, reused across requests.

## Migration Strategy

1. **Types first** (5.1) — pure move, zero logic change. Extract types to `types.rs`.
2. **Error module** (from Phase 2) — `error.rs` with `ApiError`.
3. **Handlers** (5.2) — split route groups into files. Each handler imports from `types.rs` and `error.rs`.
4. **Coordinator** — extract the agent_chat orchestration into `coordinator.rs`.
5. **Workers** (5.3 + 5.4) — move functions to `luwu-memory`, route through trait.

Each step compiles and passes tests independently.

## Constraints

- Don't change HTTP API behavior — routes, request/response formats, status codes stay identical.
- `agent_chat` is 312 lines — the extraction must preserve the exact SSE event ordering.
- Worker refactor (5.4) depends on Phase 2 error types and Phase 3.1 shared client.
- This is the riskiest phase — needs manual E2E testing after each step.

## Dependencies

- Phase 2 (error types needed for `ApiError`)
- Phase 3.1 (shared client needed for workers)
- [Q4](../open-questions.md#q4-should-apirs-split-happen-before-or-after-error-handling-overhaul) — ordering decision
