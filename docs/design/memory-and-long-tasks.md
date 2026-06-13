# Luwu 记忆与长任务机制设计

> 参考：MiMo Code 长程任务设计 (mimo.xiaomi.com/zh/blog/mimo-code-long-horizon)

## 1. 问题定义

### 1.1 上下文耗尽

Agent 持续执行 N 轮后，对话历史（用户消息 + LLM 回复 + 工具调用 + 工具输出）会填满上下文窗口。
到达上限时，要么会话结束，要么模型质量退化（lost in the middle）。

### 1.2 状态断裂

简单的摘要压缩会丢失细节。长任务（50+ 轮）中，早期做出的架构决定、遇到的错误及修复路径、
用户明确表达的偏好——这些信息在压缩中逐渐衰减，导致后续轮次决策质量下降。

### 1.3 跨 session 遗忘

每次 session 结束后所有经验丢失。Agent 无法从过去的工作中积累，每次重新发现同样的项目约束。

## 2. 核心概念

### 2.1 Cycle — 无界会话的基本单元

```
一个逻辑会话 (Session) = N 个 Cycle 链

Cycle 1:
  [turn 1..10] → checkpoint@20% → [turn 11..25] → checkpoint@45%
  → [turn 26..35] → checkpoint@70% → [turn 36..40] → rebuild

Cycle 2:
  [从 checkpoint 恢复上下文] → [turn 41..50] → checkpoint@20% → ... → rebuild

Cycle N: ...
```

每个 Cycle 内，上下文窗口有界。Cycle 之间通过 checkpoint 文件传递状态。
从 LLM 视角看，对话从未中断——每次 rebuild 后它拿到的上下文包含完整工作状态。

### 2.2 Checkpoint — 结构化状态快照

不是"摘要"，是 11 个固定字段的结构化提取：

```
1. current_intent     — 当前意图：Agent 正在做什么
2. next_action        — 下一步动作：紧接着该执行什么
3. constraints        — 工作约束：用户要求的规则、限制
4. task_tree          — 任务树：总目标 → 子任务 → 进度
5. current_work       — 当前工作：正在处理的文件/函数/模块
6. involved_files     — 涉及文件：已读/已改/待处理的文件清单
7. discoveries        — 跨任务发现：项目架构、API 特性、踩过的坑
8. errors_and_fixes   — 错误与修复：遇到的错误及解决方案
9. runtime_state      — 运行时状态：分支、环境变量、进程
10. design_decisions  — 设计决策：为什么选 A 不选 B
11. notes             — 杂项笔记
```

### 2.3 Writer — 独立提取者

关键约束：**主 Agent 不维护自己的记忆。**

Writer 是一个独立的 LLM 调用（用同一个 provider，但独立于主循环）：
- 读取当前 session 的完整对话历史
- 输出结构化 checkpoint
- 写入磁盘文件
- 与主 Agent 并发执行，不抢占主循环的 token 预算

### 2.4 Rebuild — 上下文重建

当 token 用量接近上限时，执行 rebuild：
1. 停止当前 cycle
2. 读取最新 checkpoint 文件
3. 读取 project 记忆、global 记忆
4. 读取 notes（主 Agent 的自由格式暂存）
5. 组装为新的 system prompt + 上下文注入
6. 开启新 cycle，主 Agent 从中继续

注入顺序（按优先级）：

```
[任务清单]           ← Agent 首先要知道自己该做什么
[Session Checkpoint] ← 当前工作状态
[最近用户消息原文]    ← 防止 writer 改写偏离用户原意
[Project 记忆]       ← 跨 session 的项目知识
[Global 记忆]        ← 用户偏好
[Notes]              ← 主 Agent 的零散发现
[Tail Reminder]      ← "下一步该做什么"
```

## 3. 四层记忆

```
┌─────────────────────────────────────────────────────┐
│ Global 记忆 (~/.luwu/memory/global.md)              │
│ 用户级偏好，跨项目。用户明确表达的编码习惯、沟通风格。   │
│ 生命周期：永久，手动或 Dream 维护。                     │
├─────────────────────────────────────────────────────┤
│ Project 记忆 (.luwu/memory/project.md)               │
│ 项目级知识——架构决定、用户规则、验证过的技术事实。       │
│ 生命周期：永久。Writer 从 session 提炼稳定观察时写入。   │
├─────────────────────────────────────────────────────┤
│ Session 记忆 (.luwu/memory/{session_id}/checkpoint.md)│
│ 当前 session 工作状态。11 字段结构化快照。              │
│ 生命周期：当前 session，session 结束后归档到 history。  │
├─────────────────────────────────────────────────────┤
│ History (.luwu/memory/{session_id}/history.jsonl)    │
│ 完整对话原文。每条消息、每次工具调用原文存储。            │
│ 生命周期：永久，可回溯。                                │
└─────────────────────────────────────────────────────┘

上层：精炼、持久、小
下层：完整、庞大、慢
```

### 3.1 存储格式选择

全部使用文件，不用 SQLite。原因：

1. **可审查** — 用户可以直接 `read` 工具看 Agent 记住了什么
2. **可编辑** — 用户可以 `edit` 工具修改/删除记忆
3. **简单** — 不引入数据库依赖，文件系统就是数据库
4. **Git 友好** — project 记忆可以纳入版本控制

History 用 JSONL（每行一条 JSON），便于 append 和回溯搜索。

### 3.2 写入权限

| Actor | 可写 | 可读 |
|---|---|---|
| 主 Agent | notes.md（append only） | 全部 |
| Writer | checkpoint.md, project.md | 全部 |
| 用户 | 全部 | 全部 |

Single-writer 不变量：每个文件只有一个写入者。

## 4. 模块设计

### 4.1 新增 crate：luwu-memory

```
crates/luwu-memory/
├── Cargo.toml
└── src/
    ├── lib.rs          — 公开接口
    ├── store.rs        — 文件系统存储（读写四层记忆）
    ├── checkpoint.rs   — Checkpoint 结构体 + 序列化
    ├── history.rs      — JSONL 历史记录 append/search
    └── estimator.rs    — Token 用量估算
```

依赖关系：`luwu-core` 定义 trait，`luwu-memory` 实现。

### 4.2 luwu-core 变更

```rust
// engine.rs 新增

/// Token 用量估算（粗略：chars / 4）
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Cycle 管理状态
pub struct CycleState {
    /// 当前 cycle 序号
    pub cycle_index: usize,
    /// 估算的 token 消耗
    pub tokens_used: usize,
    /// 配置的 token 上限
    pub token_budget: usize,
    /// Checkpoint 触发阈值（百分比）
    pub checkpoint_thresholds: Vec<u8>,  // 默认 [20, 45, 70]
    /// 已触发的 checkpoint
    pub triggered_checkpoints: Vec<u8>,
}

impl TurnEngine {
    /// 在 run_stream 的主循环中，每次 LLM 调用后检查是否需要 checkpoint。
    /// 如果到达阈值 → 触发 writer（异步，不阻塞主循环）。
    /// 如果接近上限 → 执行 rebuild。
    fn check_cycle(&mut self, tokens_used: usize) -> CycleAction {
        let pct = (tokens_used * 100 / self.cycle_state.token_budget) as u8;
        // ...
    }
}

pub enum CycleAction {
    Continue,
    Checkpoint,          // 触发 writer
    RebuildAndContinue,  // rebuild 后继续
    Stop,                // 真正结束
}
```

### 4.3 Writer 实现

Writer 不需要新的 crate。它就是一个独立的 LLM 调用：

```rust
// 在 luwu-core 或 luwu-server 中

async fn run_checkpoint_writer(
    provider: Arc<dyn LlmProvider>,
    messages: Vec<Message>,
    checkpoint_path: &Path,
) -> Result<()> {
    let system_prompt = r#"
你是一个状态提取器。阅读以下对话历史，提取结构化状态。
输出 Markdown 格式，包含以下 11 个字段。
只输出你确信的信息，不确定的标为"未知"。
"#;

    let request = LlmRequest {
        system_prompt: Some(system_prompt.to_string()),
        messages,
        tools: vec![],  // writer 不需要工具
        temperature: Some(0.1),  // 低温度，精确提取
        ..Default::default()
    };

    // 用 run() 而非 run_stream()，writer 不需要流式
    let result = TurnEngine::run(provider, request, ...).await?;

    // 写入 checkpoint 文件
    std::fs::write(checkpoint_path, &result.assistant_text)?;

    Ok(())
}
```

### 4.4 Rebuild 实现

```rust
fn build_rebuild_context(
    checkpoint: &str,
    project_memory: &str,
    global_memory: &str,
    notes: &str,
    recent_user_messages: &[String],
) -> String {
    let mut ctx = String::new();

    // 1. Checkpoint（工作状态）
    ctx.push_str("## 当前工作状态\n\n");
    ctx.push_str(checkpoint);

    // 2. 最近用户消息原文
    if !recent_user_messages.is_empty() {
        ctx.push_str("\n\n## 用户原始请求\n\n");
        for msg in recent_user_messages {
            ctx.push_str(msg);
            ctx.push('\n');
        }
    }

    // 3. Project 记忆
    if !project_memory.is_empty() {
        ctx.push_str("\n\n## 项目知识\n\n");
        ctx.push_str(project_memory);
    }

    // 4. Global 记忆
    if !global_memory.is_empty() {
        ctx.push_str("\n\n## 用户偏好\n\n");
        ctx.push_str(global_memory);
    }

    // 5. Notes
    if !notes.is_empty() {
        ctx.push_str("\n\n## 工作笔记\n\n");
        ctx.push_str(notes);
    }

    // 6. Tail reminder
    ctx.push_str("\n\n## 下一步\n\n");
    ctx.push_str("请根据以上状态继续工作。从 next_action 字段描述的动作开始。");

    ctx
}
```

### 4.5 API 变更

现有的 `POST /v1/sessions/{id}/chat` 不变，长任务是同一个接口——
只是内部多了 cycle 管理。前端不需要感知 cycle 的存在。

新增的配置项：

```toml
# ~/.luwu/config.toml

[agent]
# Token 预算（估算值）。到达上限时触发 rebuild。
# 默认 100000（约 100K token）
token_budget = 100000

# Checkpoint 触发阈值（百分比）
checkpoint_thresholds = [20, 45, 70]

# 是否启用长任务（记忆）机制
memory_enabled = true
```

新增的记忆相关 API（可选，供前端展示用）：

```
GET  /v1/sessions/{id}/memory        — 获取 session 记忆状态
GET  /v1/sessions/{id}/checkpoint    — 获取最新 checkpoint
GET  /v1/sessions/{id}/history       — 获取对话历史
```

## 5. 数据流

### 5.1 正常流程（短任务）

```
用户消息 → run_stream → LLM 调用 → 工具调用 → LLM 调用 → ... → Done
                                                    ↓
                                              (token < 20%)
                                              不触发任何记忆操作
```

短任务（< 20% token 预算）完全不受记忆机制影响，零额外开销。

### 5.2 长任务流程

```
用户消息 → run_stream 开始
  │
  ├─ [主循环] LLM 调用 → 工具 → LLM 调用 → ...
  │                                      ↓
  │                               累计 token 达到 20%
  │                                      ↓
  │                         [触发 Writer] ← 异步，不阻塞主循环
  │                              Writer 读对话历史
  │                              Writer 输出 checkpoint.md
  │                              Writer 写磁盘
  │                                      ↓
  │                         [主循环继续] ← 不中断
  │                                      ↓
  │                               累计 token 达到 45%
  │                                      ↓
  │                         [触发 Writer] ← 增量更新 checkpoint
  │                                      ↓
  │                               累计 token 达到 90%
  │                                      ↓
  │                         [Rebuild]
  │                              读取 checkpoint.md
  │                              读取 project 记忆
  │                              读取 global 记忆
  │                              组装新上下文
  │                              清空对话历史
  │                              注入新上下文
  │                                      ↓
  │                         [新 Cycle 开始]
  │                              token 计数器归零
  │                              主 Agent 继续
  │                                      ↓
  │                               ... (循环)
  │                                      ↓
  └─ [Done] 最终结果
```

### 5.3 Writer 时序

```
主循环:  [turn] [turn] [turn] [20%!] [turn] [turn] [45%!] [turn] ...
                          ↓                    ↓
Writer:              [读历史]              [读历史]
                     [写checkpoint]        [更新checkpoint]
                          ↓                    ↓
                     磁盘: checkpoint.md   磁盘: checkpoint.md (updated)
```

Writer 和主循环并发。Writer 完成前主循环不等待（但 rebuild 时必须等 writer 完成）。

## 6. 目录结构

```
~/.luwu/
├── config.toml                              # 配置
└── memory/
    ├── global.md                            # 全局记忆
    └── {project_hash}/                      # 项目级记忆
        ├── project.md                       # 项目记忆
        └── sessions/
            └── {session_id}/
                ├── checkpoint.md            # 最新 checkpoint
                ├── notes.md                 # 主 Agent 自由格式笔记
                └── history.jsonl            # 完整对话原文
```

项目路径通过哈希映射到目录名，避免路径中的特殊字符问题。

## 7. 实施计划

### Phase A: 存储层（luwu-memory crate）

1. 创建 `luwu-memory` crate
2. 实现 `MemoryStore` — 四层记忆的读写接口
3. 实现 `Checkpoint` 结构体 — 11 字段序列化/反序列化
4. 实现 `HistoryWriter` — JSONL append + 搜索
5. 实现 `TokenEstimator` — 基于字符数的 token 估算
6. 单元测试

### Phase B: Cycle 管理（luwu-core 变更）

1. 在 `TurnEngine` 中加入 `CycleState`
2. 每次 LLM 调用后估算 token 用量
3. 在 checkpoint 阈值处触发 Writer
4. 实现 rebuild 逻辑（清空 + 注入）
5. 集成测试

### Phase C: Writer（luwu-server 变更）

1. 实现 `run_checkpoint_writer()` — 独立 LLM 调用
2. 实现 `build_rebuild_context()` — 上下文组装
3. 集成到 `run_stream` 的主循环
4. 端到端测试

### Phase D: API + 配置

1. 配置文件加入 `[agent]` section
2. 新增记忆相关 API 端点
3. 前端可查询记忆状态

## 8. 不做的事

- **Max Mode（并行采样）**：token 消耗 ×5，不适合当前阶段
- **Goal（完成度验证）**：需要额外 LLM 调用预算，后续考虑
- **Dream/Distill**：跨 session 的记忆整理，依赖 History 积累后再做
- **Dynamic Workflow**：大规模并行编排，属于 Phase 8（Multi-agent）范畴
