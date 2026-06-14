// components/Overlay.tsx — shared Modal Pane wrapper
// Source: Claude Code doc 29 §13 — all local-jsx commands render in a Modal Pane
// ▔ (U+2594) separator line in permission color, paddingX=2, opaque background
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';

interface OverlayProps {
  title?: string;
  hint?: string;
  children: React.ReactNode;
}

const SEPARATOR_CHAR = '▔'; // U+2594 upper one-eighth block

export function Overlay({ title, hint, children }: OverlayProps) {
  return (
    <Box flexDirection="column" marginTop={1}>
      {/* Separator line — permission color */}
      <Text color={theme.permission}>{SEPARATOR_CHAR.repeat(60)}</Text>
      {/* Title */}
      {title && (
        <Box marginTop={1} paddingLeft={2}>
          <Text bold color={theme.claude}>{title}</Text>
        </Box>
      )}
      {/* Content — paddingX=2 per doc 29 §13 */}
      <Box flexDirection="column" paddingLeft={2} paddingRight={2} marginTop={title ? 0 : 1}>
        {children}
      </Box>
      {/* Footer hint */}
      {hint && (
        <Box paddingLeft={2} marginTop={1}>
          <Text color={theme.inactive}>{hint}</Text>
        </Box>
      )}
    </Box>
  );
}
