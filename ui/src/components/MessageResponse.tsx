// components/MessageResponse.tsx — ⎿ indent wrapper for all responses
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';

export function MessageResponse({ children }: { children: React.ReactNode }) {
  return (
    <Box flexDirection="row">
      <Text color={theme.inactive}>{LAYOUT.RESPONSE_INDENT}</Text>
      <Box flexShrink={1} flexGrow={1}>{children}</Box>
    </Box>
  );
}
