import React, { useState, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';

interface PromptInputProps {
  onSubmit: (text: string) => void;
  disabled: boolean;
  phase: string;
}

/**
 * Bottom input prompt — Claude Code style
 * Single-line input with colored ">" prefix, no external deps
 */
export function PromptInput({ onSubmit, disabled, phase }: PromptInputProps) {
  const [value, setValue] = useState('');

  useInput(useCallback((input, key) => {
    if (disabled) return;

    // Enter — submit
    if (key.return) {
      if (value.trim().length > 0) {
        onSubmit(value.trim());
        setValue('');
      }
      return;
    }

    // Backspace
    if (key.backspace || key.delete) {
      setValue(v => v.slice(0, -1));
      return;
    }

    // Ctrl+U — clear line
    if (key.ctrl && input === 'u') {
      setValue('');
      return;
    }

    // Regular character
    if (input && !key.ctrl && !key.meta && input.length === 1 && input >= ' ') {
      setValue(v => v + input);
    }
  }, [disabled, value, onSubmit]));

  const placeholder = disabled
    ? phase === 'thinking' ? 'thinking…' : 'busy…'
    : 'send a message';

  return (
    <Box marginTop={1}>
      <Text color={disabled ? theme.subtle : theme.suggestion} bold>{'> '}</Text>
      {value.length > 0 ? (
        <Text color={theme.text}>{value}</Text>
      ) : (
        <Text color={theme.subtle} italic>{placeholder}</Text>
      )}
      {!disabled && <Text color={theme.claude}>{'▏'}</Text>}
    </Box>
  );
}
