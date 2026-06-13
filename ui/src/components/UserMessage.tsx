// components/UserMessage.tsx — background color, NO prefix (corrected from Claude Code analysis)
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function UserMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return (
    <Box
      flexDirection="column"
      marginTop={addMargin ? 1 : 0}
      backgroundColor={theme.userMessageBackground}
      paddingRight={1}
    >
      <Text color={theme.text}>{truncateText(msg.content)}</Text>
    </Box>
  );
}
