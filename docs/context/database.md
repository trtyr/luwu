# Database Layer — luwu-memory

## Architecture Overview

luwu-memory uses a **hybrid storage model**: the filesystem is the source of truth (Markdown + JSONL files), and SQLite serves as a **mirror search index** only. SQLite is not a primary datastore — if `search.db` is deleted or corrupted, it is silently rebuilt from the Markdown/JSONL originals.

```
Source of Truth (filesystem)          Search Index (SQLite)
┌──────────────────────────┐          ┌──────────────────┐
│ global.md                │─────────▶│                  │
│ corrections.md           │─────────▶│  search.db       │
│ <hash>/project.md        │─────────▶│  (memory_fts     │
│ sessions/<id>/checkpoint │─────────▶│   FTS5 table)    │
│ sessions/<id>/notes.md   │─────────▶│                  │
└──────────────────────────┘          └──────────────────┘
          read/write                        write-on-index
```

The `SearchIndex` is optional — `MemoryStore` wraps it in `Option<SearchIndex>` and degrades gracefully (no search, but everything still works) if SQLite fails to open.

## Database Technology

| Property | Value |
|---|---|
| **Engine** | SQLite (via `rusqlite 0.37`, `bundled` feature — static-linked, no system SQLite dependency) |
| **Feature used** | FTS5 full-text search virtual table |
| **Tokenizer** | `unicode61` (default FTS5 tokenizer) |
| **CJK support** | Custom pre-tokenization — spaces inserted between CJK characters before indexing, because `unicode61` does not segment CJK words |
| **Role** | Search index only — not a primary datastore |

### Dependency (Cargo.toml)

```toml
rusqlite = { version = "0.37", features = ["bundled"] }
```

No other database drivers (no PostgreSQL, MySQL, Redis, or MongoDB).

## SQLite Schema

A single FTS5 virtual table. No standard tables exist.

### `memory_fts` (FTS5 virtual table)

| Column | Type | Indexed? | Description |
|---|---|---|---|
| `layer` | TEXT | **Yes** (FTS) | Memory layer tag: `global`, `project`, `correction`, `notes` |
| `content` | TEXT | **Yes** (FTS) | Tokenized content (CJK chars spaced out for matching) |
| `original` | TEXT | No (UNINDEXED) | Untouched original content — returned in search results |
| `session_id` | TEXT | No (UNINDEXED) | Session that produced this entry (empty string for global/project) |
| `timestamp` | TEXT | No (UNINDEXED) | RFC 3339 UTC timestamp of indexing |

**DDL** (executed on `SearchIndex::open`):

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    layer,
    content,
    original UNINDEXED,
    session_id UNINDEXED,
    timestamp UNINDEXED,
    tokenize='unicode61'
);
```

> **No migrations.** The schema is idempotent — `CREATE ... IF NOT EXISTS` runs on every open. There is no version tracking or migration system. If the table structure needs to change, the entire `search.db` is deleted and rebuilt.

## Filesystem Data Model (Source of Truth)

All persistent data lives under `~/.luwu/memory/`. The project directory is hashed (`DefaultHasher`, first 12 hex chars) to create an isolated namespace.

```
~/.luwu/memory/
├── global.md                  # Global memory (user preferences, cross-project)
├── corrections.md             # Correction entries (mistakes learned from)
├── search.db                  # SQLite FTS5 index (rebuildable)
└── <12-hex-hash>/             # Per-project namespace
    ├── project.md             # Project-specific knowledge
    └── sessions/
        └── <session_id>/
            ├── checkpoint.md          # Structured state snapshot (11 fields)
            ├── notes.md               # Main agent scratchpad (append-only)
            ├── history.jsonl          # Full conversation log
            ├── observations.jsonl     # Observer worker output
            └── reflections.jsonl      # Reflector worker output
```

### File Formats

| File | Format | Entry Delimiter | Aging Metadata |
|---|---|---|---|
| `global.md` | Markdown | `§` | `<!-- created: YYYY-MM-DD, ref: YYYY-MM-DD -->` |
| `project.md` | Markdown | `§` | Same as above |
| `corrections.md` | Markdown | `§` | Same as above |
| `checkpoint.md` | Markdown (structured) | `## section` headers | None |
| `notes.md` | Plain text | Newline | None |
| `history.jsonl` | JSON Lines | One JSON per line | Inline (`timestamp` field) |
| `observations.jsonl` | JSON Lines | One JSON per line | Inline (`timestamp` field) |
| `reflections.jsonl` | JSON Lines | One JSON per line | Inline (`timestamp` field) |

## Data Models & Relationships

No ORM is used. Rust structs are serialized directly to/from Markdown or JSONL.

### Core Structs

**`Checkpoint`** (checkpoint.md) — 11 structured fields, serialized as Markdown sections:

| Field | Markdown Header | Description |
|---|---|---|
| `current_intent` | `## 当前意图` | What the agent is doing right now |
| `next_action` | `## 下一步动作` | What to do immediately after rebuild |
| `constraints` | `## 工作约束` | User-requested rules and limits |
| `task_tree` | `## 任务树` | Goal → subtasks → progress |
| `current_work` | `## 当前工作` | Files/functions being processed |
| `involved_files` | `## 涉及文件` | Read/modified/pending file list |
| `discoveries` | `## 跨任务发现` | Architecture, API quirks, pitfalls |
| `errors_and_fixes` | `## 错误与修复` | Errors encountered and fixes applied |
| `runtime_state` | `## 运行时状态` | Branch, env vars, running processes |
| `design_decisions` | `## 设计决策` | Why A was chosen over B |
| `notes` | `## 杂项笔记` | Miscellaneous |

**`HistoryEntry`** (history.jsonl) — one line per message:

| Field | Type | Description |
|---|---|---|
| `timestamp` | String (RFC 3339) | When the entry was logged |
| `role` | String | `User`, `Assistant`, `ToolCall`, `ToolResult` |
| `content` | String (JSON) | Serialized message content |
| `tokens` | usize | Estimated token count |

**`Observation`** (observations.jsonl):

| Field | Type | Description |
|---|---|---|
| `id` | String (12-hex) | Unique identifier |
| `timestamp` | String (RFC 3339) | Creation time |
| `priority` | Enum (`high`/`medium`/`low`) | Importance level |
| `category` | String | `event`, `decision`, `preference`, `error`, `pattern` |
| `content` | String | The observation text |

**`Reflection`** (reflections.jsonl):

| Field | Type | Description |
|---|---|---|
| `id` | String (12-hex) | Unique identifier |
| `timestamp` | String (RFC 3339) | Creation time |
| `content` | String | Durable fact/pattern/constraint |
| `source_ids` | `Vec<String>` | **References Observation IDs** that led to this reflection |

### Relationships

```
┌──────────────┐         source_ids          ┌──────────────┐
│  Reflection  │─────── (references) ───────▶│ Observation  │
│ reflections  │     many-to-many (soft)      │ observations │
│   .jsonl     │                              │   .jsonl     │
└──────────────┘                              └──────────────┘

┌──────────────┐  owns   ┌──────────────┐
│ MemoryStore  │────────▶│ SearchIndex  │  (Optional — graceful degradation)
└──────────────┘         └──────────────┘

┌──────────────┐  owns   ┌──────────────┐
│ MemoryStore  │────────▶│ HistoryLog   │  (Opened on-demand per session)
└──────────────┘         └──────────────┘
```

> The Reflection → Observation link (`source_ids`) is a **soft reference** — it is not enforced at the data layer. If an observation is pruned by the Dropper worker, its ID remains in `source_ids` but has no matching entry.

## Query Patterns

### SQLite (SearchIndex)

All SQL is **raw, hand-written** — no query builder, no ORM. All queries use `params![]` parameter binding.

**Write — index an entry:**

```sql
INSERT INTO memory_fts (layer, content, original, session_id, timestamp)
VALUES (?1, ?2, ?3, ?4, ?5)
```

> Note: `content` is pre-tokenized via `tokenize_cjk()` (spaces between CJK chars); `original` holds the untouched text.

**Read — full-text search with BM25 ranking:**

```sql
SELECT layer, original, session_id, timestamp
FROM memory_fts
WHERE memory_fts MATCH ?1
ORDER BY bm25(memory_fts)
LIMIT ?2
```

> Query string is also pre-tokenized for CJK, then sanitized via `sanitize_query()` — CJK queries pass through directly; ASCII queries are wrapped in double quotes for exact phrase matching.

**Delete — clear session entries:**

```sql
DELETE FROM memory_fts WHERE session_id = ?1
```

**Delete — clear all:**

```sql
DELETE FROM memory_fts
```

### Filesystem (MemoryStore)

| Operation | Method | Pattern |
|---|---|---|
| Read global | `read_global()` / `read_global_clean()` | Full file read, optionally strip aging comments |
| Write global | `write_global()` | Full overwrite |
| Append global entry | `append_global_entry()` | Append with `<!-- created -->` metadata + `§` delimiter |
| Read project | `read_project()` / `read_project_clean()` | Same pattern as global |
| Write project | `write_project()` | Full overwrite |
| Append project entry | `append_project_entry()` | Same append pattern |
| Read checkpoint | `read_checkpoint()` | Parse Markdown → `Checkpoint` struct |
| Write checkpoint | `write_checkpoint()` / `write_checkpoint_raw()` | Struct → Markdown, or raw LLM output |
| Append notes | `append_notes()` | File append mode, one line per call |
| Append history | `append_history()` | Serialize `Message` → JSONL line, append |
| Search history | `search_history()` | Read all lines, reverse iterate, case-insensitive `contains` |
| Append observation | `append_observation()` | Serialize to JSON, append line |
| Append reflection | `append_reflection()` | Serialize to JSON, append line |
| Drop observations | `drop_observations()` | Read all → filter by ID set → full rewrite |
| Cross-layer search | `search_all()` | Read each `.md` file, split by `§`, case-insensitive `contains` match |
| Build context | `build_rebuild_context()` | Read all layers, concatenate with section headers |

> `search_all()` does **not** use the SQLite FTS5 index — it does a linear scan of Markdown files split by `§`. This is a fallback for when the FTS index is unavailable or for consistency across all layers.

## Connection Configuration

| Property | Value |
|---|---|
| **Connection count** | 1 per `SearchIndex` instance |
| **Pool** | None — single connection |
| **Concurrency** | `Mutex<Connection>` — all access serialized |
| **Timeout** | Default SQLite (no explicit busy timeout set) |
| **WAL mode** | Not enabled (default rollback journal) |
| **DB path** | `~/.luwu/memory/search.db` |
| **Auto-create dirs** | Yes — `create_dir_all` on parent before opening |

The connection is created in `SearchIndex::open()` and held for the lifetime of the `MemoryStore`. There is no reconnection logic — if the connection drops, the index is simply unavailable.

## Indexes & Performance

### FTS5 Internal Index

FTS5 maintains its own internal B-tree and posting lists. No manual index creation is needed or possible — the virtual table IS the index.

**BM25 ranking** is used for result ordering. Lower BM25 scores = better matches (FTS5 convention — `ORDER BY bm25(memory_fts)` returns best first).

### CJK Tokenization

The `tokenize_cjk()` function inserts spaces between CJK characters (U+4E00–U+9FFF Chinese, U+3040–U+30FF Japanese kana) before insertion and before querying. This is necessary because the `unicode61` tokenizer treats CJK as continuous strings without word boundaries.

```
Input:  "用户偏好使用pnpm"
Stored: "用 户 偏 好 使 用 pnpm"   (content column)
Stored: "用户偏好使用pnpm"          (original column — returned as-is)
```

### Consolidation (File Growth Control)

When Markdown memory files exceed **8,000 characters** (default), a Writer LLM consolidates entries — merging similar ones, removing stale info, targeting 50–60% compression. This prevents unbounded file growth.

| File | Default Threshold |
|---|---|
| `global.md` | 8,000 chars |
| `project.md` | 8,000 chars |
| `corrections.md` | 8,000 chars |

### Memory Worker Thresholds

| Worker | Trigger | Default |
|---|---|---|
| Observer | Tokens since last run ≥ threshold | 15,000 tokens |
| Reflector + Dropper | Tokens since last run ≥ threshold | 25,000 tokens |
| Dropper prune | Pool size exceeds max | 20,000 entries |
| Dropper target | After pruning | 10,000 entries |

## Transaction Usage

**No explicit transactions.** All SQLite operations are single-statement `execute()` or `prepare()`/`query_map()` calls. Since the connection is single-threaded (Mutex-guarded) and each call is atomic, this is safe — but there is no multi-statement atomicity guarantee.

For filesystem operations, each write is a single `fs::write()` or append — also atomic per-operation but not grouped. The `drop_observations()` method does a read-filter-rewrite cycle that is **not** atomic (concurrent writes during the rewrite window could be lost).

## Key Invariants

1. **SQLite is disposable.** Deleting `search.db` is safe — the index is rebuilt from Markdown/JSONL files on next `MemoryStore::new()`.
2. **Markdown files are append-only** (except checkpoint, notes-clear, and consolidation which do full rewrites).
3. **JSONL files are append-only** (except `drop_observations()` which does a full rewrite).
4. **`§` is the entry delimiter** for Markdown memory files — splitting on it yields individual memory entries.
5. **Project isolation** is via path hashing — two projects with the same absolute path share memory (by design).
