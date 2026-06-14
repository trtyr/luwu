// hooks/useCommands.ts — slash command execution
// All interactive commands return overlay types (doc 29)
import { useCallback } from 'react';
import { COMMANDS } from '../core/constants.js';
import type { CommandDef } from '../core/types.js';

export type OverlayType = 'help' | 'stats' | 'skills' | 'sessions' | 'model';

export type CommandResult =
  | { type: 'clear' }
  | { type: 'exit' }
  | { type: 'overlay'; overlay: OverlayType }
  | { type: 'setModel'; model: string };

export function useCommands(model: string, setModel: (m: string) => void) {
  const executeCommand = useCallback(async (raw: string): Promise<CommandResult> => {
    const parts = raw.slice(1).split(/\s+/);
    const cmd = parts[0]?.toLowerCase();
    const arg = parts[1];

    switch (cmd) {
      case 'help': case 'h': case '?':
        return { type: 'overlay', overlay: 'help' };

      case 'clear': case 'cls':
        return { type: 'clear' };

      case 'model':
        if (arg) {
          setModel(arg);
          return { type: 'setModel', model: arg };
        }
        return { type: 'overlay', overlay: 'model' };

      case 'stats':
        return { type: 'overlay', overlay: 'stats' };

      case 'skills':
        return { type: 'overlay', overlay: 'skills' };

      case 'sessions': case 'ls':
        return { type: 'overlay', overlay: 'sessions' };

      case 'exit': case 'quit': case 'q':
        return { type: 'exit' };

      default:
        return { type: 'overlay', overlay: 'help' };
    }
  }, [model, setModel]);

  const getCommandList = useCallback((): CommandDef[] => COMMANDS, []);
  return { executeCommand, getCommandList };
}
