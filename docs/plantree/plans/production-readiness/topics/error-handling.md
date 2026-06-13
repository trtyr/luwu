# Topic: Error Handling

## Problem

fuck-u-code error_handling score: **55/100** (average). Worst files:

| File | Score | Ignored Rate |
| ---- | ----- | ------------ |
| `crates/luwu-tools/src/edit.rs` | 0/100 | 100% (5/5) |
| `crates/luwu-llm/src/openai.rs` | 1.2/100 | 100% (10/10) |
| `crates/luwu-tools/src/web_fetch.rs` | 4.3/100 | 75% (3/4) |
| `crates/luwu-server/src/api.rs` | 3.1/100 | 81.6% (31/38) |
| `crates/luwu-core/src/engine.rs` | 5.3/100 | 94.7% (18/19) |
| `crates/luwu-llm/src/anthropic.rs` | 11.1/100 | 88.9% (8/9) |

The default pattern across every crate: `let _ = result.ok()` or `.unwrap()`.

## Strategy

### Step 1: Per-crate error enums

Use `thiserror` (already a workspace dep). One error enum per crate:

- `luwu-core`: Expand existing `LuwuError` with structured variants (not just `Llm(String)`)
- `luwu-llm`: `LlmError { Http(reqwest::Error), Status(u16, String), Json(serde_json::Error), Stream(String) }`
- `luwu-tools`: `ToolError { Io, Parse, InvalidInput, External }` — tools currently return `Option::None` on failure
- `luwu-server`: `ApiError` implementing `IntoResponse` — maps to HTTP status codes

### Step 2: Silent-drop audit

For each file, replace patterns:
- `let _ = result.ok();` → `if let Err(e) = result { tracing::warn!(?e, "context"); }`
- `.unwrap()` on non-infallible → `?` or `.map_err(|e| ...)?`
- `.expect("msg")` on startup → keep with better message OR convert to `?`

Priority order: `api.rs` → `engine.rs` → `openai.rs` + `anthropic.rs` → `edit.rs` + `web_fetch.rs`

### Step 3: Error sanitization

`openai.rs:96-101`: full API response body in error string. Truncate to 500 chars, redact potential secrets.

## Constraints

- Don't break the `LlmProvider` / `Tool` trait signatures in Phase 2 — that's Phase 5 scope.
- `LuwuError` must remain backward-compatible until all call sites are updated.
- Workers in `api.rs` use raw HTTP — error types for those arrive in Phase 5 (after refactor).

## Dependencies

- Resolves: [Q2](../open-questions.md#q2-should-llmerror-be-a-core-trait-or-per-provider) (error enum placement)
- Blocks: Phase 3 (retry layer needs classified errors)
