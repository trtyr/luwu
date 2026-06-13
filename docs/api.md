# luwu HTTP API 接口文档

> luwu-server 版本 v0.1.0  
> 服务地址：`http://127.0.0.1:51740`  
> CORS 已开启（`CorsLayer::permissive()`）

---

## 目录

| # | 端点 | 方法 | 说明 |
|---|---|---|---|
| 1 | `/health` | GET | 健康检查 |
| 2 | `/v1/models` | GET | 列出可用模型 |
| 3 | `/v1/chat/completions` | POST | OpenAI 兼容对话（支持 SSE 流式） |
| 4 | `/v1/sessions` | GET | 列出所有会话 |
| 5 | `/v1/sessions` | POST | 创建会话 |
| 6 | `/v1/sessions/{id}` | GET | 获取会话详情 |
| 7 | `/v1/sessions/{id}` | DELETE | 删除会话 |
| 8 | `/v1/sessions/{id}/chat` | POST | Agent 事件流（核心交互接口） |
| 9 | `/v1/sessions/{id}/cancel` | POST | 取消正在运行的 Agent |
| 10 | `/v1/sessions/{id}/checkpoint` | GET | 获取会话检查点 |
| 11 | `/v1/sessions/{id}/history` | GET | 获取会话历史记录 |
| 12 | `/v1/skills` | GET | 列出已加载的 Skill |
| 13 | `/v1/skills/{name}` | GET | 获取 Skill 详情 |

---

## 1. 健康检查

```
GET /health
```

**响应** `200 OK`

```json
"ok"
```

纯文本响应。用于负载均衡器健康探测。

---

## 2. 列出可用模型

```
GET /v1/models
```

**响应** `200 OK`

```json
{
  "object": "list",
  "data": [
    {
      "id": "glm-4.7",
      "object": "model",
      "created": 0,
      "owned_by": "zhipu"
    },
    {
      "id": "MiniMax-M3",
      "object": "model",
      "created": 0,
      "owned_by": "minimax"
    },
    {
      "id": "deepseek-v4-flash",
      "object": "model",
      "created": 0,
      "owned_by": "deepseek"
    }
  ]
}
```

返回 `~/.luwu/config.toml` 中配置的所有 provider 对应模型。

---

## 3. OpenAI 兼容对话

```
POST /v1/chat/completions
Content-Type: application/json
```

兼容 OpenAI Chat Completions API 格式。可用于任何 OpenAI SDK 客户端（如 `openai` Python 包）直接对接。

### 请求体

```json
{
  "model": "glm-4.7",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello"}
  ],
  "stream": true,
  "temperature": 0.7,
  "max_tokens": 4096
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `model` | string | 否 | **会被服务端忽略**，实际使用的模型由 config 决定 |
| `messages` | ChatMessage[] | 是 | 对话消息数组 |
| `stream` | boolean | 否 | `true` 返回 SSE 流，`false` 返回完整 JSON（默认 false） |
| `temperature` | number | 否 | 生成温度 |
| `max_tokens` | integer | 否 | 最大生成 token 数 |

### ChatMessage 结构

```json
{
  "role": "user | assistant | system | tool",
  "content": "消息内容（string 或 null）",
  "tool_calls": [],
  "tool_call_id": "call_xxx",
  "name": "bash"
}
```

> 仅 `role` 和 `content` 必填。`tool_calls`、`tool_call_id`、`name` 为可选字段，用于多轮 tool 调用场景。

### 非流式响应 `200 OK`

```json
{
  "id": "chatcmpl-xxxx",
  "object": "chat.completion",
  "created": 1718000000,
  "model": "glm-4.7",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "你好！有什么可以帮你的吗？"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 123,
    "completion_tokens": 45,
    "total_tokens": 168
  }
}
```

### 流式响应 `200 OK`（`Content-Type: text/event-stream`）

每个事件格式：

```
data: {"id":"chatcmpl-xxxx","object":"chat.completion.chunk","created":1718000000,"model":"glm-4.7","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxxx","object":"chat.completion.chunk","created":1718000000,"model":"glm-4.7","choices":[{"index":0,"delta":{"content":"你"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxxx","object":"chat.completion.chunk","created":1718000000,"model":"glm-4.7","choices":[{"index":0,"delta":{"content":"好"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxxx","object":"chat.completion.chunk","created":1718000000,"model":"glm-4.7","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

> **注意：** 此端点不触发 Agent 工具调用，仅做单轮 LLM 对话。需要 Agent 完整能力请用 `/v1/sessions/{id}/chat`。

---

## 4. 列出所有会话

```
GET /v1/sessions
```

**响应** `200 OK`

```json
{
  "sessions": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "model": "glm-4.7",
      "message_count": 3,
      "title": null,
      "created_at": "2024-06-13T12:00:00Z",
      "updated_at": "2024-06-13T12:05:00Z",
      "is_running": false
    }
  ]
}
```

---

## 5. 创建会话

```
POST /v1/sessions
Content-Type: application/json
```

### 请求体

```json
{
  "model": "glm-4.7",
  "provider": "zhipu"
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `model` | string | 否 | 指定模型，不填使用 config 默认模型 |
| `provider` | string | 否 | 指定 provider（`zhipu` / `minimax` / `deepseek`），不填使用默认 |

### 响应 `201 Created`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "model": "glm-4.7"
}
```

### 错误响应 `400 Bad Request`

```json
{
  "error": "Unknown provider: anthropic. Available: zhipu, minimax, deepseek"
}
```

---

## 6. 获取会话详情

```
GET /v1/sessions/{id}
```

### 路径参数

| 参数 | 说明 |
|---|---|
| `id` | 会话 UUID |

### 响应 `200 OK`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "model": "glm-4.7",
  "message_count": 3,
  "title": null,
  "created_at": "2024-06-13T12:00:00Z",
  "updated_at": "2024-06-13T12:05:00Z",
  "is_running": false
}
```

### 错误响应 `404 Not Found`

```
Session not found
```

---

## 7. 删除会话

```
DELETE /v1/sessions/{id}
```

### 响应 `200 OK`

```json
{
  "status": "deleted"
}
```

### 错误响应 `404 Not Found`

```
Session not found
```

---

## 8. Agent 事件流（核心接口）

```
POST /v1/sessions/{id}/chat
Content-Type: application/json
```

**这是前端与 Agent 交互的核心接口。** 向会话发送用户消息，Agent 执行完整的思考→工具调用→迭代循环，通过 SSE 实时推送所有事件。

### 请求体

```json
{
  "message": "帮我创建一个 hello world 程序",
  "stream": true
}
```

| 字段 | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `message` | string | 是 | — | 用户消息 |
| `stream` | boolean | 否 | `true` | 是否以 SSE 流返回事件 |

### 响应 `200 OK`（`Content-Type: text/event-stream`）

SSE 事件流。每个 `data:` 行是一个 JSON 对象，包含一个 TurnEvent。

### TurnEvent 类型一览

所有事件都有一个 `type` 字段用于区分：

#### 8.1 `text_delta` — LLM 文本增量

Agent 正在输出文字。前端应逐步追加到聊天区域。

```json
{
  "type": "text_delta",
  "delta": "好的，"
}
```

```json
{
  "type": "text_delta",
  "delta": "我来帮你创建。"
}
```

#### 8.2 `reasoning_delta` — 推理/思考增量

模型在"思考中"。支持思考模式的模型（GLM-4.7、DeepSeek）会先发送 reasoning，再发送正式回复。前端可选择折叠显示或隐藏。

```json
{
  "type": "reasoning_delta",
  "delta": "用户想要创建一个 hello world 程序..."
}
```

> 如果模型支持思考模式（如 GLM-4.7），`reasoning_delta` 事件会在 `text_delta` 之前到达。

#### 8.3 `tool_call` — LLM 请求调用工具

Agent 决定调用一个工具。

```json
{
  "type": "tool_call",
  "call_id": "call_abc123",
  "tool_name": "write",
  "arguments": {
    "path": "/tmp/hello.py",
    "content": "print('hello world')"
  }
}
```

| 字段 | 说明 |
|---|---|
| `call_id` | 工具调用的唯一 ID |
| `tool_name` | 工具名称（`bash` / `read` / `write` / `edit` / `grep` / `web_fetch`） |
| `arguments` | 工具参数（JSON 对象，因工具而异） |

#### 8.4 `tool_started` — 工具执行开始

```json
{
  "type": "tool_started",
  "call_id": "call_abc123",
  "tool_name": "write"
}
```

#### 8.5 `tool_completed` — 工具执行完成

```json
{
  "type": "tool_completed",
  "call_id": "call_abc123",
  "tool_name": "write",
  "output": "Created /tmp/hello.py (29 bytes)",
  "is_error": false
}
```

| 字段 | 说明 |
|---|---|
| `output` | 工具的输出文本（截断至 25KB） |
| `is_error` | 是否为错误输出 |

#### 8.6 `iteration_end` — 单次迭代完成

一次"LLM 调用 → 可选工具调用"的循环结束。Agent 可能进行多次迭代。

```json
{
  "type": "iteration_end",
  "iteration": 1,
  "tool_calls": 1
}
```

#### 8.7 `done` — 整个回合完成

Agent 完成了所有工作。

```json
{
  "type": "done",
  "assistant_text": "好的，我已经帮你在 /tmp/hello.py 创建了 hello world 程序。",
  "llm_calls": 2,
  "tool_calls": 1,
  "usage": {
    "prompt_tokens": 1234,
    "completion_tokens": 567,
    "total_tokens": 1801
  }
}
```

| 字段 | 说明 |
|---|---|
| `assistant_text` | Agent 最终的完整文本回复 |
| `llm_calls` | 本回合 LLM 调用次数 |
| `tool_calls` | 本回合工具调用次数 |
| `usage` | Token 用量统计 |

#### 8.8 `cancelled` — 用户取消

```json
{
  "type": "cancelled"
}
```

#### 8.9 `error` — 发生错误

```json
{
  "type": "error",
  "message": "LLM provider error: connection timeout"
}
```

### 错误响应

| 状态码 | 说明 |
|---|---|
| `404` | 会话不存在 |
| `409` | 会话已有 Agent 在运行（不能同时跑两个） |

```json
{
  "error": "Session is already running"
}
```

### SSE 事件流示例

下面是一个完整的 Agent 交互示例：用户让 Agent 写一个 hello world 文件。

```
data: {"type":"reasoning_delta","delta":"用户想创建一个 hello world 文件..."}

data: {"type":"tool_call","call_id":"call_001","tool_name":"write","arguments":{"path":"/tmp/hello.py","content":"print('hello world')"}}

data: {"type":"tool_started","call_id":"call_001","tool_name":"write"}

data: {"type":"tool_completed","call_id":"call_001","tool_name":"write","output":"Created /tmp/hello.py (24 bytes)","is_error":false}

data: {"type":"iteration_end","iteration":1,"tool_calls":1}

data: {"type":"text_delta","delta":"已经帮你在 /tmp/hello.py 创建好了！"}

data: {"type":"done","assistant_text":"已经帮你在 /tmp/hello.py 创建好了！","llm_calls":2,"tool_calls":1,"usage":{"prompt_tokens":1234,"completion_tokens":567,"total_tokens":1801}}
```

### 前端渲染建议

```
┌─ Agent 事件流前端渲染逻辑 ──────────────────────────────────┐
│                                                             │
│  reasoning_delta → 折叠显示 "思考中..." 或隐藏              │
│  tool_call       → 显示 "正在调用 write 工具"               │
│  tool_started    → 展示加载动画                              │
│  tool_completed  → 显示工具输出（可折叠）                    │
│  iteration_end   → 静默，不需要展示                         │
│  text_delta      → 逐字追加到聊天区域（打字机效果）          │
│  done            → 标记回复完成，显示 token 用量             │
│  cancelled       → 显示 "已取消"                             │
│  error           → 显示错误提示                              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 9. 取消正在运行的 Agent

```
POST /v1/sessions/{id}/cancel
```

### 响应 `200 OK`

```json
{
  "status": "cancelled"
}
```

> 如果会话没有正在运行的 Agent，仍然返回 200。

---

## 10. 获取会话检查点

```
GET /v1/sessions/{id}/checkpoint
```

返回长任务执行过程中的最新检查点（由 Writer 子 Agent 生成）。

### 响应 `200 OK`

```json
{
  "session_id": "550e8400-...",
  "checkpoint": {
    "current_intent": "实现用户认证模块",
    "next_action": "编写 JWT 验证中间件",
    "constraints": "不使用外部依赖",
    "task_tree": "未完成",
    "current_work": "正在实现 login handler",
    "files_involved": "src/auth.rs, src/middleware.rs",
    "cross_task_findings": "无",
    "errors_and_fixes": "无",
    "runtime_state": "LLM 调用 3 次，工具调用 5 次",
    "design_decisions": "使用 session-based 认证",
    "notes": "无"
  },
  "raw": "## 当前意图\n实现用户认证模块\n\n## 下一步动作\n..."
}
```

| 字段 | 说明 |
|---|---|
| `checkpoint` | 解析后的结构化检查点（11 个字段） |
| `raw` | 原始 Markdown 文本 |

### 错误响应 `404 Not Found`

短对话或未触发检查点阈值时返回。

```
No checkpoint found for this session
```

---

## 11. 获取会话历史记录

```
GET /v1/sessions/{id}/history?q=keyword&limit=20
```

### 查询参数

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `q` | string | 空 | 关键词搜索（空则返回最近记录） |
| `limit` | integer | 20 | 返回条数上限 |

### 响应 `200 OK`

```json
{
  "session_id": "550e8400-...",
  "entries": [
    {
      "timestamp": "2024-06-13T12:05:00Z",
      "role": "user",
      "content": "帮我创建一个 hello world"
    },
    {
      "timestamp": "2024-06-13T12:05:03Z",
      "role": "assistant",
      "content": "好的，我已经帮你创建了..."
    }
  ],
  "total": 42
}
```

---

## 12. 列出已加载的 Skill

```
GET /v1/skills
```

### 响应 `200 OK`

```json
{
  "skills": [
    {
      "name": "echo-test",
      "description": "A test skill that echoes back a message. Use when the user says \"test skill\"."
    },
    {
      "name": "deploy",
      "description": "Deploy project to staging or production. Use when user says \"deploy\"."
    }
  ]
}
```

Skill 来源：
- 全局目录 `~/.luwu/skills/`
- 项目目录 `<project>/.luwu/skills/`

项目级 Skill 优先级高于全局。

---

## 13. 获取 Skill 详情

```
GET /v1/skills/{name}
```

### 路径参数

| 参数 | 说明 |
|---|---|
| `name` | Skill 名称（小写字母 + 数字 + 连字符） |

### 响应 `200 OK`

```json
{
  "name": "echo-test",
  "description": "A test skill that echoes back a message.",
  "instructions": "# Echo Test Skill\n\nThis is a simple test skill...\n\n## Usage\n\n1. Use the `bash` tool to run...",
  "base_path": "/Users/user/.luwu/skills/echo-test",
  "files": [
    "SKILL.md"
  ]
}
```

| 字段 | 说明 |
|---|---|
| `instructions` | 完整的 SKILL.md 正文（不含 frontmatter） |
| `base_path` | Skill 目录在磁盘上的绝对路径 |
| `files` | Skill 目录下所有文件的相对路径列表 |

### 错误响应 `404 Not Found`

```
Skill not found
```

---

## 附录 A：错误码汇总

| HTTP 状态码 | 含义 | 出现场景 |
|---|---|---|
| `200` | 成功 | 绝大多数 GET 请求 |
| `201` | 已创建 | POST /v1/sessions |
| `400` | 请求错误 | 参数缺失、provider 不存在 |
| `404` | 未找到 | 会话/skill 不存在 |
| `409` | 冲突 | 会话已有 Agent 在运行 |
| `500` | 服务端错误 | LLM provider 故障、IO 错误 |

## 附录 B：工具参数参考

### bash

```json
{
  "command": "cargo test",
  "timeout": 30
}
```

### read

```json
{
  "path": "/path/to/file.rs",
  "offset": 10,
  "limit": 50
}
```

| 参数 | 说明 |
|---|---|
| `path` | 文件或目录路径 |
| `offset` | 起始行号（1-indexed，可选） |
| `limit` | 读取行数（可选） |

> 如果 `path` 是目录，返回目录列表。文件内容以 `行号:哈希|内容` 格式返回。

### write

```json
{
  "path": "/path/to/file.rs",
  "content": "fn main() { println!(\"hello\"); }"
}
```

### edit

文本替换模式：
```json
{
  "path": "/path/to/file.rs",
  "old_text": "println!(\"hello\")",
  "new_text": "println!(\"world\")"
}
```

锚点模式：
```json
{
  "path": "/path/to/file.rs",
  "anchor": "5:3a2",
  "new_text": "替换后的新内容"
}
```

> 三级匹配策略：Strict（精确匹配）→ Resilient（空白归一化）→ Fuzzy（仅建议不修改）

### grep

```json
{
  "query": "TurnEngine",
  "mode": "regex",
  "glob": "*.rs"
}
```

| 参数 | 说明 |
|---|---|
| `query` | 搜索内容（支持纯文本/正则/模糊） |
| `mode` | `plaintext` / `regex` / `fuzzy`（默认 plaintext） |
| `glob` | 文件过滤（如 `*.rs`，可选） |

### web_fetch

```json
{
  "url": "https://example.com",
  "format": "markdown",
  "max_chars": 50000,
  "timeout_ms": 15000
}
```

| 参数 | 说明 |
|---|---|
| `url` | 仅支持 http/https |
| `format` | `markdown` / `text` / `raw`（默认 markdown） |
| `max_chars` | 最大返回字符数（默认 50000） |
| `timeout_ms` | 超时毫秒（默认 15000） |

## 附录 C：Skill 文件格式

Skill 遵循 [Agent Skills 开放标准](https://agentskills.io/specification)。

### SKILL.md 格式

```markdown
---
name: my-skill
description: 描述这个 skill 做什么以及什么时候使用（最多 1024 字符）。
---

# My Skill

## Steps
1. 第一步
2. 第二步
```

### 命名规则

- 1-64 个字符
- 仅小写字母 `a-z`、数字 `0-9`、连字符 `-`
- 不能以连字符开头或结尾
- 不能有连续连字符

### 目录结构

```
my-skill/
├── SKILL.md          # 必需
├── scripts/          # 可选：脚本文件
├── references/       # 可选：参考文档
└── assets/           # 可选：其他资源
```

## 附录 D：配置文件参考

配置文件路径：`~/.luwu/config.toml`

```toml
[default]
provider = "zhipu"
model = "glm-4.7"

[providers.zhipu]
api_key = "your-api-key"
base_url = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-4.7"

[providers.minimax]
api_key = "your-api-key"
base_url = "https://api.minimaxi.com/v1"
model = "MiniMax-M3"

[providers.deepseek]
api_key = "your-api-key"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
```

| 字段 | 说明 |
|---|---|
| `default.provider` | 默认使用的 provider |
| `default.model` | 默认模型（被 provider 级别覆盖） |
| `providers.*.api_key` | API 密钥 |
| `providers.*.base_url` | API 基础 URL |
| `providers.*.model` | 该 provider 使用的模型 |
| `providers.*.temperature` | 可选，生成温度 |
| `providers.*.max_tokens` | 可选，最大生成 token 数 |
