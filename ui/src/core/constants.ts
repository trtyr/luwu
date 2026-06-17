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

// ─────────────────────────────────────────────────────────────────
// Cost per million tokens (USD). Used by StatusLine to show real
// cost spent and cost saved by prefix caching. Updated 2026-06.
//
// Sources (approximate, can drift — users can override per-model
// in their config if needed):
// - DeepSeek V4: hit $0.0028 / miss $0.14 / output $0.28 per MTok
//   (1/50 hit ratio is the headline number; output is ~2x miss)
// - GLM/智谱: hit $0.5 / miss $2.0 / output $2.0 per MTok
//   (1/4 hit ratio on Coding Plan; output ~= miss)
// - MiniMax: hit $0.2 / miss $1.0 / output $1.0 per MTok
//   (1/5 hit ratio; output ~= miss)
// - OpenAI 4o fallback: hit $0.5 / miss $5.0 / output $15.0 per MTok
//   (conservative; used when model is unknown)
//
// `default` is the fallback for unknown models. The TUI falls back
// to this when `contextWindowFor` also can't identify the model.
// ─────────────────────────────────────────────────────────────────
export interface ModelCost {
  /// Cost per million cache-hit prompt tokens
  hit: number;
  /// Cost per million cache-miss prompt tokens
  miss: number;
  /// Cost per million completion tokens
  output: number;
}

export const MODEL_COSTS: Record<string, ModelCost> = {
  deepseek: { hit: 0.0028, miss: 0.14, output: 0.28 },
  glm: { hit: 0.5, miss: 2.0, output: 2.0 },
  minimax: { hit: 0.2, miss: 1.0, output: 1.0 },
  default: { hit: 0.5, miss: 5.0, output: 15.0 },
};

/// Look up per-million-token cost rates for a model. Matches on
/// substring (case-insensitive) to cover all naming conventions:
/// deepseek-chat / deepseek-v4-flash, glm-4.7 / z-ai/glm-4.6,
/// MiniMax-M3 / abab-6, etc.
export function getModelCost(model: string): ModelCost {
  const m = model.toLowerCase();
  if (m.includes('deepseek')) return MODEL_COSTS.deepseek;
  if (m.includes('glm') || m.includes('z-')) return MODEL_COSTS.glm;
  if (m.includes('minimax') || m.includes('abab')) return MODEL_COSTS.minimax;
  return MODEL_COSTS.default;
}

export interface CostEstimate {
  /// Cost with cache applied (hit at hit rate, miss at miss rate)
  effective: number;
  /// Cost as if NO caching (all prompt tokens at miss rate)
  raw: number;
  /// Amount saved by caching (raw - effective, always >= 0)
  saved: number;
  /// Percentage saved (0-100), useful for the badge
  savedPct: number;
}

/// Compute cost for a single LLM call's usage stats, in USD.
///
/// `prompt_cache_miss_tokens` is reported by DeepSeek (V4 flat fields)
/// and inferred for GLM/OpenAI (which only report prompt_tokens and
/// prompt_tokens_details.cached_tokens — for those, miss is computed
/// as `prompt_tokens - hit`). If neither hit nor miss is known, we
/// treat the full prompt as miss.
export function estimateCost(
  usage: {
    prompt_cache_hit_tokens?: number;
    prompt_cache_miss_tokens?: number;
    prompt_tokens?: number;
    completion_tokens?: number;
  },
  model: string,
): CostEstimate {
  const c = getModelCost(model);
  const hit = usage.prompt_cache_hit_tokens ?? 0;
  let miss = usage.prompt_cache_miss_tokens ?? 0;
  const output = usage.completion_tokens ?? 0;
  const prompt = usage.prompt_tokens ?? 0;

  // If miss wasn't reported but hit and prompt were, infer miss.
  if (miss === 0 && hit > 0 && prompt > 0) {
    miss = Math.max(0, prompt - hit);
  }

  // Effective cost: hit at hit rate, miss at miss rate, output at output rate
  const effective =
    (hit / 1_000_000) * c.hit +
    (miss / 1_000_000) * c.miss +
    (output / 1_000_000) * c.output;

  // Raw cost: all prompt tokens at miss rate (no cache savings)
  const totalPrompt = hit + miss;
  const raw =
    (totalPrompt / 1_000_000) * c.miss +
    (output / 1_000_000) * c.output;

  const saved = Math.max(0, raw - effective);
  const savedPct = raw > 0 ? Math.min(100, Math.round((saved / raw) * 100)) : 0;

  return { effective, raw, saved, savedPct };
}

/// Format a dollar amount for the status bar badge.
/// Uses compact notation: <$0.0001 for tiny, $0.003 for small,
/// $1.23 for normal, $1.2k for large.
export function formatCost(usd: number): string {
  if (usd <= 0) return '$0';
  if (usd < 0.0001) return '<$0.0001';
  if (usd < 1) return `$${usd.toFixed(usd < 0.01 ? 3 : 2)}`;
  if (usd < 1000) return `$${usd.toFixed(2)}`;
  return `$${(usd / 1000).toFixed(1)}k`;
}
