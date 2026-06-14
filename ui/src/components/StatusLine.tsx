// StatusLine — segment-based status bar
// Inspired by CCometixLine: each info is a segment with icon + value.
// Segments: model · runtime · cwd · git branch + status · [bar] ctx% tokens · iter · sess
// Git status: ✓ clean / ● dirty / ⚠ conflict (polled every 5s)
// Context zones: ≤70% green, 71-85% yellow, >85% red
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';

interface StatusLineProps {
  model: string;
  sessionId: string | null;
  cwd: string;
  gitBranch: string | null;
  contextPercent: number;
  contextTokens?: number;
  phase: string;
  iteration?: number;
}

// ── Git status detection (CCometixLine pattern) ──
interface GitStatus {
  clean: boolean;
  conflict: boolean;
  ahead: number;
  behind: number;
}

function getGitStatus(cwd: string): GitStatus | null {
  try {
    const status = Bun.spawnSync({
      cmd: ['git', '--no-optional-locks', 'status', '--porcelain'],
      cwd,
    });
    if (status.exitCode !== 0) return null;
    const text = new TextDecoder().decode(status.stdout);
    const lines = text.trim().split('\n').filter(Boolean);

    const conflict = lines.some(l => l.includes('UU') || l.includes('AA') || l.includes('DD'));
    const clean = lines.length === 0;

    const ahead = Bun.spawnSync({
      cmd: ['git', '--no-optional-locks', 'rev-list', '--count', '@{u}..HEAD'],
      cwd,
    });
    const behind = Bun.spawnSync({
      cmd: ['git', '--no-optional-locks', 'rev-list', '--count', 'HEAD..@{u}'],
      cwd,
    });

    const a = parseInt(new TextDecoder().decode(ahead.stdout).trim(), 10) || 0;
    const b = parseInt(new TextDecoder().decode(behind.stdout).trim(), 10) || 0;

    return { clean, conflict, ahead: a, behind: b };
  } catch {
    return null;
  }
}

// ── Token formatting (CCometixLine pattern) ──
function formatTokens(tokens: number): string {
  if (tokens <= 0) return '-';
  if (tokens >= 1000) {
    const k = tokens / 1000;
    if (k === Math.floor(k)) return `${k}k`;
    return `${k.toFixed(1)}k`;
  }
  return String(tokens);
}

// Session start time captured once on module load
const SESSION_START = Date.now();

export function StatusLine({
  model, sessionId, cwd, gitBranch, contextPercent, contextTokens, phase, iteration,
}: StatusLineProps) {
  const [gitStatus, setGitStatus] = useState<GitStatus | null>(null);
  useEffect(() => {
    const poll = () => setGitStatus(getGitStatus(cwd));
    poll();
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  }, [cwd]);

  const ctxColor: string =
    contextPercent <= 70 ? theme.success :
    contextPercent <= 85 ? theme.warning :
    theme.error;

  const barWidth = 10;
  const filled = Math.round((contextPercent / 100) * barWidth);
  const bar = '█'.repeat(filled) + '░'.repeat(barWidth - filled);

  const [runtime, setRuntime] = useState('0s');
  useEffect(() => {
    const update = () => {
      const elapsed = Math.floor((Date.now() - SESSION_START) / 1000);
      const m = Math.floor(elapsed / 60);
      const s = elapsed % 60;
      setRuntime(m > 0 ? `${m}m${s.toString().padStart(2, '0')}` : `${s}s`);
    };
    update();
    const interval = setInterval(update, 1000);
    return () => clearInterval(interval);
  }, []);

  const hint = (phase === 'thinking' || phase === 'streaming')
    ? 'esc to interrupt'
    : '? for shortcuts · ↑↓ history · / commands';

  const sep = <Text color={theme.subtle}>{' · '}</Text>;

  let gitIndicator: string = '';
  let gitColor: string = theme.suggestion;
  if (gitStatus) {
    if (gitStatus.conflict) { gitIndicator = ' ⚠'; gitColor = theme.error; }
    else if (gitStatus.clean) { gitIndicator = ' ✓'; gitColor = theme.success; }
    else { gitIndicator = ' ●'; gitColor = theme.warning; }
    if (gitStatus.ahead > 0) gitIndicator += ` ↑${gitStatus.ahead}`;
    if (gitStatus.behind > 0) gitIndicator += ` ↓${gitStatus.behind}`;
  }

  const tokenStr = contextTokens && contextTokens > 0 ? formatTokens(contextTokens) : null;

  return (
    <Box flexDirection="column">
      <Box>
        <Text color={theme.permission}>{'❯ '}</Text>
        <Text color={theme.inactive}>{model}</Text>

        {sep}

        <Text color={theme.subtle}>⏱ {runtime}</Text>

        {sep}

        <Text color={theme.inactive}>{shortenPath(cwd)}</Text>

        {gitBranch && (
          <>
            {sep}
            <Text color={gitColor}>{gitBranch}{gitIndicator}</Text>
          </>
        )}

        {sep}

        <Text color={ctxColor}>[{bar}] {contextPercent}%</Text>
        {tokenStr && <Text color={theme.subtle}> {tokenStr}</Text>}

        {iteration !== undefined && iteration > 0 && (
          <>
            {sep}
            <Text color={theme.subtle}>iter {iteration}</Text>
          </>
        )}

        {sessionId && (
          <>
            {sep}
            <Text color={theme.inactive}>sess {sessionId.slice(0, 8)}</Text>
          </>
        )}
      </Box>

      <Box>
        <Text color={theme.inactive}>{hint}</Text>
      </Box>
    </Box>
  );
}

function shortenPath(p: string): string {
  const parts = p.replace(/\/$/, '').split('/');
  return parts[parts.length - 1] || p;
}
