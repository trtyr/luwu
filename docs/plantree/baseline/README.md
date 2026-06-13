# Baseline

Project-wide context for planning. Links to `docs/context/` instead of duplicating.

## Source Documents

| Document | Location | Content |
| -------- | -------- | ------- |
| Architecture | [docs/context/architecture.md](../../context/architecture.md) | Crate dependency graph, 4 core traits, TurnEngine loop, CycleState, 4-layer memory |
| Modules | [docs/context/modules.md](../../context/modules.md) | Per-module reference for all 5 crates |
| Tech Stack | [docs/context/tech-stack.md](../../context/tech-stack.md) | Rust edition 2024, locked deps, notable transitives |
| Conventions | [docs/context/conventions.md](../../context/conventions.md) | Code style, error handling, naming patterns |
| API | [docs/context/api.md](../../context/api.md) | 13 HTTP endpoints, two chat paths, SSE event types |
| Deploy | [docs/context/deploy.md](../../context/deploy.md) | Build commands, config, startup sequence, data dir |
| Database | [docs/context/database.md](../../context/database.md) | SQLite FTS5 as mirror, filesystem as source of truth |

## Review Results

| Document | Location | Overall |
| -------- | -------- | ------- |
| Summary | [docs/review/summary.md](../../review/summary.md) | **C+** (85/100 quant, B+ arch, C- runtime, D network) |
| Code Quality | [docs/review/code-quality.md](../../review/code-quality.md) | AI review of 10 worst-scoring files |
| Architecture | [docs/review/architecture.md](../../review/architecture.md) | B+ — microkernel solid, api.rs is god module |
| Runtime Health | [docs/review/runtime-health.md](../../review/runtime-health.md) | C- — no shutdown, no retry, no timeouts |
| Network Resilience | [docs/review/network-resilience.md](../../review/network-resilience.md) | D — no reconnection, no error classification |

## Risk Hotspots

See [risk-hotspots.md](risk-hotspots.md) for the prioritized list of systemic issues from the review.
