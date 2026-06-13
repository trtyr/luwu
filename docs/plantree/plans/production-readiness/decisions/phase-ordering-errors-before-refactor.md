# Decision: Phase Ordering — Errors Before Refactor

## Decision

Keep current roadmap order: Phase 2 (error handling) before Phase 5 (api.rs split).

## Rationale

Correctness > structure. Fixing errors in a large file is annoying but safe. Moving code around first means refactoring broken code, then fixing errors, then re-verifying the moved code — extra churn for no gain.

The split in Phase 5 becomes cleaner when the error types already exist: `types.rs` and `error.rs` land together, handlers import a mature `ApiError` instead of a placeholder.

## Tradeoff

Working in the 1380-line `api.rs` during Phase 2 is unpleasant. The split would make Phase 2 easier to parallelize across files. But the risk of refactoring + error-fixing simultaneously is higher than the inconvenience of a large file.

## Supersedes

Resolves [Q4](../open-questions.md#q4-should-apirs-split-happen-before-or-after-error-handling-overhaul).
