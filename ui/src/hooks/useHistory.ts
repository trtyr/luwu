// hooks/useHistory.ts — input history with ↑↓ navigation
import { useState, useCallback, useRef } from 'react';

export function useHistory() {
  const historyRef = useRef<string[]>([]);
  const [index, setIndex] = useState(-1);

  const push = useCallback((value: string) => {
    if (value.trim()) historyRef.current.push(value);
    setIndex(-1);
  }, []);

  const up = useCallback((currentInput: string): string => {
    const hist = historyRef.current;
    if (hist.length === 0) return currentInput;
    const newIdx = index === -1 ? hist.length - 1 : Math.max(0, index - 1);
    setIndex(newIdx);
    return hist[newIdx] ?? currentInput;
  }, [index]);

  const down = useCallback((currentInput: string): string => {
    if (index === -1) return currentInput;
    const newIdx = index + 1;
    if (newIdx >= historyRef.current.length) { setIndex(-1); return ''; }
    setIndex(newIdx);
    return historyRef.current[newIdx] ?? '';
  }, [index]);

  const reset = useCallback(() => setIndex(-1), []);

  return { push, up, down, reset };
}
