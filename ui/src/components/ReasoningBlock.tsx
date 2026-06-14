// ReasoningBlock — ∴ thinking display with ctrl+o expand toggle
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

  // ctrl+o toggles expand/collapse
  useInput(useCallback((input: string, key: any) => {
    if (!active) return;
    if (key.ctrl && input === 'o') {
      setExpanded(prev => !prev);
    }
  }, [active]));

  if (!reasoning || !reasoning.trim()) return null;

  if (!expanded) {
    return (
      <Box marginTop={mt}>
        <Text dimColor italic>
          ∴ Thinking{' '}
          <Text color={theme.subtle}>(ctrl+o to expand)</Text>
        </Text>
      </Box>
    );
  }

  return (
    <Box flexDirection="column" marginTop={mt} width="100%">
      <Text dimColor italic>
        ∴ Thinking…{' '}
        <Text color={theme.subtle}>(ctrl+o to collapse)</Text>
      </Text>
      <Box paddingLeft={2} marginTop={1}>
        <Markdown>{reasoning}</Markdown>
      </Box>
    </Box>
  );
}
