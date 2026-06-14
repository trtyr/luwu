// components/SuggestionList.tsx — Claude Code 1:1
// Source: docs/23-slash-command-system.md §5.2
// KEY: NO ❯ pointer — selection shown by COLOR ONLY
//   selected   → suggestion color (text)
//   unselected → inactive color
// ▔ separator line in permission color at top (non-fullscreen inline)
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
      {/* ▔ separator line in permission color */}
      <Text color={theme.permission}>{'▔'.repeat(50)}</Text>
      {visible.map((s, i) => {
        const realIdx = start + i;
        const selected = realIdx === selectedIndex;
        const padding = ' '.repeat(Math.max(0, maxDisplayWidth - s.displayText.length));

        return (
          <Box key={s.id}>
            {/* NO ❯ pointer — color-only differentiation per doc 23 §5.2 */}
            <Text color={selected ? theme.suggestion : theme.inactive}>
              {s.displayText}{padding}
            </Text>
            {s.description && (
              <Text color={selected ? theme.suggestion : theme.inactive}>
                {'  '}{s.description}
              </Text>
            )}
          </Box>
        );
      })}
    </Box>
  );
}
