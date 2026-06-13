# Luwu — Module Reference

Organized by functional area. Each section covers path, responsibility, public API, dependencies, and notable patterns.

---

## 1. Core Agent Framework — `luwu-core`

`crates/luwu-core/`

The foundation crate: defines every trait and type that other crates depend on. No provider, tool, or storage implementation lives here — it's all abstractions and the engine that orchestrates them.

**Internal dependencies:** None (this is the root).

### engine.rs — Turn Engine

The heart of the system. Runs the agentic loop: user input → LLM → [tool calls → execute → feed back → LLM] → done.

| Export | Kind | Notes |
|--------|------|-------|
| `TurnEngine` | struct | Holds `Arc<dyn LlmProvider>`, `ToolRegistry`, `SkillRegistry`, `EventBus`. Configurable max iterations (default 50). |
| `TurnEngine::run` | async fn | Non-streaming: collects full response into `TurnResult`. |
| `TurnEngine::run_stream` | async fn | Streaming: returns `mpsc::Receiver<TurnEvent>`. Emits text deltas, tool events, iteration markers in real time. |
| `TurnResult` | struct | Final result: messages, assistant text, llm_calls/tool_calls counts. |
| `CancelToken` | struct | `tokio::watch` backed cancellation. Cloned into session manager. |

**Dependencies:** llm, tool_registry, skill, event, message, session, prompt.

**Notable pattern:** `run_stream` spawns a `tokio::spawn` task that owns the entire loop lifecycle. Tool calls are accumulated from streaming deltas (`PendingToolCall`) before execution. Skill references in assistant text are detected and auto-injected.

### cycle.rs — Context Window Management

Tracks token budget per cycle and decides when to checkpoint (20/45/70%) or rebuild (90%). Also triggers checkpoints on tool-call count (default 15).

| Export | Kind | Notes |
|--------|------|-------|
| `CycleState` | struct | Token tracking, checkpoint thresholds, tool-call counter, enabled flag. |
| `CycleAction` | enum | `Continue` / `Checkpoint` / `Rebuild`. |

**Dependencies:** None (pure data + logic).

### event.rs — Event Bus

Pub/sub nervous system. Every lifecycle event (turn start, text delta, tool call) flows through here.

| Export | Kind | Notes |
|--------|------|-------|
| `EventBus` | struct | `tokio::broadcast` channel. Clone freely. |
| `Event` | enum | Internal events: session/turn lifecycle, LLM streaming, tool execution, errors. |
| `TurnEvent` | enum | Consumer-facing streaming events (serializable, sent over SSE). |
| `SessionId` / `TurnId` | newtypes | UUID-backed, `Display`, `Serialize`. |

**Dependencies:** llm (`LlmUsage`), tool (`ToolOutput`).

### llm.rs — LLM Provider Trait

The single abstraction through which luwu talks to any language model.

| Export | Kind | Notes |
|--------|------|-------|
| `LlmProvider` | trait | `name()`, `list_models()`, `stream(LlmRequest) → Receiver<Result<LlmEvent>>`. |
| `LlmRequest` | struct | model, messages, tools, system_prompt, temperature, max_tokens, stop_sequences. |
| `LlmEvent` | enum | `TextDelta`, `ReasoningDelta`, `ToolCallBegin`, `ToolCallDelta`, `Done(usage)`, `Error`. |
| `LlmUsage` / `ToolDefinition` | structs | Token counts / JSON Schema tool specs. |

**Dependencies:** message, error.

### tool.rs + tool_registry.rs — Tool System

| Export | Kind | Notes |
|--------|------|-------|
| `Tool` | trait | `name()`, `description()`, `parameters_schema()`, `execute(input, ctx)`. |
| `ToolOutput` | struct | `content: String`, `is_error: bool`. Constructors: `text()`, `error()`. |
| `ToolContext` | struct | `working_dir`, `session_id`. Passed to every tool execution. |
| `ToolRegistry` | struct | `Arc<HashMap>` backed. `register()`, `get()`, `definitions()`, `execute()`. Cloneable after all registrations. |

**Dependencies:** error, event, llm (`ToolDefinition`).

**Notable pattern:** `ToolRegistry` wraps tools in `Arc<HashMap>`, so it's cloneable for use across async tasks — but registration must happen before sharing (panics on `Arc::get_mut` failure).

### message.rs — Message Types

Provider-agnostic conversation model. Every LLM plugin translates between `Message` and its own wire format.

| Export | Kind | Notes |
|--------|------|-------|
| `Message` | struct | `role`, `content: Vec<ContentPart>`, `name`, `tool_call_id`. |
| `ContentPart` | enum | `Text`, `ToolCall { id, name, arguments }`, `ToolResult { id, content, is_error }`. |
| `Role` | enum | `System`, `User`, `Assistant`, `Tool`. |
| `Message::user/system/assistant/tool_result` | constructors | Convenience builders. |

**Dependencies:** None.

### session.rs + session_manager.rs — Session Lifecycle

| Export | Kind | Notes |
|--------|------|-------|
| `SessionData` | struct | Core session: id, timestamps, messages, model, provider. `push_message()`. |
| `SessionManager` | struct | Server-side map with file persistence (`~/.luwu/sessions/{id}.json`). CRUD + `set_running` / `cancel`. |
| `ManagedSession` | struct | `SessionData` + runtime state (`cancel_token`, `is_running`). Runtime fields never persisted. |
| `SessionSummary` | struct | Lightweight listing metadata. |

**Dependencies:** engine (`CancelToken`), message, session, event.

**Notable pattern:** `append_messages` performs append + disk write atomically inside a single write lock — eliminates read-modify-write race. On load, all sessions resume `is_running: false`.

### storage.rs — Storage Trait

| Export | Kind | Notes |
|--------|------|-------|
| `Storage` | trait | `save_session`, `load_session`, `list_sessions`, `delete_session`. |

**Dependencies:** error, event, session. Currently unused by server (SessionManager handles persistence directly).

### skill.rs — Skill System

Follows the [Agent Skills standard](https://agentskills.io). Progressive disclosure: Level 1 metadata always in context, Level 2 instructions on activation.

| Export | Kind | Notes |
|--------|------|-------|
| `Skill` / `SkillFrontmatter` | structs | Loaded SKILL.md: name, description, instructions, base_path. |
| `SkillRegistry` | struct | Discovers from `~/.luwu/skills/` + `<project>/.luwu/skills/`. `get()`, `list()`, `skill_metadata_prompt()`, `detect_skill_reference()`. |

**Dependencies:** error.

### prompt/ — System Prompts

| Export | Notes |
|--------|-------|
| `default_system_prompt()` | Base agent identity + tool usage guide. |
| `system_prompt_with_tools(names)` | Appends available tool list. |
| `system_prompt_with_tools_and_skills(names, skills)` | Appends tools + Level 1 skill metadata. |
| `writer_system_prompt()` | Chinese-language prompt for checkpoint Writer subagent (11-field structured state extraction). |

### error.rs — Unified Error

| Export | Notes |
|--------|-------|
| `LuwuError` | Variants: `Llm`, `Tool`, `Storage`, `Session`, `Config`, `Io`, `Serde`. |
| `Result<T>` | Alias for `Result<T, LuwuError>`. |

---

## 2. LLM Providers — `luwu-llm`

`crates/luwu-llm/`

Concrete `LlmProvider` implementations. Each translates `LlmRequest` → vendor wire format and vendor SSE → `LlmEvent`.

**Dependencies:** `luwu-core` (LlmProvider trait, Message, LlmEvent, etc.).

### openai.rs — OpenAI-Compatible Provider

| Export | Kind | Notes |
|--------|------|-------|
| `OpenAiProvider` | struct | Implements `LlmProvider`. `new(api_key)` or `with_base_url(api_key, base_url)` for Ollama/vLLM. |

Supports streaming via Chat Completions API, function calling, `reasoning_content` (GLM/DeepSeek/MiniMax). Handles `[DONE]` termination.

### anthropic.rs — Anthropic Provider

| Export | Kind | Notes |
|--------|------|-------|
| `AnthropicProvider` | struct | Implements `LlmProvider`. `new(api_key)` or `with_base_url(api_key, base_url)`. |

Translates to Anthropic Messages API. Handles `content_block_start/delta/stop`, `message_delta` usage. Tool use via `input_json_delta`.

### sse.rs — Shared SSE Parser

| Export | Kind | Notes |
|--------|------|-------|
| `parse_sse_stream(response)` | fn | `reqwest::Response` → `impl Stream<Item=Result<SseEvent>>`. Buffer-based, handles partial chunks. |
| `SseEvent` | struct | `data: String`, `event_type: Option<String>`. |

Both providers call this before parsing vendor-specific JSON. Filters `[DONE]` and comment lines.

---

## 3. Tool Implementations — `luwu-tools`

`crates/luwu-tools/src/`

Every tool implements `luwu_core::Tool`. Registered via `all_builtin_tools()`.

| File | Tool Name | Responsibility |
|------|-----------|----------------|
| `bash.rs` | bash | Execute shell commands (build, test, git, pkg managers). |
| `read.rs` | read | Read file contents / list directories. Output includes `LINE:HASH` anchors. |
| `write.rs` | write | Create or completely overwrite files. |
| `edit.rs` | edit | Precise text replacement (old_text match or anchor mode). |
| `grep.rs` | grep | Search file contents across project. |
| `web_fetch.rs` | web_fetch | Fetch web pages, extract readable content (markdown). |
| `memory_search.rs` | memory_search | Search persistent memory (global/project/corrections/notes/checkpoint). |
| `hashline.rs` | — | Hash-line anchor generation utility (supports `read`/`edit`). |

**Dependencies:** `luwu-core` (Tool trait, ToolContext, ToolOutput), `luwu-memory` (for `memory_search`).

**Notable pattern:** `all_builtin_tools() → Vec<Box<dyn Tool>>` returns a ready-made vector — server just iterates and registers.

---

## 4. HTTP Server — `luwu-server`

`crates/luwu-server/src/`

Axum-based HTTP server. Two API surfaces: OpenAI-compatible `/v1/chat/completions` and full agent `/v1/sessions/{id}/chat` with tool visibility + memory workers.

**Dependencies:** `luwu-core` (everything), `luwu-llm` (OpenAiProvider), `luwu-tools` (all_builtin_tools), `luwu-memory` (MemoryStore + workers).

### main.rs — Server Entry

Loads config (`~/.luwu/config.toml`), initializes `SessionManager` with file persistence, discovers skills, builds `AppState`, binds to `127.0.0.1:51740`.

### config.rs — Configuration

| Export | Kind | Notes |
|--------|------|-------|
| `Config` | struct | TOML config: `default` provider/model + `providers` map. `load()`, `resolve(name?) → ResolvedConfig`. |
| `ResolvedConfig` | struct | Flattened: provider_name, api_key, base_url, model, temperature, max_tokens. |

### api.rs — HTTP Handlers + Agent Integration

The largest file (~1380 lines). Contains all route handlers, OpenAI-compatible request/response types, and the **memory worker orchestration** that runs during agent turns.

Key endpoints:

| Route | Method | Purpose |
|-------|--------|---------|
| `/health` | GET | Health check. |
| `/v1/models` | GET | List configured models. |
| `/v1/chat/completions` | POST | OpenAI-compatible chat (streaming SSE). |
| `/v1/sessions` | GET/POST | List / create sessions. |
| `/v1/sessions/{id}` | GET/DELETE | Get / delete session. |
| `/v1/sessions/{id}/chat` | POST | Full agent turn with tool visibility + cycle management. |
| `/v1/sessions/{id}/cancel` | POST | Cancel running turn. |
| `/v1/sessions/{id}/checkpoint` | GET | Get latest memory checkpoint. |
| `/v1/sessions/{id}/history` | GET | Search session history (`?q=keyword`). |
| `/v1/skills` | GET | List loaded skills. |
| `/v1/skills/{name}` | GET | Get skill detail. |

**Notable patterns:**
- `agent_chat` is the integration hub: builds `TurnEngine`, `MemoryStore`, `CycleState`, then streams events while interleaving memory operations (deterministic compaction, Observer worker spawns, consolidation checks, correction detection).
- Memory workers (`run_observer_worker`, `run_reflector_worker`, `run_consolidation_writer`) are `tokio::spawn`ed concurrently with the main agent loop.
- OpenAI-compat endpoint forwards reasoning deltas as content to maintain compatibility.

---

## 5. Memory System — `luwu-memory`

`crates/luwu-memory/src/`

Four-layer persistent memory: Global → Project → Session (checkpoint) → History (JSONL). Plus three memory workers (Observer/Reflector/Dropper) and zero-LLM deterministic compaction.

**Dependencies:** `luwu-core` (Message type only).

### store.rs — MemoryStore (Central Hub)

File-system backed. Paths derived from `luwu_home` + project hash + session ID.

| Method Group | Key APIs | Storage |
|--------------|----------|---------|
| Global memory | `read_global`, `write_global`, `append_global_entry` | `~/.luwu/memory/global.md` |
| Project memory | `read_project`, `write_project`, `append_project_entry` | `~/.luwu/memory/{hash}/project.md` |
| Session checkpoint | `read_checkpoint`, `write_checkpoint`, `write_checkpoint_raw` | `.../sessions/{id}/checkpoint.md` |
| Notes | `append_notes`, `read_notes`, `clear_notes` | `.../sessions/{id}/notes.md` |
| History | `history_log`, `append_history`, `search_history` | `.../sessions/{id}/history.jsonl` |
| Corrections | `read_corrections`, `append_correction` | `~/.luwu/memory/corrections.md` |
| Session ledger | `append_observation`, `append_reflection`, `read_*`, `drop_observations`, `render_ledger` | `observations.jsonl` / `reflections.jsonl` |
| Rebuild | `build_rebuild_context` | Merges all layers into fenced `<luwu-memory-context>` block. |
| Search | `search_all`, `check_consolidation` | Cross-layer keyword search / size threshold check. |

**Notable pattern:** All memory entries use `§` delimiter for splitting. Aging metadata via HTML comments (`<!-- created: ..., ref: ... -->`). FTS5 search index opened with graceful degradation.

### Supporting Modules

| Module | Key Exports | Responsibility |
|--------|-------------|----------------|
| `checkpoint.rs` | `Checkpoint` | 11-field structured state snapshot (markdown serialization). |
| `history.rs` | `HistoryLog`, `HistoryEntry`, `TokenEstimator` | JSONL append-only conversation log with keyword search. |
| `consolidation.rs` | `ConsolidationChecker`, `ConsolidationConfig`, `apply_consolidation`, `consolidation_prompt` | Detect oversized memory files (>8K chars), trigger LLM-based merging. |
| `correction.rs` | `CorrectionDetector`, `CorrectionPattern` | Detect user corrections (strong/weak patterns) from message text. |
| `deterministic.rs` | `DeterministicSummary`, `FileChange`, `compile` | Zero-LLM structured summary extraction from messages (file changes, intents). |
| `workers.rs` | `Observation`, `Reflection`, `Priority`, `WorkerThresholds`, `*_prompt` | Three-layer memory worker types + system prompts. Observer extracts observations, Reflector synthesizes reflections, Dropper prunes. |
| `search_index.rs` | `SearchIndex`, `SearchResult` | SQLite FTS5 full-text index across memory layers. Opens with graceful degradation. |

---

## Cross-Crate Dependency Graph

```
luwu-server
  ├── luwu-core    (engine, session_manager, skill, event, cycle, prompt, ...)
  ├── luwu-llm     (OpenAiProvider, AnthropicProvider)
  ├── luwu-tools   (all_builtin_tools → bash, read, write, edit, grep, web_fetch, memory_search)
  └── luwu-memory  (MemoryStore, workers, consolidation, correction, deterministic)

luwu-tools → luwu-core (Tool trait)
           → luwu-memory (MemoryStore for memory_search tool)

luwu-llm → luwu-core (LlmProvider trait, Message, LlmEvent)

luwu-memory → luwu-core (Message type)
```

`luwu-core` is the universal dependency root. `luwu-server` is the only crate that depends on all four others simultaneously — it's the composition layer.
