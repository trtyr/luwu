// UserMessage — Claude Code 1:1
// Source: docs/01-user-message-ui.md
// Pointer: ❯ in subtle color (NOT suggestion blue)
// Background: userMessageBackground, paddingRight=1
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function UserMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  const boxProps: any = {
    flexDirection: 'column' as const,
    marginTop: addMargin ? 1 : 0,
    paddingRight: 1,
  };
  boxProps.backgroundColor = theme.userMessageBackground;

  return (
    <Box {...boxProps}>
      <Text>
        <Text color={theme.subtle}>{'❯ '}</Text>
        <Text color={theme.text}>{truncateText(msg.content)}</Text>
      </Text>
    </Box>
  );
}
