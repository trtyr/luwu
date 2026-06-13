# Luwu — Architecture Review

> Reviewed against source at HEAD. Evidence cites file:line from the actual codebase.
> No source files were modified. Scope: this project only.

---

## TL;DR

Luwu is a **well-structured microkernel**. The trait boundaries in `luwu-core` are clean, the dependency graph points the right way, and adding a new provider or tool is genuinely easy. The one structural weakness is **`api.rs`**: a 1,379-line file that mixes HTTP routing, memory-worker orchestration, and raw `reqwest` LLM calls — it's a god module wearing an axum handler's clothes. If that gets split, this is an A-tier codebase.

---

## 1. Separation of Concerns

| | |
|---|---|
| **Score** | **B** |
| **Evidence** | **Good:** `luwu-core` has zero `luwu-*` crate dependencies (`luwu-core/Cargo.toml:7-16` — only serde/tokio/uuid/etc.). The four trait boundaries are isolated: `LlmProvider` (`llm.rs:78`), `Tool` (`tool.rs:56`), `Storage` (`storage.rs:14`), `EventBus` (`event.rs:197`). Core never imports a concrete implementation. Each provider lives in its own crate. **Problem:** `api.rs` is 1,379 lines and bundles HTTP handlers + OpenAI-compat types + the entire memory-worker orchestration + four raw-HTTP LLM worker functions (`run_observer_worker` at `api.rs:1091`, `run_reflector_worker` at `api.rs:1158`, `run_checkpoint_writer` at `api.rs:1000`, `run_consolidation_writer` at `api.rs:950`). `agent_chat` (`api.rs:636-947`) is a single 310-line function that builds the engine, manages `CycleState`, spawns workers, and streams SSE — all at once. The memory workers are **luwu-server functions**, not `luwu-memory` functions. That's the wrong layer. |
| **Recommendation** | Extract `api.rs` into modules: `handlers.rs` (axum routes), `types.rs` (OpenAI-compat structs), and move the four worker functions into `luwu-memory` (or a `luwu-workers` crate). `agent_chat` should delegate to a coordinator struct, not inline everything. |

---

## 2. Dependency Direction

| | |
|---|---|
| **Score** | **B** |
| **Evidence** | **Correct edges:** `luwu-core` → none; `luwu-llm` → `luwu-core`; `luwu-memory` → `luwu-core`; `luwu-tools` → `luwu-core` + `luwu-memory`; `luwu-server` → all four (`luwu-server/Cargo.toml:7-10`). Dependencies always point toward the core — no back-edges, no cycles. This is textbook. **Violation:** The four worker functions bypass the `LlmProvider` trait entirely. They construct a `reqwest::Client` and hand-roll `POST {base_url}/chat/completions` (`api.rs:1102-1118`, `api.rs:1174-1190`, `api.rs:1020-1035`, `api.rs:959-976`). The `LlmProvider` abstraction exists precisely to avoid this — these calls should go through `Arc<dyn LlmProvider>`. **Dead abstraction:** `Storage` trait (`storage.rs:14`) is defined and exported (`lib.rs:41`) but never implemented by any crate. `SessionManager` does its own file persistence directly (`session_manager.rs`). |
| **Recommendation** | Route worker LLM calls through `LlmProvider` — pass the existing `provider_arc` into the workers instead of raw `api_key`/`base_url` strings. Either implement `Storage` or remove it if it's aspirational. |

---

## 3. Coupling

| | |
|---|---|
| **Score** | **C** |
| **Evidence** | **Low coupling (good):** Core traits are decoupled — `TurnEngine` holds `Arc<dyn LlmProvider>` and `ToolRegistry`, never a concrete type (`engine.rs:92-100`). `CycleState` is pure data + logic with zero dependencies. `EventBus` decouples via broadcast channel. **High coupling (problem):** `api.rs` imports 25+ symbols across all four crates (`api.rs:30-43`). `agent_chat` directly touches `SessionManager`, `Config`, `OpenAiProvider`, `ToolRegistry`, `EventBus`, `TurnEngine`, `MemoryStore`, `CycleState`, `CorrectionDetector`, and `compile_summary` — that's every crate in one function. **Copy-paste coupling:** The four worker functions share an identical reqwest pattern (build JSON body → POST → parse `choices[0].message.content`) repeated 4× with `api.rs`. Model name `"MiniMax-M3"` is hardcoded in 4 places (`api.rs:961, 1021, 1104, 1176`). **Horizontal coupling:** `luwu-tools → luwu-memory` (`luwu-tools/Cargo.toml:9`) exists because one tool (`memory_search`) needs `MemoryStore`. This couples the tools crate to the memory crate. |
| **Recommendation** | Extract a `LlmWorkerClient` that wraps the repeated HTTP pattern. Make the model configurable (read from config, not hardcoded). For `luwu-tools → luwu-memory`, either accept it (documented, single tool) or pass `MemoryStore` via `ToolContext` so tools don't need the compile-time dependency. |

---

## 4. Cohesion

| | |
|---|---|
| **Score** | **B** |
| **Evidence** | **High cohesion (good):** Each core module has a single clear responsibility — `engine.rs` (agent loop), `cycle.rs` (context window), `llm.rs` (provider trait), `tool.rs` (tool trait), `skill.rs` (skill system). `luwu-llm` cleanly separates `openai.rs` (436 lines) + `anthropic.rs` (436 lines) + `sse.rs` (shared parser). `luwu-memory` splits into `store.rs`, `checkpoint.rs`, `history.rs`, `consolidation.rs`, `workers.rs`, `deterministic.rs` — each with one job. **Low cohesion (problem):** `api.rs` is the offender. It handles: (1) axum routing, (2) OpenAI-compatible request/response types, (3) agent orchestration, (4) memory worker spawning, (5) cycle management, (6) raw HTTP LLM calls. Six responsibilities in one file. |
| **Recommendation** | Split `api.rs` along the six responsibilities. The OpenAI-compat types (`ChatRequest`, `ChatResponse`, `ChatChoice`, etc., `api.rs:71-160`) belong in a `types.rs`. The worker functions belong in `luwu-memory`. The SSE event-forwarding loop can stay in a handler but should delegate to a coordinator. |

---

## 5. Extensibility

| | |
|---|---|
| **Score** | **B** |
| **Evidence** | **Easy to extend (good):** New LLM provider → implement `LlmProvider` trait, register in `agent_chat`. New tool → implement `Tool` trait, add to `all_builtin_tools()` (`luwu-tools/src/lib.rs:25-35`). The trait system makes both genuinely one-file jobs. **Not extensible (problem):** `AnthropicProvider` is fully implemented (`anthropic.rs`, 436 lines) but **never wired** — `agent_chat` hardcodes `OpenAiProvider::with_base_url` (`api.rs:682-683`). Selecting Anthropic requires editing the handler. The `Storage` trait is a dead extension point — defined but unusable. Worker model is hardcoded to `"MiniMax-M3"` in 4 places — using a different model for observation/reflection/checkpoint requires editing 4 functions. **No tool plugin mechanism:** tools must be compiled into `luwu-tools`; there's no runtime registration API exposed via the server. |
| **Recommendation** | Wire provider selection through config — `agent_chat` should resolve `AnthropicProvider` when `provider == "anthropic"`. Pass worker model from config. Consider a `POST /v1/tools` endpoint for runtime tool registration. |

---

## 6. Design Patterns

| | |
|---|---|
| **Score** | **A** |
| **Evidence** | **Microkernel + Plugin** (`lib.rs:1-16`): Core defines traits, plugins implement. Executed correctly — no leaky abstractions in the trait layer. **Dependency Injection** (`engine.rs:104-119`): `TurnEngine::new` takes `Arc<dyn LlmProvider>`, `ToolRegistry`, `EventBus` as injected dependencies. Clean. **Observer** (`event.rs:197`): `EventBus` on `tokio::broadcast` allows multiple subscribers without coupling. Used appropriately for lifecycle events. **Strategy**: `LlmProvider`, `Tool`, `Storage` are strategy interfaces — each with focused method sets, not bloated god-traits. **Producer-Consumer** (`engine.rs:317`): `mpsc::channel(256)` decouples the streaming task from the consumer. **Factory** (`luwu-tools/src/lib.rs:25`): `all_builtin_tools() → Vec<Box<dyn Tool>>` — simple, appropriate. **No over-engineering:** No unnecessary abstraction layers, no trait hierarchies, no builder labyrinths. The patterns are practical, not academic. **Minor wart:** `Storage` trait is an unused abstraction — either premature or abandoned. `ToolRegistry::register` uses `Arc::get_mut().expect()` (`tool_registry.rs:34-35`) which panics at runtime instead of returning `Result`. |
| **Recommendation** | Replace the `expect()` panic with a `Result` return on `register()`. Decide on `Storage`: implement it or remove it. Everything else is solid. |

---

## Summary Table

| # | Criterion | Score | Key Strength | Key Weakness |
|---|-----------|-------|-------------|--------------|
| 1 | Separation of Concerns | **B** | Core crate is zero-dep, traits isolated | `api.rs` = god module (1,379 lines, 6 responsibilities) |
| 2 | Dependency Direction | **B** | All edges point toward core, no cycles | Workers bypass `LlmProvider` with raw reqwest; `Storage` is dead |
| 3 | Coupling | **C** | Core traits decoupled via `Arc<dyn Trait>` | `agent_chat` touches every crate; 4× copy-paste HTTP pattern; `luwu-tools → luwu-memory` |
| 4 | Cohesion | **B** | Each core/memory module has one job | `api.rs` mixes routing + types + orchestration + workers + raw HTTP |
| 5 | Extensibility | **B** | New provider/tool = implement one trait | Anthropic not wired; model hardcoded 4×; `Storage` unusable |
| 6 | Design Patterns | **A** | Microkernel/DI/Observer/Strategy all appropriate | `Storage` unused; `register()` panics instead of `Result` |

**Overall: B+.** The foundation is excellent. The trait system and crate boundaries are the strongest part. The weakness is concentrated in one file (`api.rs`) — fix that and the architecture is A-tier.

---

## Methodology

- **Context:** `docs/context/architecture.md`, `docs/context/modules.md`
- **Structural analysis:** `codegraph callers/callees/query` for `LlmProvider`, `Tool`, `EventBus`, `TurnEngine`, `CycleState`, `Storage`, `MemoryStore`, `agent_chat`
- **Source verification:** `engine.rs`, `llm.rs`, `tool.rs`, `tool_registry.rs`, `storage.rs`, `api.rs` (lines 1-160, 620-1019), `lib.rs` (all crates), `Cargo.toml` (all crates), `memory_search.rs`, `event.rs`
- **Comment scan:** searched for `TODO|FIXME|HACK|workaround` — no architecture-significant markers found (only benign string literals in `grep.rs` tool examples)
- No source files were modified.
