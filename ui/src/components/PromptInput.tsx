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
 * Cursor sits at the START when empty (placeholder behind it),
 * moves to the END of typed text as you type.
 * IME-friendly: accepts multi-byte CJK composed input.
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

    // Backspace / Delete
    if (key.backspace || key.delete) {
      setValue(v => v.slice(0, -1));
      return;
    }

    // Ctrl+U — clear line
    if (key.ctrl && input === 'u') {
      setValue('');
      return;
    }

    // Ctrl+C handled at App level

    // Accept any printable input — IME-composed CJK arrives as
    // a single multi-byte string, so don't filter by length.
    // Skip control chars and escape sequences.
    if (!key.ctrl && !key.meta && !key.escape && !key.tab) {
      // Filter out pure control characters (code < 0x20 except space)
      // but accept everything else including multi-char IME results
      const isPrintable =
        input.length > 0 &&
        [...input].every(ch => {
          const code = ch.codePointAt(0)!;
          return code >= 0x20 || ch === ' ';
        });
      if (isPrintable) {
        setValue(v => v + input);
      }
    }
  }, [disabled, value, onSubmit]));

  const placeholder = disabled
    ? phase === 'thinking' ? 'thinking…' : 'busy…'
    : 'send a message';

  return (
    <Box marginTop={1}>
      <Text color={disabled ? theme.subtle : theme.suggestion} bold>{'> '}</Text>

      {value.length > 0 ? (
        // Typed text + cursor at end
        <Text>
          <Text color={theme.text}>{value}</Text>
          {!disabled && <Text color={theme.claude}>▏</Text>}
        </Text>
      ) : (
        // Empty: cursor FIRST, then dimmed placeholder behind it
        <Text>
          {!disabled && <Text color={theme.claude}>▏</Text>}
          <Text color={theme.subtle} italic> {placeholder}</Text>
        </Text>
      )}
    </Box>
  );
}
