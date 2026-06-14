// SystemMessage — Claude Code uses dimColor with no prefix symbol
// System messages are for status notices, turn durations, warnings etc
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function SystemMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return (
    <Box marginTop={addMargin ? 1 : 0}>
      <Text dimColor>{truncateText(msg.content)}</Text>
    </Box>
  );
}
