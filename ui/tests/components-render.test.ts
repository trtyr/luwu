import { describe, test, expect } from 'bun:test';
import { COMMANDS, LAYOUT, truncateText, computeCachePercent } from '../src/core/constants.ts';
import { canTransition, isBusy, showSpinner } from '../src/core/state.ts';
import type { Phase } from '../src/core/types.ts';

describe('COMMANDS integrity', () => {
  test('every command has name + description', () => {
    for (const cmd of COMMANDS) {
      expect(cmd.name).toBeTruthy();
      expect(cmd.description).toBeTruthy();
    }
  });
  test('all names unique', () => {
    const names = COMMANDS.map(c => c.name);
    expect(new Set(names).size).toBe(names.length);
  });
  test('aliases are arrays if present', () => {
    for (const cmd of COMMANDS) {
      if (cmd.aliases !== undefined) expect(Array.isArray(cmd.aliases)).toBe(true);
    }
  });
});

describe('LAYOUT numeric constants', () => {
  test('DOT_MIN_WIDTH positive', () => { expect(LAYOUT.DOT_MIN_WIDTH).toBeGreaterThan(0); });
  test('MAX_DISPLAY_CHARS positive', () => { expect(LAYOUT.MAX_DISPLAY_CHARS).toBeGreaterThan(0); });
  test('MIN_THINKING_DISPLAY positive', () => { expect(LAYOUT.MIN_THINKING_DISPLAY).toBeGreaterThan(0); });
  test('RESPONSE_INDENT contains ⎿', () => { expect(LAYOUT.RESPONSE_INDENT).toContain('⎿'); });
});

describe('isBusy all phases', () => {
  const phases: Phase[] = ['connecting', 'ready', 'thinking', 'streaming', 'error'];
  for (const p of phases) {
    test(`${p} returns boolean`, () => { expect(typeof isBusy(p)).toBe('boolean'); });
  }
});

describe('showSpinner all phases', () => {
  const phases: Phase[] = ['connecting', 'ready', 'thinking', 'streaming', 'error'];
  for (const p of phases) {
    test(`${p} returns boolean`, () => { expect(typeof showSpinner(p)).toBe('boolean'); });
  }
});

describe('truncateText edge cases', () => {
  test('empty string', () => { expect(truncateText('')).toBe(''); });
  test('exactly at boundary unchanged', () => { expect(truncateText('x'.repeat(10000))).toBe('x'.repeat(10000)); });
  test('over boundary is shorter', () => { expect(truncateText('x'.repeat(10001)).length).toBeLessThan(10001); });
});

describe('canTransition self-transitions', () => {
  const phases: Phase[] = ['connecting', 'ready', 'thinking', 'streaming', 'error'];
  for (const p of phases) {
    test(`${p}→${p} is false`, () => { expect(canTransition(p, p)).toBe(false); });
  }
});

describe('computeCachePercent', () => {
  // First-turn / no-data scenarios — badge should NOT show (returns 0).
  test('first turn: 0 hits, real context → 0%', () => {
    expect(computeCachePercent(0, 1000)).toBe(0);
  });
  test('undefined cacheHit → 0%', () => {
    expect(computeCachePercent(undefined, 1000)).toBe(0);
  });
  test('undefined contextTokens → 0%', () => {
    expect(computeCachePercent(500, undefined)).toBe(0);
  });
  test('zero contextTokens → 0%', () => {
    expect(computeCachePercent(500, 0)).toBe(0);
  });
  test('negative cacheHit → 0%', () => {
    expect(computeCachePercent(-100, 1000)).toBe(0);
  });
  test('both undefined → 0%', () => {
    expect(computeCachePercent(undefined, undefined)).toBe(0);
  });

  // Real GLM / DeepSeek scenarios — badge SHOULD show.
  test('GLM mid-conversation: 50% cached → 50%', () => {
    expect(computeCachePercent(500, 1000)).toBe(50);
  });
  test('heavy prefix match: 75% cached → 75%', () => {
    expect(computeCachePercent(750, 1000)).toBe(75);
  });
  test('everything cached: 100% → 100%', () => {
    expect(computeCachePercent(1000, 1000)).toBe(100);
  });
  test('small cache: 1% → 1%', () => {
    expect(computeCachePercent(10, 1000)).toBe(1);
  });
  test('rounds 33.7% up to 34%', () => {
    expect(computeCachePercent(337, 1000)).toBe(34);
  });
  test('rounds 33.4% down to 33%', () => {
    expect(computeCachePercent(334, 1000)).toBe(33);
  });

  // Defensive — should never show > 100% even with weird upstream data.
  test('cacheHit > contextTokens (defensive) → capped at 100%', () => {
    expect(computeCachePercent(1500, 1000)).toBe(100);
  });

  // Realistic GLM-4.7 context sizes (128K) — confirms math holds at scale.
  test('GLM-4.7 128K context with 64K cached → 50%', () => {
    expect(computeCachePercent(65536, 131072)).toBe(50);
  });
  test('GLM-4.7 128K context with 128K cached → 100%', () => {
    expect(computeCachePercent(131072, 131072)).toBe(100);
  });
});
