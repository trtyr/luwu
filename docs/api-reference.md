# luwu-server API Reference

Base URL: `http://127.0.0.1:51740`
CORS: permissive (all origins)
SSE keep-alive: 15s

---

## Health & Stats

### GET /health
Returns `"ok"` (plain text).

**Response**: `200 "ok"`

### GET /v1/stats
Runtime statistics.

**Response** `200:
```json
{
  "sessions": { "total": 338, "running": 0 },
  "workers": 0
}
```

### GET /v1/models
List available models from config.

**Response** `200:
```json
{
  "object": "list",
  "data": [{ "id": "glm-4.7", "object": "model", "created": 0, "owned_by": "zhipu" }]
}
```

---

## Chat Completions (OpenAI-compatible)

### POST /v1/chat/completions
OpenAI-compatible streaming/non-streaming chat.

**Request**:
```json
{
  "model": "glm-4.7",           // ignored, uses config default
  "messages": [{ "role": "user", "content": "hello" }],
  "stream": true,                // optional, default false
  "temperature": 0.7,            // optional
  "max_tokens": 4096,            // optional
  "tools": [],                   // optional JSON schema
  "session_id": "abc-123"        // optional, appends to session
}
```

**Streaming response** (SSE):
```
data: {"id":"...","object":"chat.completion.chunk","created":0,"model":"glm-4.7","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}
data: {"id":"...","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}
data: {"id":"...","choices":[{"index":0,"delta":{"content":"！"},"finish_reason":null}]}
data: {"id":"...","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
data: [DONE]
```

**Non-streaming response** `200:
```json
{
  "id": "...",
  "object": "chat.completion",
  "created": 0,
  "model": "glm-4.7",
  "choices": [{ "index": 0, "message": { "role": "assistant", "content": "你好！" }, "finish_reason": "stop" }],
  "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
}
```

---

## Sessions

### GET /v1/sessions
List all sessions.

**Response** `200:
```json
{
  "sessions": [{
    "id": "6da875c9-...",
    "model": "glm-4.7",
    "message_count": 3,
    "title": null,
    "created_at": "2026-06-13T14:26:09Z",
    "updated_at": "2026-06-13T14:30:00Z",
    "is_running": false
  }]
}
```

### POST /v1/sessions
Create a new session.

**Request**:
```json
{ "model": "glm-4.7", "provider": "zhipu" }
```
Both fields optional — defaults from config.

**Response** `201:
```json
{ "id": "6da875c9-34f8-4706-8a31-530dd2509d91", "model": "glm-4.7" }
```

### GET /v1/sessions/{id}
Get session details (message history).

**Response** `200: SessionData with messages array.
**Error** `404: session not found.

### DELETE /v1/sessions/{id}
Delete a session.

**Response** `200: deleted.
**Error** `404: not found.

---

## Agent Chat (Session-scoped SSE)

### POST /v1/sessions/{id}/chat
Full agent chat with tool use, memory, cycle management.

**Request**:
```json
{ "message": "帮我看看这个文件", "stream": true }
```
`stream` defaults to `true`.

**Response** (SSE stream of TurnEvent):

| event type | fields | description |
|---|---|---|
| `text_delta` | `delta: string` | LLM text output chunk |
| `reasoning_delta` | `delta: string` | thinking/reasoning chunk |
| `tool_call` | `call_id, tool_name, arguments` | LLM requests a tool call |
| `tool_started` | `call_id, tool_name` | tool execution started |
| `tool_completed` | `call_id, tool_name, output, is_error` | tool finished |
| `iteration_end` | `iteration: u32, tool_calls: u32` | one agentic loop done |
| `done` | `assistant_text, llm_calls, tool_calls, usage` | turn complete |
| `cancelled` | — | user cancelled |
| `error` | `message: string` | error occurred |

Plus injected events from cycle management:
| event type | description |
|---|---|
| `checkpoint` | context checkpoint saved |
| `consolidation` | memory consolidation ran |
| `rebuild` | context rebuilt |

**Status codes**:
- `404`: session not found
- `409`: session is already running (concurrent request)

SSE format: `data: {"type":"text_delta","delta":"你好"}\n\n`

### POST /v1/sessions/{id}/cancel
Cancel an in-progress agent turn.

**Response** `200: `{ "status": "cancelled" }`

---

## Memory

### GET /v1/sessions/{id}/checkpoint
Get the latest context checkpoint for a session.

**Response** `200: checkpoint JSON (11 fields: current_intent, next_action, etc.)

### GET /v1/sessions/{id}/history?q=&limit=
Search session history with optional query.

**Query params**: `q` (search text), `limit` (max results, default 10)

**Response** `200: array of history entries.

---

## Skills

### GET /v1/skills
List all discovered skills.

**Response** `200:
```json
[{ "name": "...", "description": "...", "triggers": [...] }]
```

### GET /v1/skills/{name}
Get detail for a specific skill.

**Response** `200: skill detail JSON.
**Error** `404: skill not found.
