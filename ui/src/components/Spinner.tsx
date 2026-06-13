// components/Spinner.tsx — thinking spinner with dynamic verb
import React from 'react';
import { Box, Text } from 'ink';
import InkSpinner from 'ink-spinner';
import { theme } from '../theme.js';

export function Spinner({ phase, verb }: { phase: string; verb?: string }) {
  if (phase !== 'thinking') return null;
  return (
    <Box marginLeft={2}>
      <Text color={theme.claude}><InkSpinner type="dots" /></Text>
      <Text color={theme.subtle}> {verb ?? 'thinking'}…</Text>
    </Box>
  );
}
