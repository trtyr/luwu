// components/PromptInput.tsx — Claude Code 1:1 input
// Source: docs/16-input-box-detailed-ui.md
// Border: round, top+bottom only (no left/right), always promptBorder
// Pointer: ❯ figures.pointer in subtle color
// Cursor: inverse video block ▎ in claude orange
import React, { useState, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';
import { useHistory } from '../hooks/useHistory.js';
import { useSuggestion } from '../hooks/useSuggestion.js';
import { SuggestionList } from './SuggestionList.js';

interface PromptInputProps {
  onSubmit: (text: string) => void;
  onCommand: (cmd: string) => void;
  disabled: boolean;
  phase: string;
}

export function PromptInput({ onSubmit, onCommand, disabled, phase }: PromptInputProps) {
  const [value, setValue] = useState('');
  const history = useHistory();
  const { suggestions, selectedIndex, selectUp, selectDown, isVisible, selectedSuggestion } = useSuggestion(value);

  const completeSuggestion = useCallback(() => {
    if (selectedSuggestion) {
      setValue(selectedSuggestion.displayText + ' ');
      return true;
    }
    return false;
  }, [selectedSuggestion]);

  useInput(useCallback((input: string, key: any) => {
    if (disabled) return;

    // Suggestion navigation mode
    if (isVisible) {
      if (key.upArrow) { selectUp(); return; }
      if (key.downArrow) { selectDown(); return; }
      if (key.tab) { completeSuggestion(); return; }
    }

    // Enter
    if (key.return) {
      const trimmed = value.trim();
      if (!trimmed) return;

      // If suggestion visible and selected, complete first
      if (isVisible && selectedSuggestion) {
        const completed = selectedSuggestion.displayText;
        setValue('');
        history.push(completed);
        onCommand(completed);
        return;
      }

      // Normal submit
      setValue('');
      history.push(trimmed);
      if (trimmed.startsWith('/')) onCommand(trimmed);
      else onSubmit(trimmed);
      return;
    }

    // History (only when no suggestions)
    if (!isVisible && key.upArrow) { setValue(history.up(value)); return; }
    if (!isVisible && key.downArrow) { setValue(history.down(value)); return; }

    // Backspace
    if (key.backspace || key.delete) {
      setValue(v => [...v].slice(0, -1).join(''));
      return;
    }

    // Ctrl+U clear
    if (key.ctrl && input === 'u') { setValue(''); return; }

    // Printable (CJK-safe)
    if (!key.ctrl && !key.meta && !key.escape && !key.tab && !key.return && !key.upArrow && !key.downArrow) {
      const ok = input.length > 0 && [...input].every(ch => {
        const code = ch.codePointAt(0)!;
        return code >= 0x20 || ch === ' ';
      });
      if (ok) setValue(v => v + input);
    }
  }, [disabled, value, isVisible, selectedSuggestion, history, selectUp, selectDown, completeSuggestion, onSubmit, onCommand]));

  const placeholder = disabled
    ? (phase === 'thinking' ? 'thinking…' : 'busy…')
    : 'send a message (↑↓ history, / for commands)';

  return (
    <Box flexDirection="column" marginTop={1}>
      {isVisible && <SuggestionList suggestions={suggestions} selectedIndex={selectedIndex} />}
      <Box
        borderStyle="round"
        borderColor={theme.promptBorder}
        borderLeft={false}
        borderRight={false}
        borderBottom
        width="100%"
        flexDirection="row"
        alignItems="flex-start"
      >
        {/* Claude Code ModeIndicator: ❯ pointer */}
        <Text color={disabled ? theme.subtle : theme.permission} bold>{'❯ '}</Text>
        <Box flexGrow={1} flexShrink={1}>
          {value.length > 0 ? (
            <Text>
              <Text color={value.startsWith('/') ? theme.warning : theme.text}>{value}</Text>
              {!disabled && <Text color={theme.claude}>{'▎'}</Text>}
            </Text>
          ) : (
            <Text>
              {!disabled && <Text color={theme.claude}>{'▎'}</Text>}
              <Text color={theme.subtle} italic> {placeholder}</Text>
            </Text>
          )}
        </Box>
      </Box>
    </Box>
  );
}
