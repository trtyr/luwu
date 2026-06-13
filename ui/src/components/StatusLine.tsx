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

/**
 * Bottom status line — Claude Code style
 * Layout: [phase] model · cwd · git-branch · context%
 */
export function StatusLine({ model, cwd, gitBranch, contextPercent, phase, iteration }: StatusLineProps) {
  // Context color: green < 50%, yellow < 80%, red >= 80%
  const ctxColor =
    contextPercent < 50 ? theme.success :
    contextPercent < 80 ? theme.warning :
    theme.error;

  const phaseLabel =
    phase === 'thinking' ? '✻ thinking' :
    phase === 'streaming' ? '✻ streaming' :
    phase === 'connecting' ? '✻ connecting' :
    '✻ ready';

  const phaseColor =
    phase === 'thinking' || phase === 'streaming' ? theme.claude :
    phase === 'connecting' ? theme.warning :
    theme.inactive;

  return (
    <Box flexDirection="column" marginTop={1}>
      <Box>
        {/* Phase indicator */}
        <Text color={phaseColor} bold>{phaseLabel} </Text>
        {iteration !== undefined && iteration > 0 && (
          <Text color={theme.subtle}>iter {iteration} </Text>
        )}
        <Text color={theme.subtle}>│ </Text>

        {/* Model */}
        <Text color={theme.claude}>{model}</Text>
        <Text color={theme.subtle}> · </Text>

        {/* CWD — short form */}
        <Text color={theme.inactive}>{shortenPath(cwd)}</Text>

        {/* Git branch */}
        {gitBranch && (
          <>
            <Text color={theme.subtle}> · </Text>
            <Text color={theme.suggestion}>{gitBranch}</Text>
          </>
        )}

        {/* Context usage */}
        <Text color={theme.subtle}> · </Text>
        <Text color={ctxColor}>{contextPercent}%</Text>
      </Box>

      {/* Input hint line */}
      <Box>
        <Text color={theme.subtle}>↵ send · ctrl+c cancel · esc clear</Text>
      </Box>
    </Box>
  );
}

function shortenPath(p: string): string {
  const parts = p.replace(/^\/Users\/[^/]+/, '~').split('/');
  if (parts.length <= 3) return parts.join('/');
  return parts[0] + '/…/' + parts.slice(-2).join('/');
}
