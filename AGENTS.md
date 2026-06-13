# 陆吾 (Luwu)

> Rust AI agent framework with a microkernel design — core defines traits, everything else is a plugin.

## Project Type

backend-only

## Quick Reference

| What | Value |
|------|-------|
| Language | Rust (edition 2024) |
| Framework | axum 0.8 |
| Runtime | tokio (async) |
| Package Manager | cargo (workspace, resolver 3) |
| Entry Point | `crates/luwu-server/src/main.rs` |
| Test Command | `cargo test` (Rust) / `uv run --with httpx --with openai python3 tests/test_api.py` (Python E2E) |
| Dev Command | `cargo run -p luwu-server` |
| Database | SQLite (rusqlite 0.37, bundled, FTS5 search index) |
| Network Protocols | HTTP (axum), SSE streaming, outbound HTTPS to LLM APIs |
| Server Port | 127.0.0.1:51740 |

## Overview

Luwu is an LLM-powered coding agent server. It exposes an OpenAI-compatible HTTP API plus a richer agent chat endpoint with full tool-calling visibility, streaming events, and a persistent four-layer memory system. The server manages sessions, discovers skills (Agent Skills standard), and orchestrates an agentic loop (TurnEngine) that can call tools iteratively — bash, file operations, grep, web fetch, and memory search.

## Architecture Overview

Microkernel workspace: `luwu-core` defines four trait boundaries (`LlmProvider`, `Tool`, `Storage`, `EventBus`) and the `TurnEngine` agent loop. Concrete implementations live in separate crates — `luwu-llm` (OpenAI/Anthropic providers), `luwu-tools` (8 built-in tools), `luwu-memory` (SQLite + filesystem memory). `luwu-server` wires everything together behind an axum HTTP API. Dependencies flow inward: nothing depends on `luwu-server`, everything depends on `luwu-core`.

Two chat paths: `/v1/chat/completions` (OpenAI-compatible, no tools visible) and `/v1/sessions/{id}/chat` (full agent mode with tool calls, cycle management, and memory workers). Long sessions use CycleState — checkpoint at 20/45/70% token budget, rebuild at 90%.

→ [Full architecture analysis](docs/context/architecture.md)

## Key Modules

| Module | Purpose | Details |
|--------|---------|---------|
| `luwu-core` | Traits, types, engine, session/skill management | [→ modules.md](docs/context/modules.md) |
| `luwu-llm` | LLM provider implementations (OpenAI, Anthropic) | [→ modules.md](docs/context/modules.md) |
| `luwu-tools` | Built-in tools (bash, read, write, edit, grep, web_fetch, memory_search) | [→ modules.md](docs/context/modules.md) |
| `luwu-server` | HTTP API, config, startup, memory worker orchestration | [→ modules.md](docs/context/modules.md) |
| `luwu-memory` | Persistent memory (filesystem + SQLite FTS5, checkpoints, history) | [→ modules.md](docs/context/modules.md) |

## Tech Stack

| Category | Choice | Version |
|----------|--------|---------|
| Language | Rust | edition 2024 |
| HTTP Framework | axum | 0.8 |
| Async Runtime | tokio | 1 (full) |
| Database | SQLite (rusqlite, bundled) | 0.37 |
| HTTP Client | reqwest | 0.12 |
| Serialization | serde / serde_json / toml | 1 / 1 / 0.8 |
| Error Handling | thiserror | 2 |
| Logging | tracing / tracing-subscriber | 0.1 / 0.3 |
| Search | fff-search | 0.9 |
| HTML | kawat / mdka | 0.1.5 / 2.1.6 |

→ [Full dependency analysis](docs/context/tech-stack.md)

## Commands

```bash
cargo run -p luwu-server                    # Dev run
cargo build --release -p luwu-server        # Release build
cargo test                                  # Rust unit tests
cargo test -p luwu-core                     # Single crate tests
uv run --with httpx --with openai python3 tests/test_api.py   # Python API tests
RUST_LOG=debug cargo run -p luwu-server     # Debug logging
```

## Conventions

- Error handling: unified `LuwuError` via `thiserror`, all functions return `luwu_core::Result<T>`
- All core traits are `#[async_trait]` + `Send + Sync`
- Source comments in English; system prompts and user-facing output in Chinese
- One concept per file; `lib.rs` re-exports public API
- No `rustfmt.toml` or `clippy.toml` — defaults only
- Only 3 `#[allow(...)]` annotations in entire codebase

→ [Full conventions](docs/context/conventions.md)

## Public Interfaces

HTTP API on `127.0.0.1:51740`. Two surfaces: OpenAI-compatible `/v1/chat/completions` (SSE streaming) and agent-mode `/v1/sessions/{id}/chat` (full TurnEvent stream with tool visibility). Session CRUD, memory checkpoint/history, and skill discovery endpoints.

→ [Full API reference](docs/context/api.md)

## Database

SQLite (via `rusqlite` with `bundled` feature) used as a **search index only** — the filesystem (Markdown + JSONL under `~/.luwu/`) is the source of truth. Single FTS5 virtual table (`memory_fts`) for cross-layer full-text search with CJK pre-tokenization. If `search.db` is deleted, it rebuilds from originals.

→ [Full database analysis](docs/context/database.md)

## Gotchas

- **Server binds to localhost only** (127.0.0.1:51740) — not designed for direct internet exposure.
- **Anthropic provider implemented but not wired** — `agent_chat` always uses `OpenAiProvider` with configurable base URL.
- **Python tests require a running server** — they're not pytest, each file has a custom `@test` decorator + `main()` runner.
- **No CI pipeline** — no `.github/workflows/` exists.
- **Config at `~/.luwu/config.toml`** — server exits if no default provider resolves.
- **`serde_yaml` is deprecated** but still functional in the dependency tree.
- **Two versions of reqwest coexist** — 0.12 (direct) and 0.13 (via kawat).

---

*Generated by /init-local on 2025-06-14.*
