// hooks/useCommands.ts — slash command execution
import { useCallback } from 'react';
import { getStats, getSkills, listSessions } from '../services/api.js';
import { COMMANDS } from '../core/constants.js';
import type { CommandDef } from '../core/types.js';

export type CommandResult =
  | { type: 'message'; content: string }
  | { type: 'clear' }
  | { type: 'exit' };

export function useCommands(model: string) {
  const executeCommand = useCallback(async (raw: string): Promise<CommandResult> => {
    const parts = raw.slice(1).split(/\s+/);
    const cmd = parts[0]?.toLowerCase();

    switch (cmd) {
      case 'help': case 'h': case '?':
        return { type: 'message', content: COMMANDS.map(c => `/${c.name} — ${c.description}`).join('\n') };

      case 'clear': case 'cls':
        return { type: 'clear' };

      case 'model':
        return { type: 'message', content: `当前模型: ${model}` };

      case 'stats':
        try {
          const s = await getStats();
          return { type: 'message', content: `Sessions: ${s.sessions.total} total, ${s.sessions.running} running\nWorkers: ${s.workers}` };
        } catch { return { type: 'message', content: '⚠ 无法获取统计信息' }; }

      case 'skills':
        try {
          const skills = await getSkills();
          return { type: 'message', content: skills.length > 0 ? skills.map(s => `  ${s.name}: ${s.description ?? ''}`).join('\n') : '(无可用技能)' };
        } catch { return { type: 'message', content: '⚠ 无法获取技能列表' }; }

      case 'sessions': case 'ls':
        try {
          const sessions = await listSessions();
          return { type: 'message', content: sessions.length > 0 ? sessions.slice(0, 10).map(s => `  ${s.id.slice(0, 8)}… [${s.model}] ${s.message_count} msgs${s.is_running ? ' (running)' : ''}`).join('\n') : '(无会话)' };
        } catch { return { type: 'message', content: '⚠ 无法获取会话列表' }; }

      case 'exit': case 'quit': case 'q':
        return { type: 'exit' };

      default:
        return { type: 'message', content: `未知命令: /${cmd} — 输入 /help 查看可用命令` };
    }
  }, [model]);

  const getCommandList = useCallback((): CommandDef[] => COMMANDS, []);

  return { executeCommand, getCommandList };
}
