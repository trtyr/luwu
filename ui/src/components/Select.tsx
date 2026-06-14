// Select.tsx — reusable interactive selection list
// Claude Code CustomSelect pattern: ↑↓ navigate, Enter select, Esc cancel
// ❯ marks focused item (used inside Modal Pane only, NOT in SuggestionList)
import React, { useState, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';

export interface SelectOption {
  value: string;
  label: string;
  description?: string;
}

interface SelectProps {
  options: SelectOption[];
  defaultValue?: string;
  onSelect: (value: string) => void;
  onCancel?: () => void;
  visibleCount?: number;
}

const MAX_VISIBLE = 10;

export function Select({ options, defaultValue, onSelect, onCancel, visibleCount = MAX_VISIBLE }: SelectProps) {
  const initialIndex = options.findIndex(o => o.value === defaultValue);
  const [focusedIndex, setFocusedIndex] = useState(initialIndex >= 0 ? initialIndex : 0);

  const maxVisible = Math.min(visibleCount, options.length);
  const scrollOffset = Math.max(0, focusedIndex - Math.floor(maxVisible / 2));
  const visibleStart = Math.min(scrollOffset, Math.max(0, options.length - maxVisible));
  const visibleEnd = Math.min(visibleStart + maxVisible, options.length);
  const visibleOptions = options.slice(visibleStart, visibleEnd);

  useInput(useCallback((input: string, key: any) => {
    if (key.upArrow) {
      setFocusedIndex(prev => Math.max(0, prev - 1));
    } else if (key.downArrow) {
      setFocusedIndex(prev => Math.min(options.length - 1, prev + 1));
    } else if (key.return) {
      const opt = options[focusedIndex];
      if (opt) onSelect(opt.value);
    } else if (key.escape) {
      onCancel?.();
    }
  }, [options, focusedIndex, onSelect, onCancel]));

  return (
    <Box flexDirection="column">
      {visibleOptions.map((opt, i) => {
        const realIndex = visibleStart + i;
        const isFocused = realIndex === focusedIndex;
        return (
          <Box key={`${opt.value}-${realIndex}`}>
            <Text color={isFocused ? theme.text : theme.subtle}>{isFocused ? '❯ ' : '  '}</Text>
            <Text color={isFocused ? theme.text : theme.inactive} bold={isFocused}>
              {opt.label}
            </Text>
            {opt.description && (
              <Text color={theme.subtle}> {opt.description}</Text>
            )}
          </Box>
        );
      })}
      {options.length > maxVisible && (
        <Box paddingLeft={2}>
          <Text color={theme.subtle}>
            {visibleStart > 0 ? '↑ ' : ''}({focusedIndex + 1}/{options.length}){visibleEnd < options.length ? ' ↓' : ''}
          </Text>
        </Box>
      )}
    </Box>
  );
}
