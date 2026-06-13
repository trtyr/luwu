// components/StatusLine.tsx — Claude Code-style status bar
// Layout: [phase] model · cwd · git-branch · context%
// Below: context-aware keyboard hint line with · separators
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
  hasSuggestions?: boolean;
}

export function StatusLine({ model, cwd, gitBranch, contextPercent, phase, iteration, hasSuggestions }: StatusLineProps) {
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

  // Context-aware hint line (like Claude Code's PromptInputFooterLeftSide)
  const hints: string[] = [];
  if (phase === 'thinking' || phase === 'streaming') {
    hints.push('esc to interrupt');
  } else {
    hints.push('? for shortcuts');
    hints.push('↑↓ history');
    hints.push('/ commands');
  }

  return (
    <Box flexDirection="column" marginTop={1}>
      <Box>
        <Text color={phaseColor} bold>{phaseLabel} </Text>
        {iteration !== undefined && iteration > 0 && (
          <Text color={theme.subtle}>iter {iteration} </Text>
        )}
        <Text color={theme.subtle}>· </Text>
        <Text color={theme.claude}>{model}</Text>
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
      <Box>
        <Text dimColor>{hints.join(' · ')}</Text>
      </Box>
    </Box>
  );
}

function shortenPath(p: string): string {
  const parts = p.replace(/^\/Users\/[^/]+/, '~').split('/');
  if (parts.length <= 3) return parts.join('/');
  return parts[0] + '/…/' + parts.slice(-2).join('/');
}
