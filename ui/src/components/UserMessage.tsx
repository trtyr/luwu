// UserMessage — background color via Box backgroundColor prop (runtime supported by Ink v5)
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function UserMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  // Ink v5 supports backgroundColor at runtime even though types don't include it
  const boxProps: any = {
    flexDirection: 'column' as const,
    marginTop: addMargin ? 1 : 0,
    paddingRight: 1,
  };
  boxProps.backgroundColor = theme.userMessageBackground;

  return (
    <Box {...boxProps}>
      <Text color={theme.text}>{truncateText(msg.content)}</Text>
    </Box>
  );
}
