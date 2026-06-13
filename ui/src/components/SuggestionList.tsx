// components/SuggestionList.tsx — Claude Code-style slash command list
// Features: column alignment, en-dash separator, scrollable window, dimColor
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import type { SuggestionItem } from '../core/types.js';

const MAX_VISIBLE = 6;

export function SuggestionList({
  suggestions,
  selectedIndex,
}: {
  suggestions: SuggestionItem[];
  selectedIndex: number;
}) {
  if (suggestions.length === 0) return null;

  // Compute stable column width from all suggestions (prevents layout shift)
  const maxDisplayWidth = Math.max(...suggestions.map(s => s.displayText.length));

  // Scrollable window: keep selected item centered
  const start = Math.max(0, Math.min(
    selectedIndex - Math.floor(MAX_VISIBLE / 2),
    suggestions.length - MAX_VISIBLE,
  ));
  const end = Math.min(start + MAX_VISIBLE, suggestions.length);
  const visible = suggestions.slice(start, end);

  return (
    <Box flexDirection="column" justifyContent="flex-end">
      {visible.map((s, i) => {
        const realIdx = start + i;
        const selected = realIdx === selectedIndex;
        const padding = ' '.repeat(Math.max(0, maxDisplayWidth - s.displayText.length));

        return (
          <Box key={s.id}>
            <Text color={selected ? theme.suggestion : theme.subtle}>
              {selected ? '▸ ' : '  '}
            </Text>
            <Text color={selected ? theme.suggestion : undefined} dimColor={!selected}>
              {s.displayText}
              {padding}
            </Text>
            {s.description && (
              <>
                <Text dimColor> – </Text>
                <Text dimColor>{s.description}</Text>
              </>
            )}
          </Box>
        );
      })}
    </Box>
  );
}
