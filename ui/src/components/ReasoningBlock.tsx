// ReasoningBlock — ∴ thinking display
// Source: docs/03-thinking-block-ui.md
// dimColor = theme.inactive color (NOT ANSI dim, which conflicts with bold)
// Collapsed: ∴ Thinking Ctrl+O to expand — inactive italic
// Expanded: ∴ Thinking… + marginTop=1 + paddingLeft=2 Markdown(inactive)
import React, { useState, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';
import { Markdown } from './Markdown.js';

interface Props {
  reasoning: string;
  addMargin?: boolean;
  active?: boolean;
}

export function ReasoningBlock({ reasoning, addMargin = false, active = true }: Props) {
  const [expanded, setExpanded] = useState(false);
  const mt = addMargin ? 1 : 0;

  useInput(useCallback((input: string, key: any) => {
    if (!active) return;
    if (key.ctrl && input === 'o') setExpanded(prev => !prev);
  }, [active]));

  if (!reasoning || !reasoning.trim()) return null;

  if (!expanded) {
    return (
      <Box marginTop={mt}>
        <Text color={theme.inactive} italic>
          {'∴ Thinking Ctrl+O to expand'}
        </Text>
      </Box>
    );
  }

  return (
    <Box flexDirection="column" marginTop={mt} width="100%">
      <Text color={theme.inactive} italic>
        {'∴ Thinking…'}
      </Text>
      <Box paddingLeft={2} marginTop={1}>
        <Markdown dimColor>{reasoning}</Markdown>
      </Box>
    </Box>
  );
}
