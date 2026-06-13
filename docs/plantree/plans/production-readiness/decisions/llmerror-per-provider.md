# Decision: LlmError — Per-Provider in luwu-llm

## Decision

`LlmError` enum lives in `luwu-llm/src/error.rs` (per-provider crate), not in `luwu-core`.

## Rationale

Each provider maps HTTP/JSON/stream errors differently. A core enum would force every provider to fit a one-size-fits-all taxonomy. Per-provider gives flexibility — OpenAiProvider and AnthropicProvider each classify errors naturally.

The `From<LlmError> for LuwuError` impl provides the bridge — consumers see `LuwuError::Llm(...)` which is backward-compatible.

## Tradeoff

Consumers can't pattern-match on specific LLM error types without importing LlmError. But the only consumer is `engine.rs` which treats all LLM errors as "stream failed" anyway, so the loss of granularity is theoretical.

## Implementation

`crates/luwu-llm/src/error.rs` — LlmError with variants: Http, Status{status, body}, Json, Stream, Auth, Timeout. truncate_body helper sanitizes response bodies to 500 chars.

## Supersedes

Resolves [Q2](../open-questions.md#q2-should-llmerror-be-a-core-trait-or-per-provider).
