// hooks/useCommands.ts — slash command execution
import { useCallback } from 'react';
import { getStats, getSkills, listSessions, getModels } from '../services/api.js';
import { COMMANDS } from '../core/constants.js';
import type { CommandDef } from '../core/types.js';

export type CommandResult =
  | { type: 'message'; content: string }
  | { type: 'clear' }
  | { type: 'exit' }
  | { type: 'setModel'; model: string; content: string };

export function useCommands(model: string, setModel: (m: string) => void) {
  const executeCommand = useCallback(async (raw: string): Promise<CommandResult> => {
    const parts = raw.slice(1).split(/\s+/);
    const cmd = parts[0]?.toLowerCase();
    const arg = parts[1]; // optional argument

    switch (cmd) {
      case 'help': case 'h': case '?':
        return {
          type: 'message',
          content: [
            '快捷键:',
            '  ↑ ↓     浏览历史消息',
            '  /       打开命令补全',
            '  Tab     确认补全',
            '  Esc     中断当前请求',
            '  Ctrl+O  展开/折叠推理过程',
            '  Ctrl+C  中断请求 / 退出',
            '  Ctrl+U  清空输入',
            '',
            '命令:',
            ...COMMANDS.map(c => `  /${c.name.padEnd(10)} ${c.description}`),
          ].join('\n'),
        };

      case 'clear': case 'cls':
        return { type: 'clear' };

      case 'model': {
        // /model with arg → switch to that model
        if (arg) {
          setModel(arg);
          return { type: 'setModel', model: arg, content: `已切换到模型: ${arg}` };
        }
        // /model without arg → list all available models
        try {
          const models = await getModels();
          if (models.length === 0) {
            return { type: 'message', content: '⚠ 服务器没有可用模型' };
          }
          const lines = models.map((m, i) => {
            const marker = m.id === model ? ' ← 当前' : '';
            return `  ${i + 1}. ${m.id}${marker}`;
          });
          lines.push('');
          lines.push('输入 /model <名称> 切换模型');
          return { type: 'message', content: `可用模型:\n${lines.join('\n')}` };
        } catch {
          return { type: 'message', content: `当前模型: ${model}\n⚠ 无法获取模型列表` };
        }
      }

      case 'stats':
        try {
          const s = await getStats();
          return {
            type: 'message',
            content: [
              `Sessions: ${s.sessions.total} total, ${s.sessions.running} running`,
              `Workers:  ${s.workers}`,
            ].join('\n'),
          };
        } catch { return { type: 'message', content: '⚠ 无法获取统计信息' }; }

      case 'skills':
        try {
          const skills = await getSkills();
          return {
            type: 'message',
            content: skills.length > 0
              ? skills.map(s => `  ${s.name}: ${s.description ?? ''}`).join('\n')
              : '(无可用技能)',
          };
        } catch { return { type: 'message', content: '⚠ 无法获取技能列表' }; }

      case 'sessions': case 'ls':
        try {
          const sessions = await listSessions();
          return {
            type: 'message',
            content: sessions.length > 0
              ? sessions.slice(0, 10).map(s =>
                  `  ${s.id.slice(0, 8)}… [${s.model}] ${s.message_count} msgs${s.is_running ? ' (running)' : ''}`
                ).join('\n')
              : '(无会话)',
          };
        } catch { return { type: 'message', content: '⚠ 无法获取会话列表' }; }

      case 'exit': case 'quit': case 'q':
        return { type: 'exit' };

      default:
        return { type: 'message', content: `未知命令: /${cmd} — 输入 /help 查看可用命令` };
    }
  }, [model, setModel]);

  const getCommandList = useCallback((): CommandDef[] => COMMANDS, []);

  return { executeCommand, getCommandList };
}
