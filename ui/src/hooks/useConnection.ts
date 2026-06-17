//! Connection management for the TUI.
//!
//! Owns the backend-health heartbeat and git-branch detection. The TUI uses
//! this to:
//! - Surface a `connected: boolean` for the status bar (so the user knows
//!   if the daemon died while the TUI was open).
//! - Show the current git branch in the status line (cached, no async cost).
//!
//! This module is a leaf in the dependency graph: it depends on `services/api`
//! and on `useEffect`/`useState`, but nothing in `useChatSession` does.
//! `useChatSession` calls `useConnection()` and reads `.connected` + `.gitBranch`.

import { useEffect, useState } from 'react';
import { checkHealth } from '../services/api';

/**
 * Detect the current git branch synchronously via `Bun.spawnSync`.
 * Returns null on any failure (not in a git repo, git not installed, etc.).
 */
export function getGitBranchSync(): string | null {
  try {
    const proc = Bun.spawnSync({
      cmd: ['git', 'rev-parse', '--abbrev-ref', 'HEAD'],
      cwd: process.cwd(),
    });
    if (proc.exitCode !== 0) return null;
    return proc.stdout.toString().trim() || null;
  } catch {
    return null;
  }
}

export interface ConnectionStatus {
  /** True if the last heartbeat ping succeeded; false if it failed. */
  connected: boolean;
  /** Current git branch, or null if not in a git repo. */
  gitBranch: string | null;
  /** Manually re-check the backend health. Returns the new connected state. */
  refresh: () => Promise<boolean>;
}

/**
 * Subscribe to backend health via a 10s heartbeat. Also returns the
 * current git branch (detected once on mount).
 *
 * Heartbeat interval: 10s. Daemon auto-shutdown threshold is currently
 * 120s, so 2 missed heartbeats is safe; 3+ missed → daemon may have died.
 *
 * The `refresh()` function allows manual re-check (e.g. on reconnect
 * after a network blip).
 */
export function useConnection(): ConnectionStatus {
  const [connected, setConnected] = useState(true);
  const [gitBranch] = useState<string | null>(() => getGitBranchSync());

  useEffect(() => {
    const timer = setInterval(() => {
      checkHealth()
        .then(() => setConnected(true))
        .catch(() => setConnected(false));
    }, 10_000);
    return () => clearInterval(timer);
  }, []);

  const refresh = async (): Promise<boolean> => {
    try {
      await checkHealth();
      setConnected(true);
      return true;
    } catch {
      setConnected(false);
      return false;
    }
  };

  return { connected, gitBranch, refresh };
}
