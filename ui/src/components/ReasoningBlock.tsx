// components/ReasoningBlock.tsx — Claude Code-style reasoning display
// Two modes: collapsed ("∴ Thinking") and expanded ("∴ Thinking…" + content)
import React, { useState } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Markdown } from './Markdown.js';

interface Props {
  reasoning: string;
  addMargin?: boolean;
}

export function ReasoningBlock({ reasoning, addMargin = false }: Props) {
  const [expanded, setExpanded] = useState(false);
  const mt = addMargin ? 1 : 0;

  if (!reasoning || !reasoning.trim()) return null;

  if (!expanded) {
    return (
      <Box marginTop={mt}>
        <Text dimColor italic>∴ Thinking (ctrl+o to expand)</Text>
      </Box>
    );
  }

  return (
    <Box flexDirection="column" gap={1} marginTop={mt} width="100%">
      <Text dimColor italic>∴ Thinking…</Text>
      <Box paddingLeft={2}>
        <Markdown>{reasoning}</Markdown>
      </Box>
    </Box>
  );
}
