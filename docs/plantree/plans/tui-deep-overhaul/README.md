# luwu TUI Plan Tree — Deep Overhaul

## 目标
按 Claude Code 深度分析报告，修正所有视觉错误 + 架构重构 + 补全功能

## Phase A: 架构重构（微内核分层）
A1 创建目录结构 + 迁移 core/（types + state + constants）
A2 迁移 services/api.ts（从 client.ts）
A3 迁移 theme/（从 theme.ts）
A4 创建 hooks/（useSession, useStream, useHistory, useCommands, useSuggestion）
A5 重写 App.tsx 为组合层

## Phase B: 视觉修正（P0 — 必须改）
B1 UserMessage: 背景色替代前缀
B2 AssistantMessage: ● 白色 + minWidth=2
B3 MessageResponse: ⎿ 缩进包装器
B4 消息间距: addMargin 控制

## Phase C: 功能实现（P1）
C1 PromptInput 边框（Box border）
C2 斜杠补全列表（SuggestionList 组件 + useSuggestion hook）
C3 命令系统注册（core/constants.ts 定义所有命令）
C4 Spinner 动词（Reading/Writing/Searching）

## Phase D: 测试（所有测试通过 = MVP）
D1 core/ 单元测试
D2 hooks/ 逻辑测试
D3 组件渲染测试
D4 集成测试

## 后端对接映射
| 前端组件 | 后端 API | SSE 事件 |
|---|---|---|
| useSession | POST /v1/sessions, GET /v1/sessions | — |
| useStream | POST /v1/sessions/{id}/chat | text_delta, reasoning_delta, tool_call, tool_completed, done |
| useCommands /stats | GET /v1/stats | — |
| useCommands /skills | GET /v1/skills | — |
| useCommands /sessions | GET /v1/sessions | — |
| useCommands /model | GET /v1/models | — |
| StatusLine | GET /v1/stats | iteration_end |
