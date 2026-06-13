import { describe, test, expect } from 'bun:test';
import { COMMANDS, LAYOUT, truncateText } from '../src/core/constants.ts';
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
