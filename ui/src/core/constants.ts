// core/constants.ts — Command registry + key mappings (zero dependencies)
import type { CommandDef } from './types.js';

/** All slash commands available in the TUI */
export const COMMANDS: CommandDef[] = [
  { name: 'help', description: '显示帮助信息', aliases: ['h', '?'] },
  { name: 'clear', description: '清空消息历史', aliases: ['cls'] },
  { name: 'new', description: '创建新会话' },
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
  ASSISTANT_DOT: process.platform === 'darwin' ? '⏺' : '●',
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

/**
 * Context window size (max prompt tokens) for known models.
 * Used to compute real context usage % from LLM prompt_tokens.
 * Returns a default of 128K if model is unknown.
 */
export function contextWindowFor(model: string): number {
  const m = model.toLowerCase();
  // ── Anthropic ──
  if (m.includes('claude-4') || m.includes('claude-3.7')) return 200_000;
  if (m.includes('claude-3')) return 200_000;
  // ── OpenAI ──
  if (m.includes('gpt-4o') || m.includes('gpt-4.1')) return 128_000;
  if (m.includes('o1') || m.includes('o3') || m.includes('o4')) return 200_000;
  if (m.includes('gpt-4')) return 8_192;
  if (m.includes('gpt-3.5')) return 16_385;
  // ── Google Gemini ──
  if (m.includes('gemini-2') || m.includes('gemini-1.5')) return 1_000_000;
  if (m.includes('gemini')) return 32_000;
  // ── Zhipu GLM ──
  if (m.includes('glm-4')) return 128_000;
  if (m.includes('glm')) return 128_000;
  // ── MiniMax ──
  if (m.includes('minimax') || m.includes('abab')) return 245_760;
  // ── DeepSeek ──
  if (m.includes('deepseek')) return 64_000;
  // ── Qwen ──
  if (m.includes('qwen')) return 32_768;
  // ── Default ──
  return 128_000;
}

/**
 * Compute the cache-hit percentage for the status bar badge.
 * Returns 0 if no cache data is available, capped at 100.
 *
 * Edge cases:
 * - cacheHit is 0 / undefined     → 0 (no badge shown — no hits yet)
 * - contextTokens is 0 / undefined → 0 (no badge — can't compute %)
 * - cacheHit > contextTokens       → 100 (impossible in practice, but
 *   we cap defensively so the UI never shows > 100%)
 * - negative inputs                → 0 (defensive)
 *
 * The "⚡ XX% cached" badge in StatusLine calls this to decide whether
 * to show. Users only see the badge once they have real cache activity
 * (typically from the 2nd turn onward when prefix caching starts
 * matching previous prompts — first turn is always 0%).
 */
export function computeCachePercent(
  cacheHit: number | undefined,
  contextTokens: number | undefined,
): number {
  if (!cacheHit || !contextTokens || contextTokens <= 0) return 0;
  if (cacheHit < 0) return 0;
  return Math.min(100, Math.round((cacheHit / contextTokens) * 100));
}
