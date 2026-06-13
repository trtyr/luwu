//! SearchIndex — SQLite FTS5 full-text index for memory search.
//!
//! Provides fast full-text search across all memory layers.
//! SQLite FTS5 is used as a mirror index — the source of truth remains
//! the Markdown/JSONL files. The index is rebuilt on demand if missing.

use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::debug;

/// A single search result from the FTS5 index.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub layer: String,
    pub content: String,
    pub session_id: String,
    pub timestamp: String,
}

/// SQLite FTS5 full-text search index.
#[derive(Clone)]
pub struct SearchIndex {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SearchIndex {
    /// Open or create the FTS5 index at the given path.
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(\
                layer,\
                content,\
                original UNINDEXED,\
                session_id UNINDEXED,\
                timestamp UNINDEXED,\
                tokenize='unicode61'\
            );",
        )?;

        debug!("SearchIndex opened at {}", path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path.to_path_buf(),
        })
    }

    /// Index a memory entry. CJK content is pre-tokenized for proper matching.
    pub fn index_entry(
        &self,
        layer: &str,
        content: &str,
        session_id: &str,
    ) -> Result<(), rusqlite::Error> {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let tokenized = tokenize_cjk(content);
        let conn = self.conn.lock().expect("search index lock poisoned");
        conn.execute(
            "INSERT INTO memory_fts (layer, content, original, session_id, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![layer, tokenized, content, session_id, timestamp],
        )?;
        Ok(())
    }

    /// Full-text search across all indexed entries. Uses BM25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, rusqlite::Error> {
        let tokenized_query = tokenize_cjk(query);
        let fts_query = sanitize_query(&tokenized_query);
        let conn = self.conn.lock().expect("search index lock poisoned");

        let mut stmt = conn.prepare(
            "SELECT layer, original, session_id, timestamp \
             FROM memory_fts \
             WHERE memory_fts MATCH ?1 \
             ORDER BY bm25(memory_fts) \
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(params![fts_query, limit as i64], |row| {
                Ok(SearchResult {
                    layer: row.get(0)?,
                    content: row.get(1)?,
                    session_id: row.get(2)?,
                    timestamp: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Delete all entries for a specific session.
    pub fn clear_session(&self, session_id: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        conn.execute(
            "DELETE FROM memory_fts WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Clear the entire index.
    pub fn clear_all(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().expect("search index lock poisoned");
        conn.execute("DELETE FROM memory_fts", [])?;
        Ok(())
    }
}

/// Tokenize CJK text by inserting spaces between CJK characters.
fn tokenize_cjk(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        if ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3040}'..='\u{30FF}').contains(&c) {
            if !result.is_empty() && !result.ends_with(' ') {
                result.push(' ');
            }
            result.push(c);
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Sanitize a tokenized query into a safe FTS5 query string.
fn sanitize_query(query: &str) -> String {
    let has_cjk = query
        .chars()
        .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c) || ('\u{3040}'..='\u{30FF}').contains(&c));
    if has_cjk {
        query.to_string()
    } else {
        let escaped = query.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_index() -> SearchIndex {
        let dir = std::env::temp_dir().join(format!(
            "luwu_test_fts_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        SearchIndex::open(&dir.join("test.db")).unwrap()
    }

    #[test]
    fn test_index_and_search() {
        let idx = temp_index();

        idx.index_entry("global", "User prefers pnpm over npm", "").unwrap();
        idx.index_entry("project", "Auth uses JWT tokens with RS256", "").unwrap();
        idx.index_entry("correction", "Don't use Box, use Arc for dyn traits", "").unwrap();

        let results = idx.search("pnpm", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].layer, "global");
        assert!(results[0].content.contains("pnpm"));
    }

    #[test]
    fn test_clear_session() {
        let idx = temp_index();

        idx.index_entry("notes", "entry from session A", "sess-a").unwrap();
        idx.index_entry("notes", "entry from session B", "sess-b").unwrap();
        idx.index_entry("notes", "another from A", "sess-a").unwrap();

        idx.clear_session("sess-a").unwrap();

        let results = idx.search("session", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "sess-b");
    }

    #[test]
    fn test_chinese_search() {
        let idx = temp_index();

        idx.index_entry("global", "用户偏好使用 pnpm 而不是 npm", "").unwrap();
        idx.index_entry("project", "认证模块使用 JWT 和 RS256 签名", "").unwrap();

        let results = idx.search("用户", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("用户偏好"));
    }
}
