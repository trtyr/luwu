# Topic: Concurrency

## Problem

| Issue | Score | Evidence |
| ----- | ----- | -------- |
| TOCTOU race in agent_chat | D | `api.rs:642-670` |
| Sync I/O in async locks | C | `session_manager.rs:268-284` |
| SQLite Mutex blocking | C | `search_index.rs:24` |
| Fire-and-forget tasks | C | `api.rs:721,756,773,799,832,858` |

## TOCTOU Race Fix (Phase 1.3 — P0)

**Current flow:**
```
1. state.sessions.get(&id).await     // read lock, clone, release
2. check session.is_running          // on stale clone
3. state.sessions.set_running(&id, true)  // NEW write lock
```

Two concurrent POSTs both pass step 2 before either reaches step 3.

**Fix:** Atomic check-and-set in `SessionManager`:

```rust
// session_manager.rs
pub async fn try_set_running(&self, id: &str) -> Result<(), ()> {
    let mut sessions = self.sessions.write().await;
    let session = sessions.get_mut(id).ok_or(())?;
    if session.is_running {
        return Err(());  // already running → caller returns 409
    }
    session.is_running = true;
    Ok(())
}
```

Caller in `api.rs`:
```rust
match state.sessions.try_set_running(&id).await {
    Ok(()) => { /* proceed */ }
    Err(()) => { return StatusCode::CONFLICT.into_response(); }
}
```

## Sync I/O Fix (Phase 4.1)

**Current:** `session_manager.rs:268-284` calls `std::fs::write` while holding `RwLock` write guard.

**Fix:** Use `tokio::fs::write` (async), or spawn to blocking pool:

```rust
// Option A: tokio::fs (simple)
tokio::fs::write(&self.path, &data).await
    .map_err(|e| tracing::warn!(?e, "persist failed"))?;

// Option B: spawn_blocking (if we need std::fs for some reason)
tokio::task::spawn_blocking(move || {
    std::fs::write(&path, &data)
}).await.map_err(|e| ...)??;
```

Option A is preferred — simpler, and `tokio::fs` uses a thread pool internally.

## SQLite Blocking Fix (Phase 4.2)

**Current:** `search_index.rs:24` — `Mutex<Connection>` (std). Lock held during synchronous SQLite ops.

**Fix:** Wrap ops in `spawn_blocking`:

```rust
// search_index.rs
pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, DbError> {
    let conn = self.conn.clone();  // Arc<Mutex<Connection>>
    let query = query.to_string();
    tokio::task::spawn_blocking(move || {
        let conn = conn.lock().unwrap();
        // ... synchronous SQLite query
    }).await.map_err(|e| DbError::Join(e))?
}
```

**Dependency:** See [Q5](../open-questions.md#q5-sqlite--keep-mutexconnection-or-move-to-connection-pool).

## Session Reset Guard (Phase 4.3)

**Current:** `is_running` reset to `false` only at `api.rs:879,938` — inside the stream handler. If stream is killed (cancel, client disconnect, shutdown), flag stays `true`.

**Fix:** RAII guard that resets on drop:

```rust
struct RunningGuard<'a> {
    sessions: &'a SessionManager,
    id: String,
}

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        // spawn async reset since Drop can't be async
        let sessions = self.sessions.clone();
        let id = self.id.clone();
        tokio::spawn(async move {
            sessions.set_running(&id, false).await;
        });
    }
}
```

Or use a structured concurrency pattern with an `async fn` + `finally`-style cleanup.

## Constraints

- `SessionManager` lock scope must be minimized — check-and-set should be one atomic lock acquisition.
- SQLite fix should not introduce connection pooling complexity unless [Q5](../open-questions.md) resolves in favor of a pool.
- RunningGuard's `Drop` spawns a task — ensure this doesn't race with shutdown.
