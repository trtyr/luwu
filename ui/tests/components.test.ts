import { describe, test, expect } from 'bun:test';
import { theme } from '../src/theme.js';
import { hasMarkdownSyntax } from '../src/components/Markdown.tsx';

describe('theme', () => {
  test('all keys have valid hex colors', () => {
    for (const [key, value] of Object.entries(theme)) {
      expect(value).toMatch(/^#[0-9A-Fa-f]{6}$/);
    }
  });

  test('claude orange is correct', () => {
    expect(theme.claude).toBe('#D77757');
  });

  test('suggestion is correct (dark theme blue-purple)', () => {
    expect(theme.suggestion).toBe('#B1B9F9');
  });

  test('error red is bright', () => {
    expect(theme.error).toBe('#FF6B80');
  });

  test('success green is bright', () => {
    expect(theme.success).toBe('#4EBA65');
  });

  test('has userMessageBackground', () => {
    expect(theme.userMessageBackground).toBeDefined();
  });
});

describe('hasMarkdownSyntax', () => {
  test('plain text returns false', () => {
    expect(hasMarkdownSyntax('hello world')).toBe(false);
    expect(hasMarkdownSyntax('这是一段纯文本')).toBe(false);
  });

  test('heading returns true', () => {
    expect(hasMarkdownSyntax('# Title')).toBe(true);
    expect(hasMarkdownSyntax('## Subtitle')).toBe(true);
  });

  test('code fence returns true', () => {
    expect(hasMarkdownSyntax('```python')).toBe(true);
  });

  test('bold/italic returns true', () => {
    expect(hasMarkdownSyntax('**bold**')).toBe(true);
    expect(hasMarkdownSyntax('*italic*')).toBe(true);
  });

  test('list returns true', () => {
    expect(hasMarkdownSyntax('1. first')).toBe(true);
    expect(hasMarkdownSyntax('- item')).toBe(true);
  });

  test('blockquote returns true', () => {
    expect(hasMarkdownSyntax('> quote')).toBe(true);
  });

  test('long plain text without markdown returns false', () => {
    const longText = 'a'.repeat(600) + 'b'.repeat(300);
    expect(hasMarkdownSyntax(longText)).toBe(false);
  });

  test('markdown in first 500 chars detected', () => {
    const text = 'a'.repeat(499) + '# heading';
    expect(hasMarkdownSyntax(text)).toBe(true);
  });

  test('markdown after 500 chars not detected (performance tradeoff)', () => {
    const text = 'a'.repeat(501) + '# heading';
    expect(hasMarkdownSyntax(text)).toBe(false);
  });
});
