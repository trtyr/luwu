# Decision: SQLite — spawn_blocking, Not Pool

## Decision

Wrap fire-and-forget `index_entry` calls in `tokio::task::spawn_blocking`. Keep `search()` synchronous. Do not add a connection pool (deadpool-sqlite, r2d2-sqlite) or switch to sqlx.

## Rationale

SQLite ops are sub-millisecond for a single-user local agent with a small memory index. The `Mutex<Connection>` blocks the tokio worker thread for microseconds — imperceptible. `spawn_blocking` moves the 3 fire-and-forget index writes off the async runtime entirely, which is sufficient.

Making `search()` async would cascade through all sync `store.rs` methods and their callers — a massive refactor for zero perceivable benefit.

## Tradeoff

If concurrent multi-session usage is ever planned, a connection pool would be needed. But luwu is a local single-user agent — that scenario is not on the roadmap.

## Implementation

Phase 4.2 (commit `42f7faa`): SearchIndex changed from `Mutex<Connection>` to `Arc<Mutex<Connection>>` with Clone derive. 3 `index_entry` call sites in store.rs wrapped in `tokio::task::spawn_blocking`. `search()` stays sync.

## Supersedes

Resolves [Q5](../open-questions.md#q5-sqlite--keep-mutexconnection-or-move-to-connection-pool).
