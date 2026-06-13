# 陆吾 (Luwu) — Architecture

> Rust AI agent framework. Microkernel design: core defines traits, everything else is a plugin.

## System Overview

Luwu is a multi-crate Rust workspace implementing an LLM agent server. The core crate defines four trait boundaries (`LlmProvider`, `Tool`, `Storage`, `EventBus`). Concrete implementations live in separate crates. A single binary (`luwu-server`) wires everything together and exposes an HTTP API.

```
┌─────────────────────────────────────────────────────┐
│                   luwu-server                        │
│  axum HTTP API · :51740 · OpenAI-compatible + agent  │
├──────────┬──────────┬──────────┬────────────────────┤
│          │          │          │                     │
│  luwu-   │  luwu-   │  luwu-   │   luwu-memory       │
│  llm     │  tools   │  memory  │   (SQLite/rusqlite) │
│          │          │          │                     │
│ OpenAI   │ bash     │ checkpoint│  4-layer memory    │
│ Anthropic│ read     │ history  │  Global/Project/    │
│ (SSE)    │ write    │ store    │  Session/History    │
│          │ edit     │ search   │                     │
│          │ grep     │ workers  │  Observations       │
│          │ web_fetch│          │  Reflections        │
│          │ memory   │          │  Checkpoints        │
│          │ hashline │          │                     │
├──────────┴──────────┴──────────┴────────────────────┤
│                    luwu-core                         │
│  Traits: LlmProvider · Tool · Storage · EventBus    │
│  Engine: TurnEngine (agent loop)                     │
│  Types: Message · LlmRequest · LlmEvent · TurnEvent  │
│  State: SessionData · SessionManager · CycleState    │
│  Misc:  ToolRegistry · SkillRegistry · PromptBuilder │
└──────────────────────────────────────────────────────┘
```

## Crate Dependency Graph

```
                    luwu-core  (zero crate deps)
                   ╱    │    ╲
                  ╱     │     ╲
        luwu-llm ╱   luwu-memory ╲
                ╱          │      ╲
              ╱            │       luwu-tools
             ╱             │       ╱
        luwu-server ───────┴──────┘
```

| Crate | Depends On | Purpose |
|---|---|---|
| `luwu-core` | *(none)* | Traits, types, agent engine, session/state mgmt |
| `luwu-llm` | `luwu-core` | LLM provider implementations (OpenAI, Anthropic) |
| `luwu-memory` | `luwu-core` | SQLite memory store, checkpoints, history, workers |
| `luwu-tools` | `luwu-core`, `luwu-memory` | Built-in tools (bash, file ops, search, web, memory_search) |
| `luwu-server` | all four | HTTP API, config, wiring, startup |

`luwu-tools` → `luwu-memory` dependency exists because `memory_search` tool queries the memory store directly.

## Core Trait Boundaries

All four traits use `async_trait` and are `Send + Sync`. The core never imports a concrete implementation.

| Trait | File | Key Method | Returns |
|---|---|---|---|
| `LlmProvider` | `core/src/llm.rs:78` | `stream(req: LlmRequest)` | `mpsc::Receiver<Result<LlmEvent>>` |
| `Tool` | `core/src/tool.rs:56` | `execute(input: Value, ctx: ToolContext)` | `Result<ToolOutput>` |
| `Storage` | `core/src/storage.rs:14` | `save_session` / `load_session` / `list_sessions` / `delete_session` | `Result<…>` |
| `EventBus` | `core/src/event.rs:197` | `publish(Event)` / `subscribe()` | `broadcast::Receiver<Event>` |

### LlmEvent (LLM → Engine stream)

```
LlmEvent::TextDelta(String)
LlmEvent::ReasoningDelta(String)       // GLM-4.7, DeepSeek, MiniMax thinking
LlmEvent::ToolCallBegin { id, name }
LlmEvent::ToolCallDelta { id, delta }  // streamed JSON args
LlmEvent::Done(LlmUsage)
LlmEvent::Error(String)
```

### TurnEvent (Engine → HTTP SSE stream)

```
TurnEvent::TextDelta { delta }
TurnEvent::ReasoningDelta { delta }
TurnEvent::ToolCall { call_id, tool_name, arguments }
TurnEvent::ToolStarted { call_id, tool_name }
TurnEvent::ToolCompleted { call_id, tool_name, output, is_error }
TurnEvent::IterationEnd { iteration, tool_calls }
TurnEvent::Done { assistant_text, llm_calls, tool_calls, usage }
TurnEvent::Cancelled
TurnEvent::Error { message }
```

## Agent Loop (TurnEngine)

The agentic loop lives in `core/src/engine.rs`. Two entry points:

| Method | Mode | Caller |
|---|---|---|
| `run()` | Non-streaming, collects full result | (internal/tests) |
| `run_stream()` | Streaming via `mpsc::Receiver<TurnEvent>` | `chat_completions`, `agent_chat` |

### run_stream data flow

```
                    ┌─────────────┐
  user_message ───► │  build msgs  │
                    │  + system    │
                    │  prompt      │
                    └──────┬───────┘
                           │
                    ┌──────▼───────┐
              ┌────►│ provider.     │
              │     │ stream(req)   │
              │     └──────┬───────┘
              │            │ mpsc::Receiver<LlmEvent>
              │     ┌──────▼───────┐
              │     │ consume SSE  │──► TurnEvent::TextDelta ──► HTTP SSE
              │     │ stream       │──► TurnEvent::ToolCall   ──► HTTP SSE
              │     └──────┬───────┘
              │            │
              │     ┌──────▼───────┐
              │     │ tool calls?  │
              │     └──┬───────┬───┘
              │        │ No    │ Yes
              │        │       │
              │        │  ┌────▼─────────────┐
              │        │  │ execute each tool │──► TurnEvent::ToolCompleted
              │        │  │ append results    │
              │        │  └────┬──────────────┘
              └────────┘       │
                               │ (loop back, iteration++)
              ┌────────────────┘
              │
         iteration > max (50)?
              │ Yes
              ▼
     TurnEvent::Done ──► HTTP SSE close
```

**Key properties:**

- Max 50 iterations (configurable via `with_max_iterations`).
- `CancelToken` (tokio `watch` channel) checked at iteration boundary and during SSE consumption.
- Skill detection (`detect_skill_reference`) runs on user input — can inject skill content into context.
- System prompt built from tool names + skills via `system_prompt_with_tools_and_skills`.

## Cycle Management (Context Window)

`CycleState` (`core/src/cycle.rs`) manages long-running sessions that exceed context limits.

| Concept | Trigger | Action |
|---|---|---|
| **Checkpoint** | Token usage hits 20% / 45% / 70% of budget, or 15 tool calls | Writer subagent extracts structured snapshot |
| **Rebuild** | Token usage hits 90% of budget | Clear window, reconstruct context from persisted memory, start new cycle |

```
Token budget: 100K (default)
                │
  0% ──────────►│──────── 20% ──── 45% ──── 70% ──── 90% ──►
  (running)     │ Checkpoint   Chk      Chk      REBUILD
                │   ↓           ↓        ↓        ↓
                │ Writer extracts structured checkpoint
                │              ...cycle resets...
```

Checkpoints fire once per threshold (tracked in `triggered` vec). After rebuild, `reset_cycle()` increments cycle index and clears all counters.

## Memory System (luwu-memory)

SQLite-backed four-layer memory. Persists under `~/.luwu/`.

| Layer | Scope | Content |
|---|---|---|
| **Global** | Cross-project | User preferences, environment facts |
| **Project** | Per-project | Architecture decisions, project knowledge |
| **Session** | Per-session | Working state — 11 structured fields per checkpoint |
| **History** | Per-session | Full JSONL conversation log |

### Memory workers (three-layer)

| Worker | Role |
|---|---|
| **Observer** | Extracts observations from conversation |
| **Reflector** | Generates reflections from observations |
| **Dropper** | Prunes redundant/outdated entries |

### Deterministic compaction

`compile_summary()` — zero-LLM structured extraction from session data. Produces `DeterministicSummary` with file changes and key facts. Used during rebuild to reconstruct context without an extra LLM call.

## Server (luwu-server)

### Startup (`main.rs`)

```
Config::load() → SessionManager::with_persistence(~/.luwu/sessions)
              → SkillRegistry::discover(~/.luwu, cwd)
              → AppState { config, sessions, working_dir, skills }
              → router(state) → axum::serve(:51740)
```

### API surface

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | Liveness check |
| GET | `/v1/models` | List available LLM models |
| POST | `/v1/chat/completions` | OpenAI-compatible chat (SSE streaming) |
| GET | `/v1/sessions` | List all sessions |
| POST | `/v1/sessions` | Create session |
| GET | `/v1/sessions/{id}` | Get session detail |
| DELETE | `/v1/sessions/{id}` | Delete session |
| POST | `/v1/sessions/{id}/chat` | Agent chat (full TurnEvent stream + cycle mgmt) |
| POST | `/v1/sessions/{id}/cancel` | Cancel running turn |
| GET | `/v1/sessions/{id}/checkpoint` | Get latest checkpoint |
| GET | `/v1/sessions/{id}/history` | Search session history |
| GET | `/v1/skills` | List discovered skills |
| GET | `/v1/skills/{name}` | Get skill detail |

### Two chat paths

| Path | Engine | Streaming | Cycle Mgmt | Memory |
|---|---|---|---|---|
| `/v1/chat/completions` | `TurnEngine::run_stream` | OpenAI-format SSE chunks | No | No |
| `/v1/sessions/{id}/chat` | `TurnEngine::run_stream` | Full `TurnEvent` stream (tool visibility) | Yes (`CycleState`) | Yes (`MemoryStore`) |

The agent chat path builds `MemoryStore`, `CycleState`, and `TurnEngine` per-request from `AppState`.

### Per-request wiring (`agent_chat`)

```
session = sessions.get(id)
provider = OpenAiProvider::with_base_url(api_key, base_url)
tools    = builtin_tool_registry()        ← fresh ToolRegistry each call
events   = EventBus::new(256)
engine   = TurnEngine::new(provider, tools, skills, events, working_dir)
memory   = MemoryStore::new(~/.luwu, working_dir, session_id)
cycle    = CycleState::default()
```

> **Note:** Provider is always `OpenAiProvider` with configurable base URL. Anthropic provider is implemented but not wired in the current `agent_chat` handler.

## LLM Providers (luwu-llm)

| Provider | File | API | Streaming |
|---|---|---|---|
| `OpenAiProvider` | `llm/src/openai.rs` | OpenAI Responses API | SSE via shared `sse.rs` |
| `AnthropicProvider` | `llm/src/anthropic.rs` | Anthropic Messages API | SSE via shared `sse.rs` |

Both transform `LlmRequest` → provider-specific JSON, stream SSE responses back as `LlmEvent`. Any OpenAI-compatible endpoint (Ollama, vLLM) works through the OpenAI provider with a custom base URL.

## Skills System

`SkillRegistry` (`core/src/skill.rs`) discovers skill definitions from `~/.luwu/skills/` and `{cwd}/.luwu/skills/`. Skills carry frontmatter metadata and markdown instructions. `detect_skill_reference()` scans user input for skill triggers and injects relevant content into context.

## Key Design Decisions

| Decision | Rationale |
|---|---|
| **Trait-based core, no concrete deps** | Any LLM, tool, or storage can be swapped without touching the engine |
| **mpsc channels for LLM streaming** | Decouples provider task from consumer; provider owns the HTTP stream lifecycle |
| **broadcast channel for EventBus** | Multiple subscribers (logging, SSE, metrics) without coupling |
| **`run_stream` over `run`** | Server needs incremental SSE; `run` exists for non-streaming convenience |
| **Fresh `TurnEngine` per agent chat** | Avoids shared mutable state across concurrent sessions; each request isolated |
| **CycleState separate from SessionData** | Context management logic is orthogonal to conversation state |
| **SQLite for memory** | Single-file, zero-config, embedded — no external DB process |
| **Deterministic compaction (no LLM)** | Rebuild doesn't cost an extra API call; structured extraction only |
| **`max_iterations = 50`** | Prevents infinite tool loops; configurable per engine instance |
| **OpenAI-compatible API** | Drop-in for any client that speaks OpenAI; agent endpoints add tool visibility |

## Request lifecycle (end-to-end)

```
Client POST /v1/sessions/{id}/chat { message: "..." }
  │
  ├─ session = sessions.get(id)     ← in-memory + disk persisted
  ├─ session.is_running? → 409 if true
  ├─ sessions.set_running(id, true) → cancel_token
  │
  ├─ Build TurnEngine(provider, tools, skills, events, working_dir)
  ├─ Build MemoryStore + CycleState
  │
  ├─ engine.run_stream(session_id, model, messages, user_message, cancel)
  │    │
  │    ├─ tokio::spawn(async { ── agent loop ── })
  │    │
  │    └─ returns mpsc::Receiver<TurnEvent> immediately
  │
  ├─ axum Sse::new(async_stream { ── forward TurnEvents ── })
  │    │
  │    ├─ while rx.recv().await → SseEvent::data(json)
  │    ├─ TurnEvent::ToolCompleted → inject into stream
  │    ├─ TurnEvent::Done → break
  │    └─ drop → stream closes
  │
  └─ sessions.set_running(id, false)
     sessions.update_messages(id, new_messages)
```
