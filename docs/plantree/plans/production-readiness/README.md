# Plan: Production Readiness

Lift luwu from **C+** to **B+** by addressing the systemic gaps identified in the [review](../../../review/summary.md).

## Scope

**In scope:** Error handling, runtime resilience (shutdown, timeout, retry), concurrency fixes, connection pooling, api.rs refactor, dead abstraction cleanup.

**Out of scope:** New features, UI, provider implementations beyond wiring existing code, test framework changes (unless blocking).

## Target Grades

| Dimension | Current | Target | What it takes |
| --------- | ------- | ------ | ------------- |
| Error Handling | 55/100 (F) | 80+ (B) | Per-crate error enums, replace 100+ silent drops |
| Runtime Health | C- | B | Graceful shutdown, timeouts, task tracking, config validation |
| Network Resilience | D | B- | Retry layer, error classification, shared client, SSE liveness |
| Architecture | B+ | A | Split api.rs, route workers through trait, wire dead abstractions |
| Overall | C+ | B+ | All of the above |

## File Map

| File | Purpose |
| ---- | ------- |
| [roadmap.md](roadmap.md) | Phased plan with status (Done / In Progress / Next / Deferred) |
| [open-questions.md](open-questions.md) | Unresolved decisions |
| [topics/error-handling.md](topics/error-handling.md) | Error strategy, per-crate enums, silent-drop audit |
| [topics/runtime-resilience.md](topics/runtime-resilience.md) | Shutdown, timeout, retry, task lifecycle |
| [topics/concurrency.md](topics/concurrency.md) | TOCTOU race, sync I/O, fire-and-forget tasks |
| [topics/api-refactor.md](topics/api-refactor.md) | Split api.rs, route workers through LlmProvider, dead abstractions |
| [decisions/](decisions/) | Stable decision records |

## Reading Path

1. This file (scope + targets)
2. [roadmap.md](roadmap.md) (phases + status)
3. Topic files for the active phase
4. [open-questions.md](open-questions.md) for unresolved items
