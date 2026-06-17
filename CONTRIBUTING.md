# Contributing to Luwu

> Status: open for contributions.
> Architecture: microkernel + 5-layer hexagonal (see `docs/context/architecture.md`).

## Quick start

```bash
# 1. Clone
git clone https://github.com/trtyr/luwu && cd luwu

# 2. Build (Rust 1.85+ required for edition 2024)
cargo build

# 3. Run tests
cargo test

# 4. Run server + TUI (one-shot)
cargo run                    # spawns daemon + launches TUI
cargo run -- --headless      # server only
```

## Architecture at a glance

```
        ┌─────────────────────────────────────────┐
        │            luwu-server (HTTP)           │   ← transport + composition
        │  (handlers/, services/, app.rs)         │
        └────────────────────┬────────────────────┘
                             │
        ┌────────────────────┴────────────────────┐
        │            luwu-core (traits)           │   ← microkernel
        │  LlmProvider · Tool · MemoryBackend     │
        │  TurnEngine · CycleState · EventBus     │
        │  + Prompt · Skill · ToolRegistry        │
        └────┬─────────────┬─────────────┬────────┘
             │             │             │
   ┌─────────┴──┐  ┌───────┴──────┐  ┌───┴────────┐
   │ luwu-llm   │  │ luwu-tools   │  │ luwu-memory│   ← infrastructure
   │ OpenAI /   │  │ bash, read,  │  │ MemoryStore│
   │ Anthropic  │  │ write, edit, │  │ + workers  │
   │ + retry    │  │ grep, etc.   │  │ (Observer, │
   └────────────┘  └──────────────┘  │ Reflector) │
                                      └────────────┘
```

**Layer rules** (broken rules are a release blocker):
- `luwu-core` depends on **nothing** in this workspace.
- `luwu-llm` / `luwu-tools` / `luwu-memory` depend on `luwu-core` only.
- `luwu-server` may depend on all of the above.
- Tools must use **traits** (e.g. `MemoryBackend`), never concrete infra types.
- Workers route through `LlmProvider`, not raw `reqwest`.

## Iron rules (must follow)

### 1. No `bash grep` / `find` / `cat` / `head` / `tail` / `wc` for code search

**Always use the workspace-relative tools:**

| To do this... | Use this | Never this |
|---|---|---|
| Search file content | `ffgrep` (regex) | ~~`bash grep` / `rg`~~ |
| Find file paths | `fffind` (fuzzy) | ~~`bash find`~~ |
| Read a file | `read` | ~~`bash cat` / `head` / `tail`~~ |
| Semantic code search | `fast_context` | bash one-liners |
| Run tests / build / git | `bash` | (this IS bash's job) |

**Why**: `bash` paths don't see the workspace's git-aware frecency
ranking. They're also a habit that bleeds into wrong-context usage
(filtering build output, etc.).

### 2. No `sed` / `perl` / `python` / `awk` for source code edits

**Always use the dedicated tool:**

| To do this... | Use this | Never this |
|---|---|---|
| Modify a source file | `edit` (with `read`-fresh anchors) | ~~`bash sed` / `perl -i` / `python`~~ |
| Replace a whole file | `write` | (only when edit is too fiddly) |

**Why**: regex-based text replacement is fragile on Rust (similar
identifiers, repeated patterns, multi-line constructs). It's bitten us
multiple times — including a perl truncation that destroyed a 1100-line
handler file.

### 3. No `sed -i` even for single-line changes

Even "obvious" patterns like `s/foo/d/` will match every occurrence in
the file. Rust files have repeated struct field patterns that look
identical but are semantically distinct.

**If the `edit` tool keeps rejecting with stale anchors** (3+ failures
on the same file), switch to `write` to rewrite the file from scratch.
Do not fall back to `sed`.

### 4. Subagent dispatch

**Do not use subagent dispatch for code work.** Worker subagent dispatch
has a persistent concurrency issue (20+ documented failures) that causes
silent hangs and incomplete edits. Always edit serially.

## Code style

- **Rust edition 2024** — requires rustc 1.85+. Stable features only.
- **`cargo fmt` and `cargo clippy --workspace -- -D warnings`** must
  pass before any commit. CI enforces both.
- **No `unsafe`** without an explicit `// SAFETY:` comment.
- **No `unwrap()` in production code** — use `.expect("reason")` with
  a message, or propagate the error.
- **Tracing is the logging system** — use `tracing::{debug, info,
  warn, error}` macros, not `println!` or `eprintln!`.
- **Bilingual comments are fine**: English in code, Chinese in
  design docs and user-facing strings.

## Testing

- **Where tests live**: alongside the code in `#[cfg(test)] mod tests`
  blocks. Integration tests go in `crates/<crate>/tests/`.
- **What to test**: every new public function. Bug fixes must include
  a regression test.
- **Mocking infrastructure**: `luwu-core` defines traits so tests can
  use mock implementations (e.g. `MockBackend` in
  `crates/luwu-core/src/memory_backend.rs`).
- **CJK safety**: any string truncation MUST use `floor_char_boundary()`
  or `char_indices()` — never raw `&s[..N]` byte slicing.

## Commit message format

```
<type>: <short description>

<body explaining why, not what>

<footer with context>
```

Types: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`.
Reference commits from the conversation history if there's prior art
(e.g. `// fixes review P2 #22`).

## Architecture decisions

For any non-trivial change, write a one-page design note in
`docs/plantree/plans/<plan-name>/` before implementing. The plan-tree
skill at `~/.pi/agent/pi-hermes-memory/skills/plan-tree/` documents
the format.

## Release process

1. Bump `version` in workspace `Cargo.toml`.
2. Update `CHANGELOG.md` (if it exists).
3. Tag the commit: `git tag v0.x.y`.
4. Push: `git push origin master --tags`.

## Getting help

- **Architecture overview**: `docs/context/architecture.md`
- **API reference**: `docs/api-reference.md`
- **Error code map**: `docs/error-codes.md`
- **Past reviews**: `docs/review/`
- **Plan tree**: `docs/plantree/`

If you're stuck, open an issue with a minimal reproduction. We're
friendly and we read every one.
