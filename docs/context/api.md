# Luwu HTTP API Reference

**Base URL:** `http://127.0.0.1:51740`
**Transport:** HTTP/1.1 with Server-Sent Events (SSE) for streaming endpoints
**CORS:** Permissive (all origins allowed)
**SSE Keep-Alive:** 15-second ping interval on all streaming connections

---

## Endpoint Overview

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Liveness probe |
| GET | `/v1/models` | List configured models |
| POST | `/v1/chat/completions` | OpenAI-compatible chat (SSE streaming or JSON) |
| GET | `/v1/sessions` | List all sessions |
| POST | `/v1/sessions` | Create a new session |
| GET | `/v1/sessions/{id}` | Get session metadata |
| DELETE | `/v1/sessions/{id}` | Delete a session |
| POST | `/v1/sessions/{id}/chat` | Agent chat with full event stream |
| POST | `/v1/sessions/{id}/cancel` | Cancel a running turn |
| GET | `/v1/sessions/{id}/checkpoint` | Get latest session checkpoint |
| GET | `/v1/sessions/{id}/history` | Search session history |
| GET | `/v1/skills` | List all loaded skills |
| GET | `/v1/skills/{name}` | Get skill detail |

---

## Health & Models

### GET `/health`

Returns `"ok"` (plain text). No parameters.

### GET `/v1/models`

Returns all models from the server's configuration.

**Response — `ModelsResponse`:**

| Field | Type | Description |
|-------|------|-------------|
| `object` | `string` | Always `"list"` |
| `data` | `ModelInfo[]` | One entry per configured provider model |

**`ModelInfo`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Model identifier (e.g. `"gpt-4o"`) |
| `object` | `string` | Always `"model"` |
| `created` | `i64` | Timestamp (always `0`) |
| `owned_by` | `string` | Provider name |

---

## OpenAI-Compatible Chat Completions

### POST `/v1/chat/completions`

Drop-in replacement for the OpenAI chat completions endpoint. The server ignores the `model` field in the request and always uses the configured default model.

**Request — `ChatRequest`:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `model` | `string?` | No | Ignored — server uses config default |
| `messages` | `ChatMessage[]` | Yes | Conversation messages |
| `stream` | `bool?` | No | `true` → SSE streaming (default `false`) |
| `temperature` | `f64?` | No | Accepted but currently unused |
| `max_tokens` | `u64?` | No | Accepted but currently unused |
| `tools` | `json?` | No | Accepted but currently unused |
| `session_id` | `string?` | No | If set, messages are appended to this session |

**`ChatMessage`:**

| Field | Type | Description |
|-------|------|-------------|
| `role` | `string` | `"user"`, `"assistant"`, `"system"`, or `"tool"` |
| `content` | `json?` | Message content (string or structured value) |
| `tool_calls` | `json?` | Tool call payloads (OpenAI format) |
| `tool_call_id` | `string?` | ID linking to a tool call |
| `name` | `string?` | Name for tool-role messages |

#### Streaming mode (`stream: true`)

Returns `Content-Type: text/event-stream`. Each SSE `data:` line is a JSON-encoded `ChatChunk`.

**`ChatChunk`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Completion ID (e.g. `"chatcmpl-<uuid>"`) |
| `object` | `string` | Always `"chat.completion.chunk"` |
| `created` | `i64` | Unix timestamp |
| `model` | `string` | Model name |
| `choices` | `ChatChunkChoice[]` | Always exactly one element |

**`ChatChunkChoice`:**

| Field | Type | Description |
|-------|------|-------------|
| `index` | `u32` | Always `0` |
| `delta` | `ChatChunkDelta` | Incremental content |
| `finish_reason` | `string?` | `null` during streaming, `"stop"` / `"cancel"` at end |

**`ChatChunkDelta`:**

| Field | Type | Description |
|-------|------|-------------|
| `role` | `string?` | Set to `"assistant"` on first chunk, `null` after |
| `content` | `string?` | Text delta |

**Stream lifecycle:**

1. First chunk: `delta.role = "assistant"`, `delta.content = null`
2. Content chunks: `delta.content` carries text deltas
3. Final chunk: `delta.content = null`, `finish_reason = "stop"` (or `"cancel"`)
4. Terminal line: `data: [DONE]`

Tool-call and reasoning events are **not forwarded** on this endpoint — use `/v1/sessions/{id}/chat` for full event visibility.

#### Non-streaming mode (`stream: false` or omitted)

**Response — `ChatResponse`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Completion ID |
| `object` | `string` | `"chat.completion"` |
| `created` | `i64` | Unix timestamp |
| `model` | `string` | Model name |
| `choices` | `ChatChoice[]` | Always one element |
| `usage` | `ChatUsage` | Token usage (currently all zeros) |

**`ChatChoice`:**

| Field | Type | Description |
|-------|------|-------------|
| `index` | `u32` | Always `0` |
| `message` | `ChatResponseMessage` | Full message |
| `finish_reason` | `string?` | `"stop"` |

**`ChatResponseMessage`:**

| Field | Type | Description |
|-------|------|-------------|
| `role` | `string` | `"assistant"` |
| `content` | `string?` | Complete assistant text |
| `tool_calls` | `json?` | Omitted in current implementation |

**`ChatUsage`:**

| Field | Type | Description |
|-------|------|-------------|
| `prompt_tokens` | `u32` | Input tokens |
| `completion_tokens` | `u32` | Output tokens |
| `total_tokens` | `u32` | Sum (currently always `0`) |

---

## Sessions

### GET `/v1/sessions`

**Response — `SessionListResponse`:**

| Field | Type | Description |
|-------|------|-------------|
| `sessions` | `SessionSummary[]` | All known sessions |

### POST `/v1/sessions`

**Request — `CreateSessionRequest`:**

| Field | Type | Description |
|-------|------|-------------|
| `model` | `string?` | Override model; defaults to config model |
| `provider` | `string?` | Override provider name |

**Response — `CreateSessionResponse`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | New session UUID |
| `model` | `string` | Resolved model name |

### GET `/v1/sessions/{id}`

**Response — `SessionSummary`:**

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Session UUID |
| `model` | `string` | Model name |
| `message_count` | `usize` | Number of stored messages |
| `title` | `string?` | Session title (if set) |
| `created_at` | `DateTime<Utc>` | Creation timestamp |
| `updated_at` | `DateTime<Utc>` | Last update timestamp |
| `is_running` | `bool` | Whether a turn is in progress |

Returns `404` if session not found.

### DELETE `/v1/sessions/{id}`

Returns `200 "Deleted"` on success, `404` if not found.

---

## Agent Chat

### POST `/v1/sessions/{id}/chat`

Runs a full agent turn — the LLM can call tools iteratively, and every stage is streamed as discrete events. This endpoint is the primary interface for interactive agent sessions.

**Request — `AgentChatRequest`:**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `message` | `string` | — | User message for this turn |
| `stream` | `bool` | `true` | Whether to stream SSE events |

**Error responses:**

| Status | Condition |
|--------|-----------|
| `404` | Session not found |
| `409 Conflict` | Session already has a running turn |
| `500` | Provider/config resolution failure |

#### Streaming mode — TurnEvent stream

Returns `Content-Type: text/event-stream`. Each `data:` line is a JSON-encoded `TurnEvent` tagged by `type` field.

**`TurnEvent` variants (serde tag = `"type"`):**

| `type` | Fields | Description |
|--------|--------|-------------|
| `text_delta` | `delta: string` | LLM text output chunk |
| `reasoning_delta` | `delta: string` | Model reasoning/thinking content |
| `tool_call` | `call_id`, `tool_name`, `arguments` | LLM requests a tool call |
| `tool_started` | `call_id`, `tool_name` | Tool execution began |
| `tool_completed` | `call_id`, `tool_name`, `output`, `is_error` | Tool finished |
| `iteration_end` | `iteration: u32`, `tool_calls: u32` | One agent loop step done |
| `done` | `assistant_text`, `llm_calls`, `tool_calls`, `usage` | Turn complete |
| `cancelled` | — | Turn was cancelled |
| `error` | `message: string` | Fatal error |

**`usage` (LlmUsage) in `done` event:**

| Field | Type | Description |
|-------|------|-------------|
| `prompt_tokens` | `u64` | Input tokens consumed |
| `completion_tokens` | `u64` | Output tokens consumed |
| `total_tokens` | `u64` | Sum |

**Additional injected events** (not part of `TurnEvent`):

| `type` | Fields | Description |
|--------|--------|-------------|
| `checkpoint` | `trigger`, `count` / `cycle`, `usage_pct` | Memory checkpoint written |
| `consolidation` | `files: string[]` | Memory files consolidated |
| `rebuild` | `cycle: u32` | Memory cycle rebuilt |

#### Non-streaming mode (`stream: false`)

Returns a single SSE event containing a `ChatResponse` (same shape as the non-streaming chat completions response), followed by stream close.

---

## Cancel

### POST `/v1/sessions/{id}/cancel`

Interrupts the currently running turn for the given session.

**Success:** `200 {"status": "cancelled"}`
**Not found / not running:** `404`

---

## Memory

### GET `/v1/sessions/{id}/checkpoint`

Returns the latest persisted checkpoint for the session.

**Response (200):**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | `string` | Session UUID |
| `checkpoint` | `json?` | Structured checkpoint data |
| `raw` | `string` | Raw checkpoint markdown |

Returns `404` if no checkpoint exists.

### GET `/v1/sessions/{id}/history`

Searches or browses session history entries.

**Query parameters:**

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `q` | `string` | `""` | Search keyword; empty returns recent entries |
| `limit` | `usize` | `20` | Max entries to return |

**Response (with `q`):**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | `string` | Session UUID |
| `query` | `string` | Echoed search query |
| `entries` | `json[]` | Matching history entries |

**Response (without `q`):**

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | `string` | Session UUID |
| `entries` | `json[]` | Recent entries (newest first) |
| `total` | `usize` | Total entry count |

---

## Skills

### GET `/v1/skills`

**Response:**

| Field | Type | Description |
|-------|------|-------------|
| `skills` | `object[]` | Each: `{ name, description }` |

### GET `/v1/skills/{name}`

**Response (200):**

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string` | Skill name (lowercase, hyphens, 1–64 chars) |
| `description` | `string` | What the skill does |
| `instructions` | `string` | Full SKILL.md body |
| `base_path` | `string` | Absolute path to skill directory |
| `files` | `string[]` | Relative paths to all files in the skill |

Returns `404` if skill not found.

---

## Protocol Details

- **SSE format:** Standard `text/event-stream` — each event is `data: <json>\n\n`
- **Streaming endpoints** use `axum::response::sse::Sse` with a 15-second keep-alive ping
- **OpenAI compatibility:** `/v1/chat/completions` follows the OpenAI streaming protocol (role chunk → content deltas → finish chunk → `[DONE]`)
- **Agent streaming:** `/v1/sessions/{id}/chat` uses `TurnEvent` serialization with `serde(tag = "type")`, giving discriminated JSON objects like `{"type":"text_delta","delta":"hello"}`
- **Provider resolution:** All endpoints resolve the provider from the server config (`~/.luwu/config.toml`). Session-scoped endpoints fall back to the session's provider, then the config default.
- **Session state machine:** A session can have at most one running turn at a time — attempting a second concurrent chat returns `409 Conflict`
- **Session persistence:** Sessions are stored as JSON files under `~/.luwu/sessions/` and recovered on server restart
