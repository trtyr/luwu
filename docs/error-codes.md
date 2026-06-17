# Luwu Error Code Map

> Last updated: 2026-06 — generated from `crates/luwu-core/src/error.rs`.

This document maps every `LuwuError` variant to its HTTP status, retry
strategy, and the typical user-facing message. Frontends (TUI / web)
should consult this table when deciding whether to surface, retry, or
silently log an error.

## Variant reference

| Variant | HTTP status | Retry? | Typical cause | User-facing message template |
|---|---|---|---|---|
| `Llm(String)` | 500 / 502 / 503 / 504 | yes (retry.rs handles 429/5xx) | Upstream provider error, network failure, SSE stall, rate limit, auth failure | "LLM error: {detail}" |
| `Tool(String)` | 500 | no | Tool implementation bug, invalid input that passed schema, runtime exception | "Tool error: {tool}: {detail}" |
| `Storage(String)` | 500 | no | SQLite corruption, file system error, FTS5 failure | "Storage error: {detail}" |
| `Session(String)` | 404 / 409 | no | Session not found, concurrent turn conflict (`try_set_running` AlreadyRunning), TOCTOU | "Session error: {detail}" |
| `Config(String)` | 500 | no | Invalid config.toml (empty api_key, bad base_url, out-of-range temperature/max_tokens) | "Config error: {detail}" |
| `Io(io::Error)` | varies (500 default) | depends on kind | File not found, permission denied, disk full | "I/O error: {detail}" |
| `Serde(serde_json::Error)` | 500 | no | Malformed JSON in request body, schema mismatch | "Serialization error: {detail}" |

## Retry policy

`luwu-llm/src/retry.rs` implements exponential backoff (1s/2s/4s +
jitter, max 3 attempts) for `Llm` errors classified as:

- HTTP 429 (rate limited) — respects `Retry-After` header
- HTTP 500 / 502 / 503 / 504
- Connection / timeout errors

**All other error variants are not retried** — they indicate
deterministic failures (config, schema, missing session) that
retrying won't fix.

## SSE stream retry

As of commit 05b999d, the OpenAI and Anthropic providers also retry
on **mid-stream stall** (no event for `STALL_TIMEOUT_SECS = 60`):

- `StalledNoData` (no bytes received): retried up to 3 times with
  1s/2s backoff.
- `StalledPartial` (some bytes received): NOT retried — can't safely
  resume from byte offset.

## Mapping to HTTP status (luwu-server)

In `crates/luwu-server/src/error.rs`, the `ApiError::from(LuwuError)`
impl applies these mappings:

```
LuwuError::Session(_)     -> ApiError::NotFound (404) or Conflict (409)
LuwuError::Config(_)      -> ApiError::BadRequest (400)
everything else            -> ApiError::Internal (500)
```

## Per-crate errors

Lower-level crates define their own scoped error enums for clearer
error handling at the boundary:

| Crate | Error enum | Wrapped into LuwuError via |
|---|---|---|
| `luwu-llm` | `LlmError` (Http, Status, Json, Stream, Auth, Timeout) | `From<LlmError> for LuwuError` |
| `luwu-tools` | `ToolError` (Io, Parse, InvalidInput, External) | `From<ToolError> for LuwuError` |
| `luwu-server` | `ApiError` (NotFound, BadRequest, Conflict, Internal) | `IntoResponse` (does HTTP) |

Each per-crate error has its own `tracing` log entry on construction
so the original context is preserved in logs even after the
`From` conversion flattens the variant.

## Error response body sanitization

LLM error bodies (e.g. cloud provider response text) are truncated to
**500 chars** before being included in the error string. This prevents
leaking multi-KB HTML error pages from upstream providers into client
logs. See `truncate_body()` in `luwu-llm/src/error.rs`.

## When in doubt

- **Llm errors are usually transient** — retry with backoff.
- **Session errors are usually permanent** — ask the user to verify
  the session ID or restart.
- **Config errors are bugs** — surface them loudly, the user must
  fix their `~/.luwu/config.toml`.
- **Tool errors are tool-specific** — log with full context so the
  user can file a useful bug report.
