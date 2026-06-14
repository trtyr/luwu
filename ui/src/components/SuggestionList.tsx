// components/SuggestionList.tsx — Claude Code 1:1 command panel
// Source: docs/20-command-panels-ui.md
// ▔ separator line in permission color at top
// Selected: ❯ prefix in text color
// Unselected: space prefix, inactive color
// dimColor = theme.inactive (NOT ANSI dim)
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

  const maxDisplayWidth = Math.max(...suggestions.map(s => s.displayText.length));

  const start = Math.max(0, Math.min(
    selectedIndex - Math.floor(MAX_VISIBLE / 2),
    suggestions.length - MAX_VISIBLE,
  ));
  const end = Math.min(start + MAX_VISIBLE, suggestions.length);
  const visible = suggestions.slice(start, end);

  return (
    <Box flexDirection="column" justifyContent="flex-end">
      {/* ▔ separator line in permission color (Claude Code 1:1) */}
      <Text color={theme.permission}>{'▔'.repeat(50)}</Text>
      {visible.map((s, i) => {
        const realIdx = start + i;
        const selected = realIdx === selectedIndex;
        const padding = ' '.repeat(Math.max(0, maxDisplayWidth - s.displayText.length));

        return (
          <Box key={s.id}>
            {selected ? (
              <Text color={theme.text}>{'❯ '}</Text>
            ) : (
              <Text>{'  '}</Text>
            )}
            <Text color={selected ? theme.text : theme.inactive}>
              {s.displayText}{padding}
            </Text>
            {s.description && (
              <>
                <Text color={theme.inactive}> – </Text>
                <Text color={theme.inactive}>{s.description}</Text>
              </>
            )}
          </Box>
        );
      })}
    </Box>
  );
}
