// SystemMessage — Claude Code style
// No prefix, no dot, just dimmed text
// dimColor in Claude Code = theme.inactive color (NOT ANSI dim)
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function SystemMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return (
    <Box marginTop={addMargin ? 1 : 0}>
      <Text color={theme.inactive}>{truncateText(msg.content)}</Text>
    </Box>
  );
}
