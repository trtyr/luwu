# Decision: Storage Trait — Removed

## Decision

Delete the `Storage` trait entirely. It has zero implementations and zero users.

## Rationale

`Storage` was defined in `luwu-core/src/storage.rs` (27 lines) and exported from `lib.rs`, but `SessionManager` does its own file persistence directly. The trait was aspirational — designed for a future where session persistence might be pluggable — but never wired.

Keeping dead code rots the codebase and confuses new contributors into thinking there's a pluggable storage layer when there isn't.

## Tradeoff

If pluggable storage is ever needed, the trait would need to be re-designed from scratch anyway — session persistence requirements have evolved (checkpoints, history JSONL, search index) far beyond what the original Storage trait modeled.

## Implementation

Deleted in Phase 6.1 (commit `911b0c8`): storage.rs removed, lib.rs exports cleaned, LuwuError::Storage variant remains (used by SessionManager errors).

## Supersedes

Resolves [Q3](../open-questions.md#q3-storage-trait--implement-or-remove).
