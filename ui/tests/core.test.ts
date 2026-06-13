import { describe, test, expect } from 'bun:test';
import { filterCommands, truncateText, LAYOUT, COMMANDS } from '../src/core/constants.ts';
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
