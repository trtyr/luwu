// StatusLine — Claude Code 1:1 bottom bar
// Source: docs/12-input-footer-ui.md, docs/16-input-box-detailed-ui.md §11
// Layout: ❯ model · ? for shortcuts    (single line, dimColor)
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';

interface StatusLineProps {
  model: string;
  cwd: string;
  gitBranch: string | null;
  contextPercent: number;
  phase: string;
  iteration?: number;
}

export function StatusLine({ model, cwd, gitBranch, contextPercent, phase, iteration }: StatusLineProps) {
  const ctxColor =
    contextPercent < 50 ? theme.success :
    contextPercent < 80 ? theme.warning :
    theme.error;

  const hint = (phase === 'thinking' || phase === 'streaming')
    ? 'esc to interrupt'
    : '? for shortcuts · ↑↓ history · / commands';

  return (
    <Box flexDirection="column">
      {/* Main line: ❯ model · cwd · git · context% */}
      <Box>
        <Text color={theme.permission}>{'❯ '}</Text>
        <Text color={theme.inactive}>{model}</Text>
        {iteration !== undefined && iteration > 0 && (
          <Text color={theme.subtle}> · iter {iteration}</Text>
        )}
        <Text color={theme.subtle}> · </Text>
        <Text color={theme.inactive}>{shortenPath(cwd)}</Text>
        {gitBranch && (
          <>
            <Text color={theme.subtle}> · </Text>
            <Text color={theme.suggestion}>{gitBranch}</Text>
          </>
        )}
        <Text color={theme.subtle}> · </Text>
        <Text color={ctxColor}>{contextPercent}%</Text>
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
