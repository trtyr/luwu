// ReasoningBlock — ∴ thinking display
// Source: docs/03-thinking-block-ui.md
// Collapsed: ∴ Thinking Ctrl+O to expand — dimColor (inactive) italic
// Expanded: ∴ Thinking… + gap=1 + paddingLeft=2 Markdown(dimColor)
// dimColor = theme.inactive (NOT ANSI dim, which conflicts with bold)
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
    // Collapsed: ∴ Thinking Ctrl+O to expand
    return (
      <Box marginTop={mt}>
        <Text dimColor italic>
          {'∴ Thinking '}
          <Text dimColor>Ctrl+O to expand</Text>
        </Text>
      </Box>
    );
  }

  // Expanded: ∴ Thinking… + paddingLeft=2 + Markdown(dimColor)
  // gap=1 equivalent in Ink v5 = marginTop=1 on content
  return (
    <Box flexDirection="column" marginTop={mt} width="100%">
      <Text dimColor italic>
        {'∴ Thinking…'}
      </Text>
      <Box paddingLeft={2} marginTop={1}>
        {/* dimColor Markdown — all content rendered in inactive color */}
        <Markdown dimColor>{reasoning}</Markdown>
      </Box>
    </Box>
  );
}
