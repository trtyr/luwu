import { describe, test, expect } from 'bun:test';
import { filterCommands, truncateText, LAYOUT, COMMANDS } from '../src/core/constants.ts';
import { canTransition } from '../src/core/state.ts';

describe('filterCommands edge cases', () => {
  test('empty returns all', () => {
    expect(filterCommands('').length).toBe(COMMANDS.length);
  });
  test('/he returns help', () => {
    expect(filterCommands('/he').some(c => c.name === 'help')).toBe(true);
  });
  test('/s returns multiple', () => {
    expect(filterCommands('/s').length).toBeGreaterThanOrEqual(3);
  });
  test('/xyz returns empty', () => {
    expect(filterCommands('/xyz').length).toBe(0);
  });
  test('alias q matches exit', () => {
    expect(filterCommands('/q').some(c => c.name === 'exit')).toBe(true);
  });
});

describe('truncateText boundaries', () => {
  test('short text unchanged', () => {
    expect(truncateText('hello')).toBe('hello');
  });
  test('exactly 10k unchanged', () => {
    expect(truncateText('a'.repeat(10000))).toBe('a'.repeat(10000));
  });
  test('over 10k truncated', () => {
    expect(truncateText('a'.repeat(10001)).length).toBeLessThan(10001);
  });
});

describe('LAYOUT extra constants', () => {
  test('MIN_THINKING_DISPLAY is 2000', () => {
    expect(LAYOUT.MIN_THINKING_DISPLAY).toBe(2000);
  });
});

describe('canTransition extra', () => {
  test('connecting→error', () => { expect(canTransition('connecting', 'error')).toBe(true); });
  test('error→connecting', () => { expect(canTransition('error', 'connecting')).toBe(true); });
  test('streaming→thinking', () => { expect(canTransition('streaming', 'thinking')).toBe(true); });
  test('thinking→streaming', () => { expect(canTransition('thinking', 'streaming')).toBe(true); });
});
