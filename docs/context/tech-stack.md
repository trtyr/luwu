# luwu — Tech Stack

> Generated from `Cargo.toml`, `Cargo.lock`, and project config files.
> Versions reflect the locked state at time of writing.

## Language & Runtime

| Item | Value |
|---|---|
| Language | Rust |
| Edition | 2024 |
| Workspace resolver | 3 |
| Lock file version | 4 (v4 format) |
| Project version | 0.1.0 |
| License | MIT |

No `rust-toolchain.toml` present — toolchain is whatever satisfies edition 2024
(rustc ≥ 1.85 stable).

## Workspace Crates

| Crate | Path | Role | Internal deps |
|---|---|---|---|
| `luwu-core` | `crates/luwu-core` | Microkernel: traits, types, event bus | — |
| `luwu-memory` | `crates/luwu-memory` | Persistent memory (SQLite) | `luwu-core` |
| `luwu-llm` | `crates/luwu-llm` | LLM provider implementations | `luwu-core` |
| `luwu-tools` | `crates/luwu-tools` | Built-in tool implementations | `luwu-core`, `luwu-memory` |
| `luwu-server` | `crates/luwu-server` | HTTP server / API layer | `luwu-core`, `luwu-llm`, `luwu-tools`, `luwu-memory` |

All crates share `version.workspace = true` (0.1.0) and `edition.workspace = true`.

## Direct Dependencies — Workspace-Level

Shared via `[workspace.dependencies]`, consumed by crates as `{ workspace = true }`.

### Serialization & Data

| Crate | Constraint | Locked | Features |
|---|---|---|---|
| `serde` | `1` | 1.0.228 | `derive` |
| `serde_json` | `1` | 1.0.150 | — |
| `serde_yml` | `0.0` | 0.0.12 | — |
| `toml` | `0.8` | 0.8.23 | — |

### Async Runtime & Concurrency

| Crate | Constraint | Locked | Features |
|---|---|---|---|
| `tokio` | `1` | 1.52.3 | `full` |
| `tokio-stream` | `0.1` | 0.1.18 | — |
| `futures` | `0.3` | 0.3.32 | — |
| `async-trait` | `0.1` | 0.1.89 | — |
| `async-stream` | `0.3` | 0.3.6 | — |

### HTTP / Web Framework

| Crate | Constraint | Locked | Features |
|---|---|---|---|
| `axum` | `0.8` | 0.8.9 | — |
| `tower` | `0.5` | 0.5.3 | — |
| `tower-http` | `0.6` | 0.6.11 | `cors` |
| `reqwest` | `0.12` | 0.12.28 | `stream`, `json` |

### Observability & Error Handling

| Crate | Constraint | Locked | Features |
|---|---|---|---|
| `tracing` | `0.1` | 0.1.44 | — |
| `tracing-subscriber` | `0.3` | 0.3.23 | `env-filter` |
| `thiserror` | `2` | 2.0.18 | — |

### Utilities

| Crate | Constraint | Locked | Features |
|---|---|---|---|
| `uuid` | `1` | 1.23.3 | `v4`, `serde` |
| `chrono` | `0.4` | 0.4.45 | `serde` |
| `dirs` | `6` | 6.0.0 | — |
| `fff-search` | `0.9` | 0.9.4 | — |

## Direct Dependencies — Crate-Specific

Declared in individual crate `Cargo.toml`, not in `[workspace.dependencies]`.

| Crate | Used by | Constraint | Locked | Features |
|---|---|---|---|---|
| `rusqlite` | `luwu-memory` | `0.37` | 0.37.0 | `bundled` |
| `regex` | `luwu-tools` | `1` | 1.12.4 | — |
| `rayon` | `luwu-tools` | `1.10` | 1.12.0 | — |
| `kawat` | `luwu-tools` | `0.1.5` | 0.1.5 | — |
| `mdka` | `luwu-tools` | `2.1.6` | 2.1.6 | — |

## Dev Dependencies

No `[dev-dependencies]` declared in any crate or at workspace level.

## Build Tools & Linting

| Tool | Config | Notes |
|---|---|---|
| Cargo | `Cargo.toml` (workspace) | Build, test runner; resolver 3 |
| rustfmt | none (`rustfmt.toml` absent) | Default formatting only |
| clippy | none (`clippy.toml` absent) | Default lint rules |

No `Makefile`, `justfile`, `xtask/`, or CI config (`.github/`) present.

## Python Test Suite

Integration / E2E tests live in `tests/` as standalone Python scripts (no `pytest`).

| File | Description |
|---|---|
| `tests/test_api.py` | API endpoint tests via OpenAI SDK |
| `tests/e2e_test.py` | End-to-end server test via OpenAI SDK |
| `tests/test_e2e.py` | Full agent API capabilities test |
| `tests/test_hashline.py` | Hash-line feature test |
| `tests/test_tools.py` | Tool execution tests |

Python runtime deps (inferred from imports, no `requirements.txt` / `pyproject.toml`):

| Package | Usage |
|---|---|
| `openai` | OpenAI SDK client for API calls |
| `httpx` | HTTP client for raw endpoint testing |

## Notable Transitive Dependencies

These are pulled in indirectly but are significant in size or function.

| Crate | Locked | Pulled by | Why notable |
|---|---|---|---|
| `hyper` | 1.10.1 | `axum`, `reqwest` | HTTP/1+2 engine under axum |
| `rustls` | 0.23.40 | `reqwest` | TLS implementation (AWS LC provider) |
| `aws-lc-rs` | 1.17.0 | `rustls` | C crypto, compiled via `cmake` |
| `reqwest` (0.13.4) | 0.13.4 | `kawat` | **Second major** — kawat uses reqwest 0.13 |
| `libsqlite3-sys` | 0.35.0 | `rusqlite` (`bundled`) | SQLite C source compiled in |
| `heed` | 0.22.1 | `fff-search` | LMDB key-value store |
| `lmdb-master-sys` | 0.2.6 | `heed` | LMDB C bindings |
| `git2` | 0.20.4 | `fff-search` | libgit2 bindings |
| `libgit2-sys` | 0.18.5+1.9.4 | `git2` | libgit2 C source compiled in |
| `blake3` | 1.8.5 | `fff-search` | Fast hashing for file indexing |
| `lol_html` | 2.9.0 | `kawat-html` | Streaming HTML rewriter |
| `scraper` | 0.27.0 | `kawat-*` crates | HTML parsing / CSS selectors |
| `quinn` | 0.11.9 | `reqwest` 0.13.4 | HTTP/3 via QUIC |
| `matchit` | 0.8.4 | `axum` | URL path router |

## Version Constraint Notes

- `serde_yml` 0.0.12 is the active drop-in fork of the deprecated `serde_yaml` 0.9 — workspace migrated to it (commit `9f01477`).
- Two major versions of `reqwest` coexist: **0.12.28** (direct dep) and **0.13.4** (via `kawat`).
- Two major versions of `thiserror` coexist: **2.0.18** (direct dep) and **1.0.69** (via `redox_users`).
- `dirs` has two versions locked: **6.0.0** (direct) and **5.0.1** (via `fff-search`).
- `rusqlite` uses `bundled` feature — SQLite is compiled from C source at build time, no system SQLite needed.
- `fff-search` (0.9.4) is a workspace of crates (`fff-grep`, `fff-notify-debouncer-full`, `fff-query-parser`) at the same version.
- `kawat` (0.1.5) is a workspace of 9 sub-crates (`kawat-core`, `kawat-dedup`, `kawat-extract`, etc.) all at 0.1.5.
