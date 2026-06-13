# Claude Code TUI 深度细节分析报告（逐行源码级）

> 分析方法：逐文件读 restored-src，抠渲染逻辑、布局参数、间距、颜色、交互行为
> 源码特点：React Compiler 编译产物（$[N] 缓存槽），需过滤编译噪音提取实际逻辑

---

## 一、消息布局细节（最关键的发现）

### 1.1 用户消息 — UserPromptMessage

**不是 `❯` 前缀！** 是**整行背景色** + 缩进：

```
渲染结构:
<Box flexDirection="column"
     marginTop={addMargin ? 1 : 0}         ← 消息间距：首条消息无间距，后续 +1 行
     backgroundColor="userMessageBackground" ← #373737 灰底背景
     paddingRight={1}>                      ← 右边距 1 字符
  <HighlightedThinkingText text={displayText} />
</Box>
```

**关键细节**：
- 首条消息 `addMargin=false`（marginTop=0），后续消息 `addMargin=true`（marginTop=1）
- 用户消息有**背景色**（不是前缀符号），整行灰底 `#373737`
- 长文本截断：`MAX_DISPLAY_CHARS=10000`，head 2500 + tail 2500 + `… +N lines …`
- 选中时背景变为 `messageActionsBackground`
- `paddingRight={1}` — 右侧留白

**对比 luwu 当前**：用的是 `❯ ` 前缀 + 无背景色 + 无间距控制 — **完全错误**

### 1.2 助手消息 — AssistantTextMessage

**`●` 前缀是 `text` 白色（非选中时）/ `suggestion` 蓝色（选中时），不是橙色！**

```
渲染结构（默认 case）:
<Box alignItems="flex-start"
     flexDirection="row"
     justifyContent="space-between"
     marginTop={addMargin ? 1 : 0}
     width="100%"
     backgroundColor={isSelected ? "messageActionsBackground" : undefined}>
  <Box flexDirection="row">
    {shouldShowDot && <NoSelect minWidth={2}>
      <Text color={isSelected ? "suggestion" : "text"}>{BLACK_CIRCLE}</Text>
    </NoSelect>}
    <Box flexDirection="column">
      <Markdown>{text}</Markdown>
    </Box>
  </Box>
</Box>
```

**关键细节**：
- `BLACK_CIRCLE` = `●`，宽度 `minWidth={2}`（占 2 字符位，文本从第 3 列开始）
- `NoSelect` 包裹圆点 — 用户无法选中前缀
- Markdown 在 Box(flexDirection=column) 里渲染
- 助手消息**无背景色**（除了选中态）
- 错误消息有特殊分支：rate limit / API key invalid / timeout / credit low 各有专门 UI

**对比 luwu 当前**：用的是 `● ` 橙色前缀 — **颜色错误（应该是白色 text），前缀宽度不够（应该 minWidth=2）**

### 1.3 MessageResponse — 响应缩进

**关键发现：所有回复内容用 `⎿` 符号缩进！**

```
渲染结构:
<Box flexDirection="row" height={height} overflowY="hidden">
  <NoSelect flexShrink={0}>
    <Text dimColor>  ⎿  </Text>           ← dimColor 灰色，2 空格 + ⎿ + 2 空格
  </NoSelect>
  <Box flexShrink={1} flexGrow={1}>
    {children}                              ← 实际内容
  </Box>
</Box>
```

**这是 Claude Code 的标志性视觉**：助手回复、工具结果、错误消息都包在 MessageResponse 里，统一用 `⎿` 缩进

**对比 luwu 当前**：完全没有 `⎿` 缩进 — **缺失**

### 1.4 消息间距规则

从 REPL.tsx 的 VirtualMessageList 渲染逻辑：
- 每条消息的 `addMargin` 属性控制间距
- 第一条消息 `addMargin=false`
- 后续所有消息 `addMargin=true`（marginTop=1）
- 消息之间没有额外 gap — 间距完全靠 marginTop

---

## 二、输入框细节

### 2.1 边框

PromptInput 用 `BorderTextOptions` 渲染带文字标注的边框：
- 边框样式：通过 `ink/render-border.js` 自定义渲染
- 边框上方可以显示文字（BorderText）— 类似 HTML fieldset legend
- `buildBorderText()` 函数动态构建边框文字（fast mode 提示等）

**luwu 当前**：无边框，只有 `> ` 前缀 — **需要加 Box border**

### 2.2 历史浏览

```
useArrowKeyHistory hook:
- ↑ 在第一行时触发历史浏览，否则光标上移
- ↓ 在最后一行时触发历史浏览，否则光标下移
- 多行输入时光标移动优先于历史浏览
- Ctrl+R 进入历史搜索模式
- 历史记录保存在文件中，跨 session 持久化
```

**luwu 当前**：有 ↑↓ 历史但无多行判断、无 Ctrl+R 搜索

### 2.3 slash 命令补全

**完整流程**（useTypeahead hook）：
1. 用户输入 `/` → 触发 `isCommandInput(value)` 检测
2. 实时用 Fuse.js 模糊搜索匹配命令
3. 搜索权重：命令名(3) > 命令分段/别名(2) > 描述(0.5)
4. 显示建议列表（PromptInputFooterSuggestions）
5. ↑↓ 选择，Tab/Enter 补全
6. 命令有空格后显示 `argumentHint`（参数提示）
7. 列表选中第一条默认高亮（selectedSuggestion=0）

**commandSuggestions.ts** 关键配置：
- Fuse.js threshold: 0.3（较严格匹配）
- 命令名按 `[:-_]` 分段做二级搜索 key
- 描述词拆分做低权重搜索
- 列表宽度取所有命令最大宽度（防止布局抖动）

**luwu 当前**：输入 `/` 后无补全列表 — **完全缺失**

### 2.4 PromptInputMode 状态

```
type PromptInputMode = 'prompt' | 'plan' | 'bash' | 'vim'
```
- `prompt`: 正常输入
- `plan`: 计划模式（只读分析不改文件）
- `bash`: bash 执行模式
- `vim`: vim 编辑模式

**luwu 当前**：只有默认模式 — MVP 不需要 plan/bash/vim

---

## 三、Spinner / 思考动画细节

### 3.1 SpinnerMode 状态机

```
SpinnerMode:
  idle    → 不显示
  thinking → "✻ thinking…" + 旋转动画
  streaming → 不显示（文本在流式输出）
  tool_use → "✻ <verb>…" + 旋转动画
```

### 3.2 时间控制

```
- thinkingStart: 进入 thinking 时记录时间
- 退出 thinking 时计算 elapsed
- 最小显示 2 秒（Math.max(0, 2000 - elapsed)）
- 退出后显示 "⏱ X.Xs" 持续 2 秒
- useAnimationFrame(50) — 50ms 帧率
```

### 3.3 SpinnerWithVerb

```
- 动态动词来自当前正在执行的工具
- "Reading file…", "Writing code…", "Searching…"
- stalled 检测：超过阈值无事件 → 改变样式
```

**luwu 当前**：有基础 spinner + 2s 最小显示，但无动词、无 stalled 检测

---

## 四、StatusLine 细节

### 4.1 布局

```
模型名 │ 当前目录 │ git分支 │ context% │ cost │ tokens

特殊处理:
- 模型名用 model.display_name（如果有），否则 model.id
- 目录只显示最后两级（类似 ~/…/luwu）
- context% 颜色：< 50% 绿色, 50-80% 黄色, > 80% 红色
- cost 显示 "$0.42" 格式
- 全部用 theme 色：模型 claude 橙, 目录 inactive 灰, 分支 suggestion 蓝
```

### 4.2 底部提示行

StatusLine 下方还有一行 context-aware 快捷键提示：
```
? for help · ↑↓ history · / for commands · esc to clear
```
根据当前状态动态变化（有建议时显示 Tab 补全，搜索时显示 Ctrl+R）

**luwu 当前**：有状态栏但无底部快捷键提示行

---

## 五、Markdown 渲染细节

### 5.1 性能优化

```
1. hasMarkdownSyntax(text): 正则快速检测
   - 纯文本（无 #*`|[]<>-\_~）→ 跳过解析，直接渲染 <Text>
   - 只检查前 500 字符
2. cachedLexer(content): marked.lexer 缓存
   - MRU 策略，max 500 entries
   - key = hashContent(content)
   - 滚动回旧消息时不重复解析
3. React Compiler 自动缓存 token→组件映射
```

### 5.2 token 映射

| Token | 渲染 | 颜色 |
|---|---|---|
| heading | `<Text bold>` | claude 橙 |
| code (fenced) | `<Box>` + ``` 围栏 | 内容 success 绿, 围栏 subtle 灰 |
| paragraph | `<Text>` | text 白 |
| list | `<Box flexDirection="column">` + bullet | bullet suggestion 蓝 |
| blockquote | `<Text italic>` | inactive 灰 |
| codespan (inline) | `<Text>` | success 绿 |
| link | `<Text>` | suggestion 蓝 |
| hr | `<Text>` | subtle 灰 |
| table | MarkdownTable 组件 | — |

### 5.3 代码块处理

```
- 代码块不语法高亮（默认）
- CliHighlight 是 lazy loaded 的可选高亮
- 代码块用围栏 ``` 包裹显示
- 长代码块可折叠（Ctrl+O 展开）
```

**luwu 当前**：有基础 Markdown 但无性能缓存、无表格

---

## 六、颜色系统细节

### 6.1 实际使用频率

| Theme Key | 使用场景 | 频率 |
|---|---|---|
| text | 助手消息内容、● 前缀 | 极高 |
| claude | 品牌色、heading、模型名 | 高 |
| suggestion | 用户选中态、链接、bullet | 高 |
| error | 错误消息 | 中 |
| success | 代码块、成功状态 | 中 |
| warning | 警告 | 低 |
| inactive | blockquote、次要信息 | 中 |
| subtle | 围栏、hr、placeholder | 中 |
| userMessageBackground | 用户消息背景 | 高 |
| promptBorder | 输入框边框 | 高 |

### 6.2 重要纠正

**之前复刻的错误**：
- ❌ 用户消息用 `❯ suggestion 蓝` 前缀 → ✅ 用 `userMessageBackground` 背景色，无前缀
- ❌ 助手 `● ` 用 claude 橙 → ✅ 用 `text` 白色（非选中时）
- ❌ 无 `⎿` 缩进 → ✅ 所有回复内容用 `⎿` 缩进
- ❌ 无消息间距控制 → ✅ addMargin 控制首条/后续间距

---

## 七、工具调用展示细节

### 7.1 AssistantToolUseMessage

```
结构:
⎿ ⚡ Read(file.ts)                    ← 工具名 + 参数（紧凑显示）
  ⎿ 123 lines read                     ← 结果（缩进在工具下方）

状态:
- 进行中: ⟳ 旋转 + 工具名
- 完成: ✓ + 结果摘要
- 错误: ✗ + 错误信息

折叠:
- 连续 read/grep 结果自动折叠
- 折叠后显示 "N tool results collapsed"
- Ctrl+O 展开
```

### 7.2 GroupedToolUseContent

```
多个连续工具调用合并显示:
⎿ ⚡ Read(a.ts), Read(b.ts), Read(c.ts)
  ⎿ 3 results collapsed
```

**luwu 当前**：工具调用内联在消息里，无折叠、无 ⎿ 缩进

---

## 八、REPL 整体布局

### 8.1 垂直布局（从上到下）

```
┌─────────────────────────────────┐
│                                 │ ← marginTop（终端顶部留白）
│  消息列表（滚动区域）              │
│  ┌─────────────────────────┐    │
│  │ 用户消息（灰底背景）        │    │ ← userMessageBackground
│  └─────────────────────────┘    │
│  ┌─────────────────────────┐    │
│  │ ● 助手消息（Markdown）     │    │ ← text 白色 ●
│  │ ⎿ 回复内容（缩进）          │    │ ← ⎿ dimColor
│  └─────────────────────────┘    │
│                                 │
│  ✻ thinking… / spinner          │ ← 思考动画区
├─────────────────────────────────┤
│ ┌─ 输入框（带边框）─────────────┐ │ ← borderStyle + BorderText
│ │ > type here_                │ │
│ └─────────────────────────────┘ │
│                                 │
│ /help /clear  ↑↓ history  esc   │ ← 快捷键提示行
│ model │ ~/luwu │ main │ 15%     │ ← StatusLine
└─────────────────────────────────┘
```

### 8.2 关键间距

| 元素 | 间距 |
|---|---|
| 首条消息 | marginTop=0 |
| 后续消息 | marginTop=1 |
| 用户消息背景 | 整行，paddingRight=1 |
| 助手 ● 前缀 | minWidth=2 |
| ⎿ 缩进 | 2空格 + ⎿ + 2空格 = 6 字符 |
| 输入框 | borderStyle 包裹 |
| StatusLine | 紧贴输入框下方 |

---

## 九、luwu 需要修正的清单

### 🔴 必须修正（视觉错误）

| # | 当前 | 应该是 | 优先级 |
|---|---|---|---|
| C1 | 用户消息 `❯ ` 前缀 | `userMessageBackground` 背景色，无前缀 | P0 |
| C2 | 助手 `● ` 橙色 | `●` 白色(text)，minWidth=2 | P0 |
| C3 | 无 `⎿` 缩进 | 所有回复内容用 `⎿` 缩进 | P0 |
| C4 | 无消息间距控制 | addMargin: 首条=0, 后续=1 | P0 |
| C5 | 无输入框边框 | Box border + borderText | P1 |
| C6 | 无斜杠补全列表 | `/` 触发列表 + ↑↓ 选择 | P1 |
| C7 | 无快捷键提示行 | 底部 context-aware 提示 | P2 |
| C8 | 工具结果无缩进 | `⎿` 缩进 + 可折叠 | P2 |

### 🟡 可以增强

| # | 功能 | 描述 |
|---|---|---|
| E1 | 动态 spinner 动词 | "Reading…", "Writing…" |
| E2 | Markdown 缓存 | MRU token cache |
| E3 | 表格渲染 | MarkdownTable |
| E4 | stalled 检测 | 长时间无响应改变 spinner 样式 |

---

## 十、架构参考（luwu 自己的微内核，不抄 Claude Code）

Claude Code 的前端架构是一坨——REPL.tsx 5000 行、PromptInput 2300 行、20 层 Context Provider。luwu 要做的是**反过来**：

```
ui/src/
├── core/           # 微内核：types + state machine + constants
│   ├── types.ts
│   ├── state.ts    # Phase 状态机 + 转换规则
│   └── constants.ts # 命令定义、快捷键映射
├── hooks/          # 可复用 hooks（纯逻辑，无 UI）
│   ├── useSession.ts    # 会话管理
│   ├── useStream.ts     # SSE 流处理
│   ├── useHistory.ts    # 输入历史
│   ├── useCommands.ts   # slash 命令
│   └── useSuggestion.ts # 补全建议
├── services/       # 外部服务层
│   └── api.ts      # HTTP/SSE 客户端
├── theme/          # 主题系统
│   └── index.ts
├── components/     # 纯展示组件（无业务逻辑）
│   ├── MessageList.tsx
│   ├── UserMessage.tsx
│   ├── AssistantMessage.tsx
│   ├── SystemMessage.tsx
│   ├── ToolResult.tsx
│   ├── PromptInput.tsx
│   ├── SuggestionList.tsx
│   ├── StatusLine.tsx
│   ├── Spinner.tsx
│   └── Markdown.tsx
└── App.tsx         # 组合层：wiring hooks → components
```

**核心原则**：
1. **依赖单向流动**：core ← hooks ← services ← components ← App
2. **组件纯展示**：不包含业务逻辑，所有数据通过 props 传入
3. **hooks 纯逻辑**：不渲染任何 UI，返回 state + actions
4. **services 可替换**：api.ts 可以换成 WebSocket 或任何后端
5. **core 零依赖**：不 import 任何 React/Ink 组件

---

## Round 2-6 追加发现

### PromptInput 边框
- 只开顶部+底部边框（无左右）
- borderColor 动态：busy=橙色, idle=灰色

### ReasoningBlock
- 折叠：`∴ Thinking` dimColor italic
- 展开：paddingLeft=2 Markdown

### Spinner
- 64 动词池，thinkingStatus 状态机

### SuggestionList
- 列宽对齐，en-dash 分隔，最多6行

### ToolResult
- [icon] **ToolName**(params) → nested result
