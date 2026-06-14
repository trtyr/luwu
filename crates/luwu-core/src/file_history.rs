//! File history system — checkpoint, backup, and rewind for file modifications.
//!
//! Based on Claude Code's FileHistory system:
//! - `track_edit` is called BEFORE a file-modifying tool executes, saving the original.
//! - `make_snapshot` is called before each user message, recording all tracked files.
//! - `rewind_to` restores files to a target snapshot state.
//! - Rewind is a pure filesystem side-effect — it does NOT mutate FileHistoryState,
//!   so you can rewind multiple times to different points.
//!
//! Backup files are stored at `~/.luwu/sessions/{id}/file-history/{hash}@v{version}`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Constants ──
const MAX_SNAPSHOTS: usize = 100;

// ── Types ──

/// A single file backup. `backup_file_name == None` means the file didn't exist at this version.
/// `backup_file_name == Some(null_marker)` would be weird; we use the `FileHistoryBackup` struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistoryBackup {
    /// The backup file name (hash@v{version}), or null if file didn't exist.
    pub backup_file_name: Option<String>,
    /// Version number (incrementing).
    pub version: u32,
    /// When the backup was created.
    #[serde(with = "chrono_serde")]
    pub backup_time: SystemTime,
}

/// A snapshot of all tracked files at a point in time, associated with a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHistorySnapshot {
    /// The user message this snapshot is associated with (message index or id).
    pub message_ref: String,
    /// Map of tracked file path → backup.
    pub tracked_file_backups: HashMap<String, FileHistoryBackup>,
    /// When the snapshot was taken.
    #[serde(with = "chrono_serde")]
    pub timestamp: SystemTime,
}

/// The full state of file history for a session.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileHistoryState {
    /// Snapshots, in order (oldest first). Trimmed to MAX_SNAPSHOTS.
    pub snapshots: Vec<FileHistorySnapshot>,
    /// All tracked file paths (relative to working dir).
    pub tracked_files: HashSet<String>,
    /// Monotonic counter for backup versions.
    pub next_version: u32,
}

// ── chrono_serde for SystemTime ──
mod chrono_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(time: &SystemTime, s: S) -> Result<S::Ok, S::Error> {
        let millis = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        s.serialize_u64(millis)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SystemTime, D::Error> {
        let millis = u64::deserialize(d)?;
        Ok(SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(millis))
    }
}

// ── FileHistory manager ──

/// Manages file history for a single session.
pub struct FileHistory {
    /// Root directory for backups: ~/.luwu/sessions/{id}/file-history/
    backup_dir: PathBuf,
    /// The working directory (for resolving relative paths).
    working_dir: PathBuf,
    /// In-memory state (also persisted to state.json).
    state: FileHistoryState,
    /// Path to the state.json file.
    state_path: PathBuf,
}

impl FileHistory {
    /// Create a new FileHistory for a session. Loads existing state if present.
    pub fn new(session_dir: &Path, working_dir: &Path) -> Self {
        let backup_dir = session_dir.join("file-history");
        let state_path = backup_dir.join("state.json");
        fs::create_dir_all(&backup_dir).ok();

        let state = if backup_dir.join("state.json").exists() {
            fs::read_to_string(&state_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            FileHistoryState::default()
        };

        Self {
            backup_dir,
            working_dir: working_dir.to_path_buf(),
            state,
            state_path,
        }
    }

    /// Get a reference to the current state.
    pub fn state(&self) -> &FileHistoryState {
        &self.state
    }

    /// Track a file edit — call BEFORE the tool modifies the file.
    /// Creates a backup of the original file content.
    /// If the file is already tracked in the latest snapshot, skips (prevents overwriting v1).
    pub fn track_edit(&mut self, file_path: &str, message_ref: &str) -> io::Result<()> {
        let relative = self.make_relative(file_path);

        // Check if already tracked in latest snapshot
        if let Some(latest) = self.state.snapshots.last()
            && latest.tracked_file_backups.contains_key(&relative)
        {
            // Already backed up — don't overwrite v1
            return Ok(());
        }

        let abs_path = self.working_dir.join(&relative);
        let version = self.state.next_version;
        self.state.next_version += 1;

        let backup = self.create_backup(&abs_path, version)?;
        self.state.tracked_files.insert(relative.clone());

        // Add to the latest snapshot (or create one if none exists)
        if let Some(latest) = self.state.snapshots.last_mut() {
            latest.tracked_file_backups.insert(relative.clone(), backup);
        } else {
            let mut backups = HashMap::new();
            backups.insert(relative.clone(), backup);
            self.state.snapshots.push(FileHistorySnapshot {
                message_ref: message_ref.to_string(),
                tracked_file_backups: backups,
                timestamp: SystemTime::now(),
            });
        }

        self.persist()?;
        tracing::debug!(file = %relative, version, "Tracked file edit");
        Ok(())
    }

    /// Make a snapshot of all tracked files, associated with a user message.
    /// Call this before each user message is submitted.
    pub fn make_snapshot(&mut self, message_ref: &str) -> io::Result<()> {
        let mut tracked_backups = HashMap::new();

        for file_path in &self.state.tracked_files {
            let abs_path = self.working_dir.join(file_path);

            // Check if file changed since last backup
            let needs_backup = match self.latest_backup_for(file_path) {
                Some(existing) => {
                    // Compare mtime — if file is newer than backup, re-backup
                    let bp = existing
                        .backup_file_name
                        .as_ref()
                        .map(|n| self.backup_path(n));
                    match (bp, fs::metadata(&abs_path)) {
                        (Some(bpath), Ok(orig)) => match fs::metadata(&bpath) {
                            Ok(bak) => orig.modified().ok() > bak.modified().ok(),
                            _ => true,
                        },
                        _ => true,
                    }
                }
                None => true, // Never backed up
            };

            if needs_backup {
                let version = self.state.next_version;
                self.state.next_version += 1;
                let backup = self.create_backup(&abs_path, version)?;
                tracked_backups.insert(file_path.clone(), backup);
            } else if let Some(existing) = self.latest_backup_for(file_path).cloned() {
                // Reuse existing backup
                tracked_backups.insert(file_path.clone(), existing);
            }
        }

        let snapshot = FileHistorySnapshot {
            message_ref: message_ref.to_string(),
            tracked_file_backups: tracked_backups,
            timestamp: SystemTime::now(),
        };

        self.state.snapshots.push(snapshot);

        // Trim old snapshots
        if self.state.snapshots.len() > MAX_SNAPSHOTS {
            let excess = self.state.snapshots.len() - MAX_SNAPSHOTS;
            self.state.snapshots.drain(0..excess);
        }

        self.persist()?;
        tracing::debug!(message_ref, "Created file history snapshot");
        Ok(())
    }

    /// Rewind (restore) files to the state at a given snapshot.
    /// This is a PURE filesystem operation — does NOT modify FileHistoryState.
    /// Returns the list of files that were changed.
    pub fn rewind_to(&self, message_ref: &str) -> io::Result<Vec<String>> {
        let target = self
            .state
            .snapshots
            .iter()
            .rev()
            .find(|s| s.message_ref == message_ref);

        let target = match target {
            Some(s) => s.clone(),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Snapshot not found for message: {message_ref}"),
                ));
            }
        };

        self.apply_snapshot(&target)
    }

    /// Get diff stats (insertions/deletions) between a snapshot and current state.
    pub fn diff_stats_for(&self, message_ref: &str) -> Option<DiffStats> {
        let snapshot = self
            .state
            .snapshots
            .iter()
            .rev()
            .find(|s| s.message_ref == message_ref)?;

        let mut total_insertions = 0;
        let mut total_deletions = 0;
        let mut files_changed = 0;

        for (file_path, backup) in &snapshot.tracked_file_backups {
            let abs_path = self.working_dir.join(file_path);
            let backup_name = backup.backup_file_name.as_ref();

            let backup_content =
                backup_name.and_then(|name| fs::read_to_string(self.backup_path(name)).ok());

            let current_content = fs::read_to_string(&abs_path).ok();

            match (backup_content, current_content) {
                (Some(old), Some(new)) => {
                    let old_lines: Vec<&str> = old.lines().collect();
                    let new_lines: Vec<&str> = new.lines().collect();
                    let (ins, del) = line_diff(&old_lines, &new_lines);
                    if ins > 0 || del > 0 {
                        files_changed += 1;
                        total_insertions += ins;
                        total_deletions += del;
                    }
                }
                (None, Some(_)) => {
                    // File was created after this snapshot
                    files_changed += 1;
                    total_insertions += fs::read_to_string(&abs_path)
                        .map(|c| c.lines().count())
                        .unwrap_or(0);
                }
                (Some(_), None) => {
                    // File was deleted after this snapshot
                    files_changed += 1;
                    total_deletions += 1;
                }
                (None, None) => {}
            }
        }

        Some(DiffStats {
            files_changed,
            insertions: total_insertions,
            deletions: total_deletions,
        })
    }

    /// List all snapshots with their message refs and diff stats.
    pub fn list_snapshots(&self) -> Vec<(String, SystemTime, Option<DiffStats>)> {
        self.state
            .snapshots
            .iter()
            .map(|s| {
                (
                    s.message_ref.clone(),
                    s.timestamp,
                    self.diff_stats_for(&s.message_ref),
                )
            })
            .collect()
    }

    // ── Private helpers ──

    fn apply_snapshot(&self, target: &FileHistorySnapshot) -> io::Result<Vec<String>> {
        let mut changed = Vec::new();

        for file_path in &self.state.tracked_files {
            let abs_path = self.working_dir.join(file_path);
            let target_backup = target.tracked_file_backups.get(file_path);

            // Find the backup file name — use target snapshot, or fall back to v1
            let backup_name = target_backup
                .and_then(|b| b.backup_file_name.clone())
                .or_else(|| {
                    // Fall back to earliest version backup
                    self.state
                        .snapshots
                        .iter()
                        .flat_map(|s| s.tracked_file_backups.get(file_path))
                        .filter_map(|b| b.backup_file_name.clone())
                        .next()
                });

            match backup_name {
                None => continue, // No backup found — skip
                Some(ref name) => {
                    let backup_path = self.backup_path(name);

                    if !backup_path.exists() {
                        // Backup file missing — skip
                        continue;
                    }

                    // Check if file actually changed
                    if !self.check_file_changed(&abs_path, &backup_path)? {
                        continue;
                    }

                    // Restore: copy backup over current file
                    let parent = abs_path.parent();
                    if let Some(p) = parent {
                        fs::create_dir_all(p)?;
                    }
                    fs::copy(&backup_path, &abs_path)?;

                    // Preserve permissions from backup
                    if let Ok(meta) = fs::metadata(&backup_path) {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(
                                &abs_path,
                                fs::Permissions::from_mode(meta.permissions().mode()),
                            )
                            .ok();
                        }
                    }

                    changed.push(file_path.clone());
                }
            }
        }

        // Also handle files that existed in target but were deleted later
        // (backup_file_name == None means file didn't exist at snapshot → should delete)
        for (file_path, backup) in &target.tracked_file_backups {
            if backup.backup_file_name.is_none() {
                let abs_path = self.working_dir.join(file_path);
                if abs_path.exists() {
                    fs::remove_file(&abs_path)?;
                    changed.push(file_path.clone());
                }
            }
        }

        tracing::info!(files_changed = changed.len(), "Rewind applied snapshot");
        Ok(changed)
    }

    fn create_backup(&self, file_path: &Path, version: u32) -> io::Result<FileHistoryBackup> {
        match fs::metadata(file_path) {
            Ok(_) => {
                // File exists — create backup
                let backup_name = self.backup_file_name(file_path, version);
                let backup_path = self.backup_dir.join(&backup_name);

                fs::copy(file_path, &backup_path)?;

                // Preserve permissions
                if let Ok(meta) = fs::metadata(file_path) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(
                            &backup_path,
                            fs::Permissions::from_mode(meta.permissions().mode()),
                        )
                        .ok();
                    }
                }

                Ok(FileHistoryBackup {
                    backup_file_name: Some(backup_name),
                    version,
                    backup_time: SystemTime::now(),
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // File doesn't exist — record as null backup
                Ok(FileHistoryBackup {
                    backup_file_name: None,
                    version,
                    backup_time: SystemTime::now(),
                })
            }
            Err(e) => Err(e),
        }
    }

    fn backup_file_name(&self, file_path: &Path, version: u32) -> String {
        let mut hasher = Sha256::new();
        hasher.update(file_path.to_string_lossy().as_bytes());
        let hash = hasher.finalize();
        let hash_str: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
        format!("{hash_str}@v{version}")
    }

    fn backup_path(&self, name: &str) -> PathBuf {
        self.backup_dir.join(name)
    }

    fn latest_backup_for(&self, file_path: &str) -> Option<&FileHistoryBackup> {
        self.state
            .snapshots
            .iter()
            .rev()
            .flat_map(|s| s.tracked_file_backups.get(file_path))
            .next()
    }

    fn check_file_changed(&self, original: &Path, backup: &Path) -> io::Result<bool> {
        // Quick checks: existence, size
        match (fs::metadata(original), fs::metadata(backup)) {
            (Err(_), Ok(_)) => Ok(true), // original missing, backup exists
            (Ok(_), Err(_)) => Ok(true), // original exists, backup missing
            (Err(_), Err(_)) => Ok(false),
            (Ok(orig), Ok(bak)) => {
                if orig.len() != bak.len() {
                    return Ok(true);
                }
                // Content comparison
                let orig_content = fs::read(original)?;
                let bak_content = fs::read(backup)?;
                Ok(orig_content != bak_content)
            }
        }
    }

    fn make_relative(&self, path: &str) -> String {
        let p = Path::new(path);
        if p.is_absolute() {
            match p.strip_prefix(&self.working_dir) {
                Ok(rel) => rel.to_string_lossy().to_string(),
                Err(_) => path.to_string(),
            }
        } else {
            path.to_string()
        }
    }

    fn persist(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.state).map_err(|e| io::Error::other(e))?;
        fs::write(&self.state_path, json)?;
        Ok(())
    }
}

// ── DiffStats ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// Simple line-level diff counting (not a real diff algorithm, just insertions/deletions).
fn line_diff(old: &[&str], new: &[&str]) -> (usize, usize) {
    // Simple LCS-based diff for counting
    let m = old.len();
    let n = new.len();

    // For large inputs, fall back to simple delta
    if m > 500 || n > 500 {
        return (
            new.len().saturating_sub(old.len()),
            old.len().saturating_sub(new.len()),
        );
    }

    // DP table for LCS
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    let lcs = dp[m][n];
    let deletions = m - lcs;
    let insertions = n - lcs;
    (insertions, deletions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_diff_identical() {
        let a = vec!["a", "b", "c"];
        assert_eq!(line_diff(&a, &a), (0, 0));
    }

    #[test]
    fn test_line_diff_additions() {
        let old = vec!["a", "b"];
        let new = vec!["a", "b", "c", "d"];
        let (ins, del) = line_diff(&old, &new);
        assert_eq!(ins, 2);
        assert_eq!(del, 0);
    }

    #[test]
    fn test_line_diff_deletions() {
        let old = vec!["a", "b", "c", "d"];
        let new = vec!["a", "b"];
        let (ins, del) = line_diff(&old, &new);
        assert_eq!(ins, 0);
        assert_eq!(del, 2);
    }

    #[test]
    fn test_line_diff_mixed() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "x", "c"];
        let (ins, del) = line_diff(&old, &new);
        assert_eq!(ins, 1);
        assert_eq!(del, 1);
    }
}
