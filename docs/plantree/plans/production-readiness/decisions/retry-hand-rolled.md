# Decision: Retry — Hand-Rolled (~30 lines)

## Decision

Hand-rolled exponential backoff retry. No `tokio-retry`, no `backon` dependency.

## Rationale

Full control over retry logic: 3 max attempts on 429/5xx and connection/timeout errors, backoff 1s/2s/4s capped at 4s + 0-500ms jitter, respects Retry-After header on 429. ~30 lines of code, zero new deps, trivially auditable.

## Tradeoff

Hand-rolled means we maintain it ourselves. But the logic is simple enough that maintenance burden is near zero — it's a loop with sleep and status-code matching, not a framework.

## Implementation

`crates/luwu-llm/src/retry.rs` — `send_with_retry(&RequestBuilder)` wired into both OpenAiProvider and AnthropicProvider `stream()` methods.

## Supersedes

Resolves [Q1](../open-questions.md#q1-retry-crate-choice--tokio-retry-vs-backon-vs-hand-rolled).
