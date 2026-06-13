# luwu TUI MVP Plan

## Scope
在现有 ui/ Ink TUI 基础上，借鉴 Claude Code 的核心交互模式，实现一个可用的 MVP。

## MVP 功能清单

| # | 功能 | 借鉴 Claude Code | 后端连接 | 状态 |
|---|---|---|---|---|
| M1 | 消息渲染（user/assistant/tool/error） | F2 消息分发模式 | POST /v1/sessions/{id}/chat SSE | ✅ 已有 |
| M2 | 输入框（Enter 提交，Ctrl+C 取消） | F3 PromptInput 简化版 | — | ✅ 已有 |
| M3 | 状态栏（model/cwd/git/context%） | F4 StatusLine | GET /v1/stats, GET /v1/models | ✅ 已有 |
| M4 | 主题配色 | F7 dark theme | — | ✅ 已有 |
| M5 | 历史输入浏览（↑↓） | F3 useArrowKeyHistory | — | ⬜ 待实现 |
| M6 | slash 命令（/help, /clear, /model） | F13 Commands 简化版 | GET /v1/models, GET /v1/skills | ⬜ 待实现 |
| M7 | 工具调用展示（折叠/展开） | F2 AssistantToolUseMessage | tool_call/tool_completed SSE events | ⬜ 待优化 |
| M8 | Markdown 渲染 | F6 Markdown | — | ⬜ 待实现 |
| M9 | 思考计时（2 秒最小显示） | F5 Spinner | — | ⬜ 待实现 |
| M10 | reasoning_delta 展示 | F2 AssistantThinkingMessage | reasoning_delta SSE event | ⬜ 待实现 |

## 借鉴策略

### 直接复刻
- 主题配色（已完成）
- 消息前缀符号（❯ ● ○）
- 状态栏结构

### 简化实现
- PromptInput: 不做 vim/plan/bash 模式，只做默认模式 + 历史浏览
- Markdown: 用 marked 解析 + 简化 token 映射，不做语法高亮
- Spinner: 用 ink-spinner，加动词 + 2 秒最小时间

### 不做（MVP 范围外）
- 虚拟滚动（消息量小，暂时不需要）
- 快捷键配置系统（用硬编码快捷键）
- 代码语法高亮（后续 P1）
- Diff 展示（后续 P2）
- 文件提及 @ 补全（后续 P2）

## 后端连接映射

| 前端功能 | 后端 API | 数据流 |
|---|---|---|
| 健康检查 | GET /health | init 时检查 |
| 创建会话 | POST /v1/sessions | init 时创建 |
| 发送消息 | POST /v1/sessions/{id}/chat | SSE stream → text_delta/reasoning_delta/tool_call/tool_completed/done |
| 取消 | POST /v1/sessions/{id}/cancel | Ctrl+C 时调用 |
| 模型列表 | GET /v1/models | /model 命令 |
| 技能列表 | GET /v1/skills | /skills 命令 |
| 统计 | GET /v1/stats | 状态栏 |

## Roadmap

### Phase MVP-1: 核心交互增强 (当前)
- [x] M1-M4 已完成
- [ ] M5 历史输入浏览
- [ ] M6 slash 命令
- [ ] M7 工具展示优化
- [ ] M8 Markdown 渲染
- [ ] M9 思考计时
- [ ] M10 reasoning 展示

### Phase MVP-2: 测试
- [ ] client.ts API 测试
- [ ] 组件渲染测试
- [ ] SSE 解析测试
- [ ] 输入处理测试

### Phase MVP-3: 后续增强
- 代码高亮
- 虚拟滚动
- 快捷键配置
- Diff 展示
