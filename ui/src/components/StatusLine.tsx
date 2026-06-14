// StatusLine — segment-based status bar with working indicator
// Segments: [working/waiting spinner] · model · runtime · cwd · git · [bar] ctx% · iter · sess
// Braille spinner: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ (rotating dots animation)
// Stalled detection: if no SSE activity for >15s during thinking/streaming → "waiting"
import React, { useState, useEffect, useRef } from 'react';
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
  lastActivityRef?: React.MutableRefObject<number>;
}

// Braille spinner frames — looks like rotating pixels
const BRAILLE_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const STALL_THRESHOLD_MS = 15_000; // 15s no activity → "waiting"

// ── Git status detection ──
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

function formatTokens(tokens: number): string {
  if (tokens <= 0) return '-';
  if (tokens >= 1000) {
    const k = tokens / 1000;
    if (k === Math.floor(k)) return `${k}k`;
    return `${k.toFixed(1)}k`;
  }
  return String(tokens);
}

const SESSION_START = Date.now();

export function StatusLine({
  model, sessionId, cwd, gitBranch, contextPercent, contextTokens, phase, iteration,
  lastActivityRef,
}: StatusLineProps) {
  const [gitStatus, setGitStatus] = useState<GitStatus | null>(null);
  const [spinnerIdx, setSpinnerIdx] = useState(0);
  const [stallSec, setStallSec] = useState(0);

  // Git poll every 5s
  useEffect(() => {
    const poll = () => setGitStatus(getGitStatus(cwd));
    poll();
    const timer = setInterval(poll, 5000);
    return () => clearInterval(timer);
  }, [cwd]);

  const isWorking = phase === 'thinking' || phase === 'streaming';

  // Braille spinner animation — 80ms interval while working
  useEffect(() => {
    if (!isWorking) return;
    const timer = setInterval(() => setSpinnerIdx(i => (i + 1) % BRAILLE_FRAMES.length), 80);
    return () => clearInterval(timer);
  }, [isWorking]);

  // Runtime + stall detection — 1s tick
  const [runtime, setRuntime] = useState('0s');
  useEffect(() => {
    const update = () => {
      const elapsed = Math.floor((Date.now() - SESSION_START) / 1000);
      const m = Math.floor(elapsed / 60);
      const s = elapsed % 60;
      setRuntime(m > 0 ? `${m}m${s.toString().padStart(2, '0')}` : `${s}s`);

      // Stall check
      if (isWorking && lastActivityRef) {
        const idle = Date.now() - lastActivityRef.current;
        setStallSec(idle > STALL_THRESHOLD_MS ? Math.floor(idle / 1000) : 0);
      } else {
        setStallSec(0);
      }
    };
    update();
    const interval = setInterval(update, 1000);
    return () => clearInterval(interval);
  }, [isWorking, lastActivityRef]);

  const ctxColor: string =
    contextPercent <= 70 ? theme.success :
    contextPercent <= 85 ? theme.warning :
    theme.error;

  const barWidth = 10;
  const filled = Math.round((contextPercent / 100) * barWidth);
  const bar = '█'.repeat(filled) + '░'.repeat(barWidth - filled);

  const hint = isWorking
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
        {/* Working indicator — braille spinner + status text */}
        {isWorking ? (
          stallSec > 0 ? (
            <>
              <Text color={theme.warning}>{BRAILLE_FRAMES[spinnerIdx]} </Text>
              <Text color={theme.warning}>waiting {stallSec}s</Text>
            </>
          ) : (
            <>
              <Text color={theme.claude}>{BRAILLE_FRAMES[spinnerIdx]} </Text>
              <Text color={theme.claude}>working</Text>
            </>
          )
        ) : (
          <Text color={theme.permission}>{'❯'}</Text>
        )}

        {sep}

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
