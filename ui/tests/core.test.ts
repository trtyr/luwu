import { describe, test, expect } from 'bun:test';
import {
  filterCommands, truncateText, LAYOUT, COMMANDS,
  estimateCost, getModelCost, formatCost,
} from '../src/core/constants.ts';
import { canTransition, isBusy, showSpinner } from '../src/core/state.ts';

describe('filterCommands', () => {
  test('empty string returns all commands', () => {
    expect(filterCommands('').length).toBe(COMMANDS.length);
  });

  test('/h matches help', () => {
    const result = filterCommands('/h');
    expect(result.some(c => c.name === 'help')).toBe(true);
  });

  test('/cl matches clear', () => {
    const result = filterCommands('/cl');
    expect(result.some(c => c.name === 'clear')).toBe(true);
  });

  test('/x returns empty', () => {
    expect(filterCommands('/x')).toEqual([]);
  });
});

describe('truncateText', () => {
  test('short text unchanged', () => {
    expect(truncateText('hello')).toBe('hello');
  });

  test('long text gets truncated with head+tail', () => {
    const long = 'a'.repeat(12000);
    const result = truncateText(long);
    expect(result.length).toBeLessThan(long.length);
    expect(result).toContain('… +');
    expect(result).toContain('lines …');
  });
});

describe('LAYOUT constants', () => {
  test('RESPONSE_INDENT is correct', () => {
    expect(LAYOUT.RESPONSE_INDENT).toBe('  ⎿  ');
  });

  test('ASSISTANT_DOT is platform-correct circle', () => {
    const expected = process.platform === 'darwin' ? '⏺' : '●';
    expect(LAYOUT.ASSISTANT_DOT).toBe(expected);
  });

  test('DOT_MIN_WIDTH is 2', () => {
    expect(LAYOUT.DOT_MIN_WIDTH).toBe(2);
  });
});

describe('canTransition', () => {
  test('connecting→ready', () => { expect(canTransition('connecting', 'ready')).toBe(true); });
  test('ready→thinking', () => { expect(canTransition('ready', 'thinking')).toBe(true); });
  test('ready→connecting is invalid', () => { expect(canTransition('ready', 'connecting')).toBe(false); });
  test('thinking→ready', () => { expect(canTransition('thinking', 'ready')).toBe(true); });
  test('error→ready', () => { expect(canTransition('error', 'ready')).toBe(true); });
});

describe('isBusy', () => {
  test('thinking is busy', () => { expect(isBusy('thinking')).toBe(true); });
  test('streaming is busy', () => { expect(isBusy('streaming')).toBe(true); });
  test('ready is not busy', () => { expect(isBusy('ready')).toBe(false); });
  test('connecting is not busy', () => { expect(isBusy('connecting')).toBe(false); });
  test('error is not busy', () => { expect(isBusy('error')).toBe(false); });
});

describe('showSpinner', () => {
  test('thinking shows spinner', () => { expect(showSpinner('thinking')).toBe(true); });
  test('ready does not', () => { expect(showSpinner('ready')).toBe(false); });
  test('streaming does not', () => { expect(showSpinner('streaming')).toBe(false); });
});

describe('COMMANDS', () => {
  test('has at least 7 commands', () => {
    expect(COMMANDS.length).toBeGreaterThanOrEqual(7);
  });

  test('includes required commands', () => {
    const names = COMMANDS.map(c => c.name);
    expect(names).toContain('help');
    expect(names).toContain('clear');
    expect(names).toContain('model');
    expect(names).toContain('stats');
    expect(names).toContain('skills');
    expect(names).toContain('sessions');
    expect(names).toContain('exit');
  });
});

describe('getModelCost', () => {
  test('DeepSeek variants map to deepseek rates', () => {
    const c = getModelCost('deepseek-v4-flash');
    expect(c.hit).toBe(0.0028);
    expect(c.miss).toBe(0.14);
  });

  test('GLM variants map to glm rates', () => {
    expect(getModelCost('glm-4.7').hit).toBe(0.5);
    expect(getModelCost('GLM-5').miss).toBe(2.0);
    expect(getModelCost('z-ai/glm-4.6').hit).toBe(0.5);
  });

  test('MiniMax variants map to minimax rates', () => {
    expect(getModelCost('MiniMax-M3').hit).toBe(0.2);
    expect(getModelCost('abab-6').miss).toBe(1.0);
  });

  test('Unknown model falls back to default', () => {
    const c = getModelCost('gpt-4o');
    expect(c.miss).toBe(5.0);
    expect(c.output).toBe(15.0);
  });

  test('Case-insensitive matching', () => {
    expect(getModelCost('DeepSeek-Chat').hit).toBe(0.0028);
    expect(getModelCost('GLM-4.7').miss).toBe(2.0);
  });
});

describe('estimateCost', () => {
  test('No cache → effective = raw, saved = 0', () => {
    const est = estimateCost({
      prompt_tokens: 100_000,
      prompt_cache_miss_tokens: 100_000,
      prompt_cache_hit_tokens: 0,
      completion_tokens: 10_000,
    }, 'deepseek-v4-flash');
    // 100k miss @ $0.14 + 10k output @ $0.28 = 0.014 + 0.0028 = $0.0168
    expect(est.effective).toBeCloseTo(0.0168, 5);
    expect(est.raw).toBeCloseTo(0.0168, 5);
    expect(est.saved).toBeCloseTo(0, 5);
    expect(est.savedPct).toBe(0);
  });

  test('80% cache hit on DeepSeek saves ~80% of prompt cost', () => {
    const est = estimateCost({
      prompt_tokens: 100_000,
      prompt_cache_hit_tokens: 80_000,
      prompt_cache_miss_tokens: 20_000,
      completion_tokens: 10_000,
    }, 'deepseek-v4-flash');
    // effective: 80k @ $0.0028 + 20k @ $0.14 + 10k @ $0.28
    //         = 0.000224 + 0.0028 + 0.0028 = 0.005824
    // raw:     (80k+20k) @ $0.14 + 10k @ $0.28 = 0.014 + 0.0028 = 0.0168
    // saved:   0.0168 - 0.005824 = 0.010976
    // savedPct: 65.3%
    expect(est.effective).toBeCloseTo(0.005824, 5);
    expect(est.raw).toBeCloseTo(0.0168, 5);
    expect(est.saved).toBeCloseTo(0.010976, 5);
    expect(est.savedPct).toBeGreaterThan(60);
    expect(est.savedPct).toBeLessThan(70);
  });

  test('GLM with hit but no miss reported infers miss from prompt-hit', () => {
    const est = estimateCost({
      prompt_tokens: 100_000,
      prompt_cache_hit_tokens: 50_000,
      // no prompt_cache_miss_tokens reported
      completion_tokens: 10_000,
    }, 'glm-4.7');
    // inferred miss = 100k - 50k = 50k
    // effective: 50k @ $0.5 + 50k @ $2.0 + 10k @ $2.0
    //         = 0.025 + 0.1 + 0.02 = 0.145
    // raw: (50k+50k) @ $2.0 + 10k @ $2.0 = 0.2 + 0.02 = 0.22
    // saved: 0.075 → 34%
    expect(est.effective).toBeCloseTo(0.145, 5);
    expect(est.raw).toBeCloseTo(0.22, 5);
    expect(est.saved).toBeCloseTo(0.075, 5);
    expect(est.savedPct).toBeGreaterThanOrEqual(34);
    expect(est.savedPct).toBeLessThanOrEqual(35);
  });

  test('Empty usage → zero cost', () => {
    const est = estimateCost({}, 'deepseek-v4-flash');
    expect(est.effective).toBe(0);
    expect(est.raw).toBe(0);
    expect(est.saved).toBe(0);
    expect(est.savedPct).toBe(0);
  });

  test('MiniMax 100% cache hit is nearly free', () => {
    const est = estimateCost({
      prompt_tokens: 1_000_000,
      prompt_cache_hit_tokens: 1_000_000,
      prompt_cache_miss_tokens: 0,
      completion_tokens: 0,
    }, 'MiniMax-M3');
    // effective: 1M @ $0.2 = $0.2
    // raw: 1M @ $1.0 = $1.0
    // saved: $0.8 (80%)
    expect(est.effective).toBeCloseTo(0.2, 5);
    expect(est.raw).toBeCloseTo(1.0, 5);
    expect(est.saved).toBeCloseTo(0.8, 5);
    expect(est.savedPct).toBe(80);
  });
});

describe('formatCost', () => {
  test('zero returns $0', () => {
    expect(formatCost(0)).toBe('$0');
  });

  test('tiny amount uses micro format', () => {
    expect(formatCost(0.00005)).toBe('<$0.0001');
  });

  test('sub-cent uses 3 decimals', () => {
    expect(formatCost(0.003)).toBe('$0.003');
  });

  test('sub-dollar uses 2 decimals', () => {
    expect(formatCost(0.5)).toBe('$0.50');
  });

  test('normal range uses 2 decimals', () => {
    expect(formatCost(12.34)).toBe('$12.34');
  });

  test('large range uses k notation', () => {
    expect(formatCost(1500)).toBe('$1.5k');
  });
});
