// StatusLine — rich bottom bar inspired by pi-observability + ccstatusline
// Layout: model · runtime · cwd · git · [████░░] context% · iter N · sess ID
// Context zones: ≤70% green, 71-85% yellow, >85% red (pi-observability standard)
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';

interface StatusLineProps {
  model: string;
  sessionId: string | null;
  cwd: string;
  gitBranch: string | null;
  contextPercent: number;
  phase: string;
  iteration?: number;
}

// Session start time captured once on module load (approximation)
const SESSION_START = Date.now();

export function StatusLine({ model, sessionId, cwd, gitBranch, contextPercent, phase, iteration }: StatusLineProps) {
  // Context color zones (pi-observability standard)
  const ctxColor =
    contextPercent <= 70 ? theme.success :
    contextPercent <= 85 ? theme.warning :
    theme.error;

  // Context progress bar [████░░░░░░]
  const barWidth = 10;
  const filled = Math.round((contextPercent / 100) * barWidth);
  const bar = '█'.repeat(filled) + '░'.repeat(barWidth - filled);

  // Runtime timer
  const [runtime, setRuntime] = useState('0m');
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

  // Context-aware hint line
  const hint = (phase === 'thinking' || phase === 'streaming')
    ? 'esc to interrupt'
    : '? for shortcuts · ↑↓ history · / commands';

  const sep = <Text color={theme.subtle}>{' · '}</Text>;

  return (
    <Box flexDirection="column">
      {/* Main status line */}
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
            <Text color={theme.suggestion}>{gitBranch}</Text>
          </>
        )}
        {sep}
        <Text color={ctxColor}>[{bar}] {contextPercent}%</Text>
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
      {/* Hint line */}
      <Box>
        <Text color={theme.inactive}>{hint}</Text>
      </Box>
    </Box>
  );
}

function shortenPath(p: string): string {
  const parts = p.replace(/^\/Users\/[^/]+/, '~').split('/');
  if (parts.length <= 3) return parts.join('/');
  return parts[0] + '/…/' + parts.slice(-2).join('/');
}
