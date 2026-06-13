// core/constants.ts — Command registry + key mappings (zero dependencies)
import type { CommandDef } from './types.js';

/** All slash commands available in the TUI */
export const COMMANDS: CommandDef[] = [
  { name: 'help', description: '显示帮助信息', aliases: ['h', '?'] },
  { name: 'clear', description: '清空消息历史', aliases: ['cls'] },
  { name: 'model', description: '显示当前模型' },
  { name: 'stats', description: '显示运行时统计' },
  { name: 'skills', description: '列出可用技能' },
  { name: 'sessions', description: '列出所有会话', aliases: ['ls'] },
  { name: 'exit', description: '退出 luwu', aliases: ['quit', 'q'] },
];

/** Get command suggestions filtered by prefix (simple fuzzy match) */
export function filterCommands(input: string): CommandDef[] {
  const query = input.toLowerCase().replace(/^\//, '');
  if (!query) return COMMANDS;
  return COMMANDS.filter(cmd => {
    if (cmd.name.startsWith(query)) return true;
    if (cmd.aliases?.some(a => a.startsWith(query))) return true;
    return false;
  });
}

/** Message layout constants (from Claude Code analysis) */
export const LAYOUT = {
  /** Indent for MessageResponse: "  ⎿  " = 6 chars */
  RESPONSE_INDENT: '  ⎿  ',
  /** BLACK_CIRCLE for assistant prefix */
  ASSISTANT_DOT: '●',
  /** minWidth for the dot column */
  DOT_MIN_WIDTH: 2,
  /** Max display chars for user message before truncation */
  MAX_DISPLAY_CHARS: 10_000,
  TRUNCATE_HEAD: 2_500,
  TRUNCATE_TAIL: 2_500,
  /** Minimum thinking display time (ms) */
  MIN_THINKING_DISPLAY: 2_000,
  /** Post-thinking duration display time (ms) */
  THINKING_DURATION_DISPLAY: 2_000,
} as const;

/** Helper to truncate long text with head+tail */
export function truncateText(text: string, max = LAYOUT.MAX_DISPLAY_CHARS): string {
  if (text.length <= max) return text;
  const head = text.slice(0, LAYOUT.TRUNCATE_HEAD);
  const tail = text.slice(-LAYOUT.TRUNCATE_TAIL);
  const hiddenLines = text.slice(LAYOUT.TRUNCATE_HEAD, -LAYOUT.TRUNCATE_TAIL).split('\n').length;
  return `${head}\n… +${hiddenLines} lines …\n${tail}`;
}
