// components/PromptInput.tsx — Claude Code 1:1 input
// Source: docs/16-input-box-detailed-ui.md, docs/32-copy-paste-ui.md
//
// Border: round, top+bottom only (no left/right), always promptBorder
// Pointer: ❯ figures.pointer in subtle color (not permission blue)
// Cursor: inverse video block ▎ in claude orange
//
// Paste: dual-signal detection (bracketed paste + length fallback)
// Large pastes (>800 chars or >2 lines) chip-ified as [↵ N lines]

import React, { useState, useCallback, useRef } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';
import { useHistory } from '../hooks/useHistory.js';
import { useSuggestion } from '../hooks/useSuggestion.js';
import { usePasteHandler, type PasteResult } from '../hooks/usePasteHandler.js';
import { SuggestionList } from './SuggestionList.js';

// ── Chip types ──
interface PasteChip {
  id: number;
  lineCount: number;
  charCount: number;
  content: string;
}

interface PromptInputProps {
  value: string;
  onValueChange: (v: string) => void;
  onSubmit: (text: string) => void;
  onCommand: (cmd: string) => void;
  disabled: boolean;
  phase: string;
}

// Chip format: [↵ N lines] embedded in the input text
const CHIP_PATTERN = /\[↵\s*(\d+)\s*lines?\]/g;

export function PromptInput({ value, onValueChange, onSubmit, onCommand, disabled, phase }: PromptInputProps) {
  const history = useHistory();
  const { suggestions, selectedIndex, selectUp, selectDown, isVisible, selectedSuggestion } = useSuggestion(value);
  const [chips, setChips] = useState<Map<number, PasteChip>>(new Map());
  const nextChipId = useRef(1);

  const setValue = onValueChange;

  // ── Paste handler ──
  const handlePaste = useCallback((result: PasteResult) => {
    if (result.isLarge) {
      // Chip-ify: show [↵ N lines] reference, store content separately
      const id = nextChipId.current++;
      setChips(prev => new Map(prev).set(id, {
        id,
        lineCount: result.lineCount,
        charCount: result.text.length,
        content: result.text,
      }));
      const chipLabel = result.lineCount > 1
        ? `[↵ ${result.lineCount} lines]`
        : `[↵ ${result.text.length} chars]`;
      setValue(value + chipLabel);
    } else {
      // Short paste — insert directly (single-line, spaces for newlines)
      const singleLine = result.text.replace(/\n/g, ' ');
      setValue(value + singleLine);
    }
  }, [value, setValue]);

  const { isPasteActive, justPastedRef, checkInput } = usePasteHandler(handlePaste, !disabled);

  // ── On submit: expand chips before sending ──
  const expandChips = useCallback((text: string): string => {
    let result = text;
    const chipArray = Array.from(chips.values());
    let chipIdx = 0;
    result = result.replace(CHIP_PATTERN, () => {
      if (chipIdx < chipArray.length) {
        return chipArray[chipIdx++]!.content;
      }
      return '';
    });
    // Clear chips after submit
    setChips(new Map());
    return result;
  }, [chips]);

  const completeSuggestion = useCallback(() => {
    if (selectedSuggestion) {
      setValue(selectedSuggestion.displayText + ' ');
      return true;
    }
    return false;
  }, [selectedSuggestion, setValue]);

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

      // Normal submit — expand chips before sending
      const expanded = expandChips(trimmed);
      setValue('');
      history.push(expanded);
      if (expanded.startsWith('/')) onCommand(expanded);
      else onSubmit(expanded);
      return;
    }

    // History (only when no suggestions)
    if (!isVisible && key.upArrow) { setValue(history.up(value)); return; }
    if (!isVisible && key.downArrow) { setValue(history.down(value)); return; }

    // Backspace
    if (key.backspace || key.delete) {
      // Check if we're deleting a chip reference
      const chipsInInput = [...value.matchAll(CHIP_PATTERN)];
      if (chipsInInput.length > 0) {
        const lastChip = chipsInInput[chipsInInput.length - 1]!;
        const afterChip = value.slice(lastChip.index! + lastChip[0]!.length);
        if (afterChip.length === 0) {
          // Cursor is right after a chip — delete the whole chip
          const before = value.slice(0, lastChip.index);
          // Extract chip ID from the map (by order)
          const chipValues = Array.from(chips.values());
          const chipToDelete = chipValues[chipsInInput.length - 1];
          if (chipToDelete) {
            setChips(prev => {
              const next = new Map(prev);
              next.delete(chipToDelete.id);
              return next;
            });
          }
          setValue(before);
          return;
        }
      }
      setValue([...value].slice(0, -1).join(''));
      return;
    }

    // Ctrl+U clear (also clears chips)
    if (key.ctrl && input === 'u') {
      setChips(new Map());
      setValue('');
      return;
    }

    // ── Paste detection ──
    // Check if input was consumed by paste handler
    if (checkInput(input)) return;

    // If bracketed paste is active or just finished, skip normal char processing
    if (isPasteActive.current || justPastedRef.current) return;

    // Printable (CJK-safe) character input
    if (!key.ctrl && !key.meta && !key.escape && !key.tab && !key.return && !key.upArrow && !key.downArrow) {
      const filtered = [...input]
        .filter(ch => {
          const code = ch.codePointAt(0)!;
          return code >= 0x20 || ch === '\t';
        })
        .join('')
        .replace(/\t/g, '    ')
        .replace(/\r\n?/g, ' ')
        .replace(/\n/g, ' ');
      if (filtered.length > 0) setValue(value + filtered);
    }
  }, [disabled, value, isVisible, selectedSuggestion, history, selectUp, selectDown, completeSuggestion, setValue, onSubmit, onCommand, checkInput, isPasteActive, justPastedRef, expandChips, chips]));

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
        {/* Claude Code ModeIndicator: ❯ pointer in subtle color */}
        <Text color={theme.subtle} bold>{'❯ '}</Text>
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
