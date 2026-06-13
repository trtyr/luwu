# Open Questions

Unresolved decisions. Move to `decisions/` when resolved.

## Q1: retry crate choice — `tokio-retry` vs `backon` vs hand-rolled

**Context:** Phase 3.3 needs exponential backoff + jitter. `tokio-retry` is popular but maintenance-stale; `backon` is newer and async-first. Hand-rolled is ~30 lines.

**Question:** Which approach?

**Status:** Open. Lean toward `backon` (lightweight, no-macro API), but verify maintenance status before committing.

---

## Q2: Should `LlmError` be a core trait or per-provider?

**Context:** Phase 3.2 needs network error classification. Currently `LuwuError::Llm(String)` flattens everything.

**Question:** Define `LlmError` enum in `luwu-core` (shared by all providers) or let each provider define its own and map at the trait boundary?

**Status:** Open. Core enum is simpler for consumers; per-provider is more flexible. Lean toward core enum with `#[non_exhaustive]`.

---

## Q3: `Storage` trait — implement or remove?

**Context:** Phase 6.1. `Storage` trait is defined in `storage.rs:14`, exported in `lib.rs:41`, but never implemented. `SessionManager` does its own file persistence directly.

**Question:** Is this aspirational (keep + implement) or abandoned (remove)?

**Status:** Open. Needs a decision before Phase 6.

---

## Q4: Should api.rs split happen before or after error handling overhaul?

**Context:** Phases 2 and 5 both touch api.rs heavily. Doing error handling first means working in the 1380-line file; doing the split first means moving code around before fixing it.

**Question:** Reorder phases (split before error) or keep current order?

**Status:** Open. Lean toward keeping current order (fix errors first — correctness > structure), but the split would make Phase 2 easier to parallelize.

---

## Q5: SQLite — keep `Mutex<Connection>` or move to connection pool?

**Context:** Phase 4.2. Current pattern is a single `Mutex<Connection>` (std blocking). Options: (a) wrap ops in `spawn_blocking`, (b) use `deadpool-sqlite` or `r2d2-sqlite`, (c) switch to `sqlx` with async support.

**Question:** How much SQLite infrastructure to add?

**Status:** Open. For single-user local agent, `spawn_blocking` wrapping is likely sufficient. Pool only if concurrent multi-session usage is planned.
