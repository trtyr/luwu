// components/UserMessage.tsx — background color via ANSI escape, NO prefix symbol
import React from 'react';
import { Box, Text } from 'ink';
import { paint, bgPaint } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function UserMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  const content = truncateText(msg.content);
  const lines = content.split('\n');
  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0}>
      {lines.map((line, i) => (
        <Text key={i}>
          {' '}
          {line}
          {' '}
        </Text>
      ))}
    </Box>
  );
}
