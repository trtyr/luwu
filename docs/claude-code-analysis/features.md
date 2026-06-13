# Claude Code TUI 功能深度分析

> Source: `claude-code-sourcemap/restored-src/src/`
> 分析目标：提取 luwu TUI 可借鉴的功能和设计模式

---

## 功能清单总览

| # | 功能 | 文件 | 复杂度 | MVP 需要 |
|---|---|---|---|---|
| F1 | REPL 主布局 | `screens/REPL.tsx` (875KB) | 极高 | ✅ 核心布局 |
| F2 | 消息系统 | `Message.tsx` + `messages/` 27 个子组件 | 高 | ✅ 简化版 |
| F3 | 输入框 | `PromptInput/PromptInput.tsx` (347KB) | 极高 | ✅ 简化版 |
| F4 | 底部状态栏 | `PromptInputFooter.tsx` + `StatusLine.tsx` | 中 | ✅ |
| F5 | 思考动画 | `Spinner.tsx` (86KB) | 高 | ✅ 简化版 |
| F6 | Markdown 渲染 | `Markdown.tsx` + `MarkdownTable.tsx` | 中 | ✅ |
| F7 | 主题系统 | `theme.ts` + `design-system/` | 中 | ✅ 已复刻 |
| F8 | 虚拟滚动列表 | `VirtualMessageList.tsx` (145KB) | 高 | ⬜ 后续 |
| F9 | 快捷键系统 | `keybindings/` 14 个文件 | 高 | ⬜ 后续 |
| F10 | 代码高亮 | `HighlightedCode.tsx` | 中 | ⬜ 后续 |
| F11 | Diff 展示 | `StructuredDiff.tsx` | 中 | ⬜ 后续 |
| F12 | 模型选择器 | `ModelPicker.tsx` | 低 | ⬜ 后续 |
| F13 | 命令系统 | `commands/` slash commands | 高 | ⬜ 后续 |
| F14 | 任务列表 | `TaskListV2.tsx` | 中 | ⬜ 后续 |
| F15 | 代码搜索 | `GlobalSearchDialog.tsx` | 高 | ⬜ 后续 |

---

## F1: REPL 主布局

**文件**: `screens/REPL.tsx` (875KB, 5006 行)

**功能**: 整个 TUI 的根组件，协调所有子系统。

**布局结构** (从上到下):
```
┌─────────────────────────────┐
│  消息列表 (VirtualMessageList) │  ← 可滚动，虚拟化
│  - user messages              │
│  - assistant messages         │
│  - tool results               │
│  - system messages            │
├─────────────────────────────┤
│  Spinner / Thinking 状态       │  ← 思考动画 + 计时
├─────────────────────────────┤
│  PromptInput (输入框)          │  ← 带 border
├─────────────────────────────┤
│  PromptInputFooter (状态栏)    │  ← model/cwd/context%/suggestions
└─────────────────────────────┘
```

**关键设计**:
- **useDeferredValue**: 用于消息列表渲染，避免流式输出时卡顿
- **createFileStateCacheWithSizeLimit**: 文件状态缓存，避免重复读取
- **QueryGuard**: 包装 LLM 请求，防止重复查询
- **consumeEarlyInput**: 在初始化完成前缓冲用户输入
- **MessageSelector**: 可选择/编辑历史消息
- **ScrollKeybindingHandler**: vim 风格的滚动键绑定

**luwu 可借鉴**:
- 整体布局结构（消息列表 + 输入 + 状态栏）
- `useDeferredValue` 优化流式渲染
- 思考计时（最小 2 秒显示）

---

## F2: 消息系统

**文件**: `Message.tsx` (627 行) + `messages/` 27 个子组件

**分发模式**:
```
Message.tsx (dispatcher)
  ├── UserTextMessage.tsx       → "❯ " + 文本
  ├── AssistantTextMessage.tsx  → "● " + Markdown 渲染
  ├── AssistantToolUseMessage   → 工具调用 UI (44KB)
  ├── AssistantThinkingMessage  → thinking 内容
  ├── SystemTextMessage         → 系统消息
  ├── AttachmentMessage         → 文件附件
  ├── GroupedToolUseContent     → 折叠连续工具调用
  ├── CollapsedReadSearchContent → 折叠 read/search 结果
  └── ... (19 more)
```

**关键设计**:
- **React Compiler**: 所有组件用 `_c(N)` 缓存（编译时优化）
- **buildMessageLookups**: 预计算消息关联数据
- **MessageResponse wrapper**: 统一消息间距和容器
- **CompactSummary**: 上下文压缩时显示摘要
- **ContextVisualization**: 74KB 组件可视化 context window 使用情况

**AssistantTextMessage** 特殊处理:
- 检测 rate limit / API key 错误 → 特殊 UI
- 检测 PROMPT_TOO_LONG → 提示 /compact
- 检测 INTERRUPT_MESSAGE → 中断提示
- 普通文本 → `<Markdown>` 渲染

**luwu 可借鉴**:
- 消息分发器模式（已有 MessageItem.tsx）
- 错误消息特殊处理（API key / rate limit / timeout）
- 工具结果折叠显示

---

## F3: 输入框 PromptInput

**文件**: `PromptInput/PromptInput.tsx` (347KB, 2339 行)

**功能**: 整个 TUI 最复杂的组件，处理所有用户输入。

**子系统**:
| 子系统 | 描述 |
|---|---|
| **useInputBuffer** | 输入缓冲区管理 |
| **useArrowKeyHistory** | ↑↓ 浏览历史输入 |
| **useHistorySearch** | Ctrl+R 搜索历史 |
| **useTypeahead** | @ 提及文件/命令自动补全 |
| **usePromptSuggestion** | AI 补全建议 |
| **usePasteHandler** | 多行粘贴检测 |
| **useDoublePress** | 双击 esc 退出 |
| **VimTextInput** | vim 模式 |
| **ShimmeredInput** | 输入时微光效果 |

**PromptInputMode** 状态:
- `default` — 正常输入
- `plan` — 计划模式
- `bash` — bash 模式
- `vim` — vim 模式

**快捷键** (从 defaultBindings.ts):
| 键 | 功能 |
|---|---|
| Enter | 提交 |
| Shift+Enter | 换行 |
| ↑/↓ | 浏览历史 |
| Ctrl+R | 搜索历史 |
| Esc | 清空 / 退出 |
| @ | 文件提及 |
| / | slash 命令 |

**luwu 可借鉴**:
- ↑↓ 历史浏览（简单实现）
- Enter 提交 + Shift+Enter 换行
- `/` slash 命令前缀检测

---

## F4: 底部状态栏

**文件**: `PromptInputFooter.tsx` (32KB) + `StatusLine.tsx` (48KB)

**StatusLine 显示字段**:
```
model.display_name │ current_dir │ git-branch │ context% │ cost │ tokens
```

**StatusLineCommandInput** (可自定义):
- `model.id`, `model.display_name`
- `workspace.current_dir`, `workspace.project_dir`, `workspace.added_dirs`
- `output_style`
- `cost` (total cost, tokens in/out, duration)
- `version`
- `permission_mode`
- `vim_mode`

**PromptInputFooter 额外功能**:
- 左侧: model + mode indicator + vim mode
- 右侧: suggestions (context-aware 快捷键提示)
- 中间: notifications (auto-update, bridge status, etc.)

**luwu 可借鉴**:
- 已有 StatusLine.tsx (model/cwd/git/context%)
- 可加: iteration count, token count

---

## F5: 思考动画

**文件**: `Spinner.tsx` (86KB, 562 行)

**SpinnerMode**:
- `idle` — 空闲
- `thinking` — 思考中
- `streaming` — 流式输出
- `tool_use` — 工具执行中

**关键设计**:
- **SpinnerWithVerb**: 同时显示动词 ("Reading...", "Writing...")
- **2 秒最小显示时间**: 防止 UI 抖动
- **useAnimationFrame(50)**: 50ms 帧率动画
- **stalled 检测**: 长时间无响应 → 改变动画样式

**BriefIdleStatus**: 闲置时显示简洁状态

**luwu 可借鉴**:
- 2 秒最小思考时间
- 动词 + spinner 组合 ("thinking...", "reading file...")
- 已有基础 spinner (ink-spinner dots)

---

## F6: Markdown 渲染

**文件**: `Markdown.tsx` (236 行)

**架构**:
1. **hasMarkdownSyntax** 快速检测 — 纯文本跳过解析（~0ms vs ~3ms）
2. **cachedLexer** — marked.lexer 结果缓存（MRU, max 500）
3. **token → React 组件** 映射
4. **MarkdownTable** (46KB) — 表格渲染
5. **CliHighlight** — 代码块语法高亮（lazy loaded）
6. **configureMarked** — GFM 配置

**token 类型映射**:
| Token | 组件 |
|---|---|
| heading | `<Text bold>` |
| code | `<HighlightedCode>` |
| paragraph | `<Text>` |
| list | `<Box flexDirection="column">` |
| table | `<MarkdownTable>` |
| blockquote | `<Box>` dimColor |
| link | `<Link>` |
| codespan | `<Text>` inline code |

**luwu 可借鉴**:
- hasMarkdownSyntax 快速检测
- 简化版 token→组件映射
- 表格渲染

---

## F7: 主题系统

**文件**: `theme.ts` (640 行) + `design-system/` 16 个文件

**6 个预设主题**: dark / light / light-daltonized / dark-daltonized / light-ansi / dark-ansi

**dark theme 关键色值** (已复刻):
| Key | RGB | 用途 |
|---|---|---|
| claude | rgb(215,119,87) | 品牌橙 |
| suggestion | rgb(87,105,247) | 链接/提示蓝 |
| error | rgb(255,107,128) | 错误红 |
| success | rgb(78,186,101) | 成功绿 |
| warning | rgb(255,193,7) | 警告黄 |
| text | rgb(224,224,224) | 正文 |
| inactive | rgb(136,136,136) | 次要 |
| userMessageBackground | rgb(55,55,55) | 用户消息背景 |

**color() 函数**: curried theme-aware
```ts
color('claude')('hello')  // → ANSI colored string
color('#ff0000')('hello') // → raw hex passthrough
```

**luwu 状态**: ✅ 已在 theme.ts 复刻 dark theme

---

## F8-F15: 后续阶段功能

| 功能 | 描述 | MVP 后优先级 |
|---|---|---|
| F8 VirtualMessageList | 虚拟滚动，处理万条消息 | P1 — 消息多了必须做 |
| F9 Keybindings | 可配置快捷键系统 | P2 — 提升效率 |
| F10 HighlightedCode | 代码块语法高亮 | P1 — 读代码体验 |
| F11 StructuredDiff | git diff 展示 | P2 |
| F12 ModelPicker | 模型切换 | P2 |
| F13 Commands | /help, /clear, /model 等 | P1 — 基础交互 |
| F14 TaskList | TODO 列表 | P3 |
| F15 GlobalSearch | 代码搜索 | P3 |

---

## 设计模式总结

### 1. React Compiler 缓存
所有组件用 `_c(N)` 编译时缓存，减少不必要重渲染。luwu 用 React 18 的 useMemo/useCallback 替代。

### 2. Context + Provider 栈
REPL 用 ~20 个 Context Provider 包裹。luwu 只需要 1-2 个（theme + session）。

### 3. 命令模式
所有 slash 命令注册在 command registry，统一调度。luwu 可简化为 switch-case。

### 4. 状态机
PromptInputMode (default/plan/bash/vim) 是显式状态机。luwu 的 Phase (connecting/ready/thinking/streaming/error) 已经是简化版。

### 5. 渐进式渲染
useDeferredValue + useAnimationFrame 确保流式输出不卡顿。luwu 需要加这个。
