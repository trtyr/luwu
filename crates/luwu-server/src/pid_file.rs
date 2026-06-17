//! PID file management for the luwu daemon.
//!
//! Provides atomic write (write-to-temp + rename) and stale-PID detection
//! to avoid leaving corrupt or stale PID files when the daemon crashes or
//! restarts unexpectedly.
//!
//! Design choices:
//! - **Atomic write**: write to `luwu.pid.tmp` first, then rename to `luwu.pid`.
//!   `std::fs::rename` is atomic on POSIX (same filesystem), so readers always
//!   see a complete file.
//! - **Stale detection**: on read, check if the PID in the file corresponds to
//!   a live process via `kill(pid, 0)`. If not, treat the file as stale.
//! - **No silent failures**: every IO error is reported via `tracing::warn!`
//!   instead of being swallowed with `let _ = ...`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Manages the daemon's PID file at `~/.luwu/luwu.pid`.
///
/// Construct via [`PidFile::at(path)`], then call [`write`](Self::write) to
/// install the file and [`cleanup`](Self::cleanup) to remove it on shutdown.
/// Stale PID files (from a crashed previous daemon) are reported by
/// [`read_live`](Self::read_live) and can be cleaned up with
/// [`cleanup_stale`](Self::cleanup_stale).
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    /// Create a PID file manager at the given path. Does not touch the
    /// filesystem yet — call [`write`](Self::write) when the daemon is ready.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Default location: `~/.luwu/luwu.pid`. Returns `None` if `$HOME` is unset.
    pub fn default_path() -> Option<Self> {
        let home = dirs::home_dir()?;
        Some(Self::at(home.join(".luwu").join("luwu.pid")))
    }

    /// Path to the PID file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Owned clone of the PID file path. Use this when you need to move
    /// the path into another task while keeping the `PidFile` handle.
    pub fn path_buf(&self) -> PathBuf {
        self.path.clone()
    }

    /// Atomically write the current process's PID.
    ///
    /// The write is atomic: PID is first written to `luwu.pid.tmp` and then
    /// renamed to `luwu.pid`. If the rename succeeds, readers see a complete
    /// file even if the daemon crashed during the write.
    pub fn write(&self) -> io::Result<u32> {
        let pid = std::process::id();
        self.write_pid(pid)
    }

    /// Atomically write a specific PID (used by tests and for restoring a
    /// recorded PID).
    pub fn write_pid(&self, pid: u32) -> io::Result<u32> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                tracing::warn!(path = %self.path.display(), error = %e, "Failed to create PID file parent dir");
                e
            })?;
        }

        let tmp = self.path.with_extension("pid.tmp");
        fs::write(&tmp, pid.to_string()).map_err(|e| {
            tracing::warn!(path = %tmp.display(), error = %e, "Failed to write temp PID file");
            e
        })?;
        fs::rename(&tmp, &self.path).map_err(|e| {
            tracing::warn!(from = %tmp.display(), to = %self.path.display(), error = %e, "Failed to rename temp PID file");
            // Best-effort: try to remove the temp file so we don't leave garbage.
            let _ = fs::remove_file(&tmp);
            e
        })?;
        Ok(pid)
    }

    /// Read the PID stored in the file, if any.
    pub fn read(&self) -> io::Result<Option<u32>> {
        match fs::read_to_string(&self.path) {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    trimmed.parse::<u32>().map(Some).map_err(|e| {
                        tracing::warn!(path = %self.path.display(), contents = %trimmed, error = %e, "PID file has invalid contents");
                        io::Error::new(io::ErrorKind::InvalidData, e)
                    })
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => {
                tracing::warn!(path = %self.path.display(), error = %e, "Failed to read PID file");
                Err(e)
            }
        }
    }

    /// Read the PID file and verify that the process is still alive.
    /// Returns `Ok(None)` if the file doesn't exist, contains an invalid PID,
    /// or points to a process that is no longer running.
    pub fn read_live(&self) -> io::Result<Option<u32>> {
        let Some(pid) = self.read()? else {
            return Ok(None);
        };
        if is_process_alive(pid) {
            Ok(Some(pid))
        } else {
            tracing::info!(path = %self.path.display(), %pid, "Stale PID file detected (process not running)");
            Ok(None)
        }
    }

    /// Remove a stale PID file (the recorded process is not running).
    /// Best-effort: logs warnings on error but does not propagate.
    pub fn cleanup_stale(&self) {
        match self.read() {
            Ok(Some(pid)) if !is_process_alive(pid) => {
                tracing::info!(path = %self.path.display(), %pid, "Removing stale PID file");
                if let Err(e) = fs::remove_file(&self.path) {
                    tracing::warn!(path = %self.path.display(), error = %e, "Failed to remove stale PID file");
                }
            }
            Ok(_) => {} // No file, or points to a live process — leave alone
            Err(e) => {
                tracing::warn!(path = %self.path.display(), error = %e, "Failed to read PID file for cleanup");
            }
        }
    }

    /// Remove the PID file. Used on graceful shutdown.
    /// Best-effort: logs warnings on error but does not propagate.
    pub fn cleanup(&self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Already gone — nothing to do.
            }
            Err(e) => {
                tracing::warn!(path = %self.path.display(), error = %e, "Failed to remove PID file");
            }
        }
    }
}

/// Check whether a process with the given PID is still alive.
///
/// On Unix, `kill(pid, 0)` returns 0 if the process exists and is accessible,
/// -1/ESRCH if it doesn't exist, and -1/EPERM if it exists but we lack
/// permission. Both 0 and EPERM are treated as "alive".
///
/// On non-Unix, we conservatively assume the process is alive if any PID is
/// recorded (best-effort).
#[cfg(unix)]
pub fn is_process_alive(pid: u32) -> bool {
    // Reject PIDs that don't fit in i32 — kill() takes i32 and the
    // negative range has special semantics (e.g. kill(-1, 0) broadcasts
    // to all processes the current user can signal). Treating those
    // PIDs as "alive" would be incorrect.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // Safety: libc::kill with signal 0 is a no-op, just probes existence.
    let result = unsafe { libc::kill(pid as i32, 0) };
    if result == 0 {
        true
    } else {
        let errno = std::io::Error::last_os_error();
        matches!(
            errno.raw_os_error(),
            Some(libc::EPERM) // process exists, we just can't signal it
        )
    }
}

#[cfg(not(unix))]
pub fn is_process_alive(_pid: u32) -> bool {
    // Conservative fallback: assume alive. The file will be overwritten
    // on the next daemon start, so stale-state risk is bounded.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "luwu_pid_test_{}_{}_{}.pid",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        p
    }

    #[test]
    fn write_and_read() {
        let p = temp_path("write_read");
        let pf = PidFile::at(&p);
        let pid = pf.write().expect("write should succeed");
        let read_back = pf.read().expect("read should succeed");
        assert_eq!(read_back, Some(pid));
        assert_eq!(read_back, Some(std::process::id()));
        pf.cleanup();
        // Cleanup is idempotent.
        pf.cleanup();
    }

    #[test]
    fn read_missing_file() {
        let p = temp_path("missing");
        let pf = PidFile::at(&p);
        let result = pf.read().expect("missing file should not error");
        assert_eq!(result, None);
    }

    #[test]
    fn read_live_returns_self() {
        let p = temp_path("live_self");
        let pf = PidFile::at(&p);
        pf.write().unwrap();
        let live = pf.read_live().unwrap();
        assert!(live.is_some());
        pf.cleanup();
    }

    #[test]
    fn cleanup_stale_removes_dead_pid() {
        let p = temp_path("stale");
        let pf = PidFile::at(&p);
        // Use a PID very unlikely to exist (1 is init on Linux; use a high
        // unallocated number to be portable).
        let fake_pid = 0xFFFFFFFFu32; // 4 billion — practically guaranteed unused
        pf.write_pid(fake_pid).unwrap();
        assert!(p.exists());
        pf.cleanup_stale();
        // Should be removed because is_process_alive(fake_pid) is false.
        assert!(!p.exists(), "stale PID file should have been removed");
    }

    #[test]
    fn cleanup_stale_preserves_live_pid() {
        let p = temp_path("live_preserve");
        let pf = PidFile::at(&p);
        pf.write().unwrap();
        assert!(p.exists());
        pf.cleanup_stale();
        // Live PID — should still be there.
        assert!(p.exists(), "live PID file should be preserved");
        pf.cleanup();
    }

    #[test]
    fn write_creates_parent_dir() {
        let mut p = env::temp_dir();
        let dir_name = format!(
            "luwu_pid_test_dir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        p.push(dir_name);
        p.push("nested");
        p.push("luwu.pid");
        let pf = PidFile::at(&p);
        pf.write().expect("write should create parent dirs");
        assert!(p.exists());
        pf.cleanup();
        // Best-effort cleanup of test dirs.
        let _ = fs::remove_dir_all(p.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn read_invalid_contents_errors() {
        let p = temp_path("invalid");
        fs::write(&p, "not-a-number").unwrap();
        let pf = PidFile::at(&p);
        let result = pf.read();
        assert!(result.is_err());
        pf.cleanup();
    }
}
