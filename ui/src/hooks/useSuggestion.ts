// hooks/useSuggestion.ts — slash command autocomplete
import { useState, useMemo, useCallback } from 'react';
import { filterCommands } from '../core/constants.js';
import type { SuggestionItem } from '../core/types.js';

export function useSuggestion(input: string) {
  const [selectedIndex, setSelectedIndex] = useState(0);

  const suggestions = useMemo<SuggestionItem[]>(() => {
    if (!input.startsWith('/')) return [];
    const cmds = filterCommands(input);
    return cmds.map(c => ({
      id: c.name,
      displayText: '/' + c.name,
      description: c.description,
    }));
  }, [input]);

  const isVisible = suggestions.length > 0 && input.startsWith('/');

  const selectUp = useCallback(() => {
    if (suggestions.length === 0) return;
    setSelectedIndex(i => (i <= 0 ? suggestions.length - 1 : i - 1));
  }, [suggestions.length]);

  const selectDown = useCallback(() => {
    if (suggestions.length === 0) return;
    setSelectedIndex(i => (i >= suggestions.length - 1 ? 0 : i + 1));
  }, [suggestions.length]);

  // Reset selection when suggestions change
  useMemo(() => setSelectedIndex(0), [suggestions]);

  const selectedSuggestion = isVisible ? suggestions[selectedIndex] : null;

  return { suggestions, selectedIndex, selectUp, selectDown, isVisible, selectedSuggestion };
}
