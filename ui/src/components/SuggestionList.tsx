// components/SuggestionList.tsx — slash command autocomplete list
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import type { SuggestionItem } from '../core/types.js';

export function SuggestionList({ suggestions, selectedIndex }: { suggestions: SuggestionItem[]; selectedIndex: number }) {
  if (suggestions.length === 0) return null;
  return (
    <Box flexDirection="column">
      {suggestions.map((s, i) => {
        const selected = i === selectedIndex;
        return (
          <Box key={s.id}>
            <Text color={selected ? theme.suggestion : theme.subtle}>{selected ? '▸ ' : '  '}</Text>
            <Text color={selected ? theme.text : theme.inactive} bold={selected}>{s.displayText}</Text>
            <Text color={theme.subtle}> {s.description}</Text>
          </Box>
        );
      })}
    </Box>
  );
}
