# luwu Skill System Design

遵循 [Agent Skills 开放标准](https://agentskills.io/specification)，实现渐进式加载的可复用工作流系统。

## 核心概念

| | Tool | Skill |
|---|---|---|
| **是什么** | 原子操作（bash、read、write） | 可复用的多步骤工作流 |
| **类比** | 手 | 菜谱 |
| **谁执行** | TurnEngine 调 tool.execute() | LLM 读指令后自己用 tool 组合完成 |
| **加载方式** | 启动时全量注册 | 渐进式：metadata → 指令 → 参考文档 |
| **来源** | 代码内硬编码 | 文件系统上的 SKILL.md |

## 渐进式加载（Progressive Disclosure）

这是整个系统最核心的设计——100 个 skill 也只占几千 token 的 metadata 开销。

```
Level 1: Metadata（启动时注入 system prompt）
  skill name + description
  每个 skill ~20-50 tokens
  ┌──────────────────────────────────────┐
  │ - deploy: Deploy project to staging  │
  │ - review: Code review with metrics   │
  │ - db-migrate: Database migration     │
  └──────────────────────────────────────┘

Level 2: Instructions（skill 被激活时加载）
  完整 SKILL.md body
  < 5000 tokens
  ┌──────────────────────────────────────┐
  │ # Deploy                             │
  │ ## Steps                             │
  │ 1. Run cargo test                    │
  │ 2. Run cargo clippy                  │
  │ 3. git tag ...                       │
  │ ...                                  │
  └──────────────────────────────────────┘

Level 3: Resources（按需读取）
  scripts/, references/, assets/
  ┌──────────────────────────────────────┐
  │ scripts/deploy.sh                    │
  │ references/staging-config.md         │
  │ assets/deploy-template.yaml          │
  └──────────────────────────────────────┘
```

## 目录结构

```
# 全局 skills
~/.luwu/skills/
├── deploy/
│   ├── SKILL.md
│   ├── scripts/
│   │   └── deploy.sh
│   └── references/
│       └── staging.md
├── code-review/
│   └── SKILL.md
└── db-migrate/
    ├── SKILL.md
    └── scripts/
        └── migrate.sh

# 项目 skills（优先级高于全局）
<project-root>/.luwu/skills/
├── test-runner/
│   └── SKILL.md
└── custom-build/
    └── SKILL.md
```

## SKILL.md 格式（遵循 Agent Skills 标准）

```markdown
---
name: deploy
description: Deploy project to staging or production. Use when user says "deploy", "ship", or "release".
---

# Deploy

## Prerequisites
- Ensure all tests pass
- Check current branch is clean

## Steps

1. Run tests: `cargo test --workspace`
2. Run linter: `cargo clippy --workspace`
3. Bump version in Cargo.toml
4. Commit with `chore: bump version to X.Y.Z`
5. Tag: `git tag vX.Y.Z`
6. Push: `git push origin master --tags`

## Rollback
If deployment fails, revert to previous tag.
```

## Skill 数据结构

```rust
/// 一个加载好的 Skill。
struct Skill {
    name: String,           // "deploy"，遵循 Agent Skills 命名规则
    description: String,    // 最多 1024 字符
    instructions: String,   // SKILL.md body（去掉 frontmatter 的部分）
    base_path: PathBuf,     // skill 目录的绝对路径
}
```

## SkillRegistry

```rust
/// 扫描、加载、管理所有 skill。
struct SkillRegistry {
    skills: Vec<Skill>,
}

impl SkillRegistry {
    /// 从全局 + 项目目录扫描所有 skill
    fn discover(luwu_home: &Path, project_dir: &Path) -> Result<Self>;

    /// 返回所有 skill 的 metadata（用于 system prompt 注入）
    fn skill_metadata_prompt(&self) -> String;

    /// 按 name 查找 skill，返回完整 instructions
    fn get(&self, name: &str) -> Option<&Skill>;

    /// 列出所有 skill（用于 API）
    fn list(&self) -> &[Skill];
}
```

## System Prompt 注入

`system_prompt_with_tools` 扩展为也包含 skill metadata：

```
## Available Tools
- bash
- read
- write
- edit
- grep
- web_fetch

## Available Skills
The following skills are available. When a task matches a skill's description, follow its instructions:

- deploy: Deploy project to staging or production. Use when user says "deploy"...
- code-review: Perform code review with quantitative metrics...
```

当 LLM 决定使用某个 skill 时，TurnEngine 检测到（通过文本匹配或专用标记），读取完整 SKILL.md 注入到下一轮对话。

## Skill 激活机制

LLM 不需要特殊的"激活 skill"工具。工作流是：

1. LLM 读到 system prompt 里的 skill metadata
2. LLM 判断当前任务匹配某个 skill → 在回复中引用它
3. TurnEngine 检测回复中包含 skill 引用 → 读取完整 instructions
4. Instructions 作为 assistant 上下文的一部分注入后续对话
5. LLM 按照 instructions 逐步执行（使用现有的 tool 系统）

具体检测方式：LLM 在回复中用 `[skill:name]` 标记表示要使用某个 skill，或者直接说"I will use the deploy skill"。TurnEngine 做简单的字符串匹配。

## HTTP API

| 端点 | 说明 |
|---|---|
| GET /v1/skills | 列出所有已加载的 skill（name + description） |
| GET /v1/skills/{name} | 获取 skill 详情（完整 instructions + 文件列表） |

不需要 POST/PUT/DELETE——skill 是文件系统管理的，不支持运行时创建。

## 模块位置

`luwu-core/src/skill.rs` — 跟 `tool.rs`、`tool_registry.rs` 同层，属于核心注册能力。

## 文件结构变化

```
crates/luwu-core/src/
├── skill.rs              # Skill struct + SkillRegistry + discovery
├── prompt/mod.rs         # system_prompt 扩展，注入 skill metadata
├── engine.rs             # TurnEngine 持有 SkillRegistry，检测 skill 激活
└── lib.rs                # 导出 Skill, SkillRegistry
```

## 不做的事

- **不做 skill 运行时创建 API** — skill 是文件系统管理的，不支持 HTTP 创建
- **不做 skill 脚本执行** — scripts/ 里的脚本通过 bash tool 执行，不需要专门的执行引擎
- **不做 skill 权限控制** — allowed-tools 字段解析但不强制执行（跟其他实现一样，experimental）
- **不做 skill 包管理** — 不做 pi install 那样的包管理器，用户自己 git clone 或手动创建
