// core/state.ts — Phase state machine (zero dependencies)
import type { Phase } from './types.js';

/** Valid phase transitions */
export function canTransition(from: Phase, to: Phase): boolean {
  const transitions: Record<Phase, Phase[]> = {
    connecting: ['ready', 'error'],
    ready: ['thinking', 'error'],
    thinking: ['streaming', 'ready', 'error'],
    streaming: ['ready', 'thinking', 'error'],
    error: ['connecting', 'ready'],
  };
  return transitions[from]?.includes(to) ?? false;
}

/** Whether the UI is in a "busy" state (input disabled) */
export function isBusy(phase: Phase): boolean {
  return phase === 'thinking' || phase === 'streaming';
}

/** Whether to show the spinner */
export function showSpinner(phase: Phase): boolean {
  return phase === 'thinking';
}
