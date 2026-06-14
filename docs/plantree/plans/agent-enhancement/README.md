# Agent 能力强化计划

## 目标

参考 MiMo Code（长程任务）、pi-hermes-memory（记忆系统）、pi-observability/ccstatusline（状态栏）的设计，强化 luwu agent 的四项核心能力。

## Phase 1: grep 索引刷新修复 (P0)

### 问题
- FilePicker 索引建一次就缓存，session 中途用 write/edit 创建或修改的文件不在索引里
- `_glob` 参数定义了但从未使用
- 无法搜索到刚创建的新文件

### 方案
- 每次 grep 执行前检查 FilePicker 的索引是否需要刷新（mtime / 文件数变化检测）
- 或者直接在 write/edit 工具执行后通知 grep 重建索引
- 实现 glob 参数过滤
- 添加正则搜索的 fallback 处理

## Phase 2: 任务管理工具 todo (P1)

### 参考设计
- **Claude Code TodoWrite**: 文件级存储 + 锁机制，支持多 agent 并发
- **pydantic-ai-todo**: 层级任务 + 依赖追踪 + 状态机
- **MiMo Code Goal**: 独立完成度验证，agent 声明完成时系统审查
- **amux**: atomic task board，每个任务可独立验证

### 方案
- 新增 `todo` 工具，支持 create/update/list/get/delete 操作
- 任务结构: { id, subject, description, status (pending/in_progress/completed/deleted), blockedBy[], owner, metadata }
- 状态机: pending → in_progress → completed (+ deleted tombstone)
- 存储方式: JSON 文件，session 级别（~/.luwu/sessions/<id>/tasks.json）
- Agent 系统提示中加入任务管理指导

## Phase 3: Memory 工具重做 (P1)

### 参考设计
- **pi-hermes-memory**: `memory` 工具支持 add/replace/remove，target=memory/user，分类 (failure/correction/insight/preference/convention/tool-quirk)
- **pi-blackhole**: Observer/Reflector/Dropper 三 worker 架构，recall 工具支持 hex ID / #N / #N:path / BM25 搜索
- **pi-observational-memory**: Observations（具体事件）+ Reflections（持久事实），token 阈值触发后台 worker
- **MiMo Code**: 4 层记忆 (session/project/global/history)，独立 writer subagent

### 方案
- 重命名 memory_search → memory
- 新增 memory 工具操作:
  - `search`: 查询记忆（原 memory_search 功能）
  - `write`: 写入记忆（新功能）— 支持 target=global/project/session，category 分类
  - `delete`: 删除记忆条目
- 后台记忆系统（luwu-memory 已有 Observer/Reflector/Dropper worker 架构）
- 前端 toolUtils.ts 更新 toolDisplayName: memory_search → Memory

## Phase 4: 状态栏增强 (P2)

### 参考设计
- **pi-observability**: model+thinking, runtime, cwd, git branch+diff, context bar [████░░] +%, tokens, TPS, cost
- **ccstatusline**: 高度可定制 widget 系统，Powerline 支持，context zone 配色 (≤70% green, 71-85% yellow, >85% red)

### 方案
- 后端 /v1/stats 增强: 返回 token usage (input/output), estimated cost, session runtime, git diff stats (+added/-removed), context window size
- 前端 StatusLine 增强:
  - Context 进度条 [████░░░░░░] 配色 (green/yellow/red)
  - Token 用量 (↑input ↓output)
  - Session runtime 计时器
  - Git branch + diff stats (+N -M)
- 前端 StatusLine 可配置性（后续）

## 优先级
1. Phase 1 (grep 修复) — 直接影响 agent 基本工作能力
2. Phase 2 (todo 工具) — 影响长任务管理
3. Phase 3 (memory 重做) — 影响跨 session 记忆
4. Phase 4 (状态栏) — 提升用户体验
