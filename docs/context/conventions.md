# Conventions — luwu

## Code Style

No `rustfmt.toml` or `clippy.toml` present — the project uses default Rust formatting and lint rules.

- **Edition 2024** — uses `resolver = "3"`, latest Rust idioms.
- **Module organization**: each crate exposes its public API through `lib.rs` with `pub mod` declarations + `pub use` re-exports. Internal modules are private.
- **Separation of concerns**: section dividers with `// ---...` comment blocks to visually group related items within a file (see `engine.rs`, `config.rs`).
- **Doc comments**: `//!` module docs on every `lib.rs`, `///` on all public items.

## Error Handling

**Single unified error type** via `thiserror::Error`:

| Crate | Error Type | Location |
|-------|-----------|----------|
| `luwu-core` | `LuwuError` | `core/src/error.rs` |
| `luwu-server` | `ConfigError` | `server/src/config.rs` |

`LuwuError` covers all domains — `Llm`, `Tool`, `Storage`, `Session`, `Config`, `Io` (via `#[from]`), `Serde` (via `#[from]`). All public functions return `Result<T>` (the `luwu_core::Result` alias). Plugin crates downstream their own errors as `LuwuError::Tool(String)` or `LuwuError::Llm(String)`.

Config errors are a separate enum (`ConfigError`) — not folded into `LuwuError`, because config loading happens before the core is initialized.

## Trait Patterns

All four core traits use `#[async_trait]` and are `Send + Sync`:

| Trait | Async | Pattern |
|-------|-------|---------|
| `LlmProvider` | ✅ | `stream()` returns `mpsc::Receiver` (not a direct future) |
| `Tool` | ✅ | `execute()` is a direct async fn |
| `Storage` | ✅ | Direct async fn |
| `EventBus` | ❌ | `publish()` is sync; backed by `tokio::broadcast` |

## Naming Conventions

| Category | Convention | Example |
|----------|-----------|---------|
| Types | `PascalCase` | `TurnEngine`, `LlmProvider`, `CycleState` |
| Functions/methods | `snake_case` | `run_stream`, `detect_skill_reference` |
| Constants | `SCREAMING_SNAKE` | (none currently) |
| Modules | `snake_case` | `session_manager`, `tool_registry` |
| Files | `snake_case.rs` | one module per file |
| Generics | single-letter or descriptive | `T`, `Provider` |

## Async Patterns

- **Tokio everywhere** — `tokio = { features = ["full"] }`.
- **Streaming**: `tokio::sync::mpsc` channels for LLM event streams, `tokio::broadcast` for EventBus.
- **Cancellation**: `tokio::sync::watch` channel wrapped in `CancelToken`, checked at iteration boundaries.
- **Spawning**: `tokio::spawn` for the agent loop, memory workers, and checkpoint writers — all fire-and-forget.

## Clippy Suppressions

Only 3 `#[allow(...)]` annotations across the codebase:

| File | Annotation | Reason |
|------|-----------|--------|
| `server/src/api.rs:72` | `#[allow(dead_code)]` | Unused struct fields (OpenAI-compat response types) |
| `server/src/api.rs:301` | `#[allow(clippy::collapsible_if)]` | Readability preference |
| `tools/src/edit.rs:317` | `#[allow(clippy::too_many_arguments)]` | Intentional multi-arg function |

## Comment Language

- **Source code**: English (`//!` doc comments, inline comments).
- **System prompts** (`prompt/mod.rs`, `workers.rs`): Chinese — the Writer subagent prompt and checkpoint field labels are in Chinese.
- **Design docs** (`docs/design/`): Chinese.
- **User-facing output** (`main.rs`): Chinese with ANSI color codes (`\x1b[2m...\x1b[0m`).

## File Organization

| Pattern | Detail |
|---------|--------|
| One concept per file | `session.rs` vs `session_manager.rs` vs `storage.rs` — distinct concerns |
| Entry points at crate root | `lib.rs` declares modules, re-exports public API |
| Prompt builder in sub-module | `prompt/` directory (only `mod.rs`) for system prompt construction |
| Tools are flat | Each tool is a single file in `luwu-tools/src/` — no nested directories |

## Logging

`tracing` crate with `tracing-subscriber` (`EnvFilter`). Log levels:

- `info!`: lifecycle events (server start, session created, turn started)
- `debug!`: internal state transitions
- `warn!`: recoverable failures (skill discovery failed, FTS5 unavailable)
- `error!`: unrecoverable failures (not commonly used — most errors propagate via `Result`)

Default filter: `info`. Override with `RUST_LOG=luwu_core=debug,luwu_server=trace`.

## Anti-Patterns Avoided

- **No `unwrap()` in production paths** — startup uses `unwrap()` only for `TcpListener::bind` (fatal if it fails). All other paths use `?` or `Result`.
- **No `Box<dyn Error>`** — all errors are strongly typed via `thiserror`.
- **No global state** — `AppState` is passed explicitly through axum's `State` extractor.
- **No shared mutable state across sessions** — each `agent_chat` builds a fresh `TurnEngine` + `ToolRegistry` + `MemoryStore`.
