// tests/textOverflow.test.ts — Regression tests for splitTextForOverflow().
//
// These tests lock down the line-vs-char offset logic that caused the
// "AI reply shown 2-3 times in scrollback" bug. The pure helper lives at
// src/core/textOverflow.ts and is called by useChatSession's
// commitTextOverflow() to decide where to split a growing text block.
//
// The bug: lines.length - MAX_DYNAMIC_LINES was being used as a CHAR
// OFFSET for text.slice(), but it's a LINE INDEX. For a 20-line text
// with 50 chars/line, splitAt=5 but text.slice(0,5) only takes the
// first 5 CHARACTERS, leaving ~995 chars in the dynamic area and
// triggering the very flicker this code was supposed to prevent.

import { describe, test, expect } from 'bun:test';
import { splitTextForOverflow } from '../src/core/textOverflow.ts';

const MAX_LINES = 15;
const MAX_CHARS = 2000;

describe('splitTextForOverflow — the regression case (P2 char offset bug)', () => {
  // This is the EXACT scenario from the production bug:
  //   20 lines × 50 chars = 1000 chars total + 19 \n = 1019 chars
  //   Old code: splitAt = 20 - 15 = 5 (line index used as char offset)
  //             text.slice(0, 5) = "line0" (5 chars, not 5 lines)
  //   Fixed code: charOffset = 5 * (50 + 1) = 255
  //               text.slice(0, 255) = first 5 lines + their 5 \n
  //               text.slice(255) = last 15 lines (no trailing \n)
  test('20 lines of 50 chars splits at char offset 255, NOT line index 5', () => {
    const line = (n: number) => `line${String(n).padStart(2, '0')}${'x'.repeat(44)}`;
    // line: "line" (4) + "00" (2) + 44 x's = 50 chars per line ✓
    const text = Array.from({ length: 20 }, (_, i) => line(i)).join('\n');
    // 20 lines × 50 chars + 19 newlines = 1000 + 19 = 1019 chars
    expect(text.length).toBe(1019);
    expect(text.split('\n').length).toBe(20);

    const result = splitTextForOverflow(text, MAX_LINES, MAX_CHARS);
    expect(result).not.toBeNull();

    // Content comparison is the most robust assertion — it doesn't
    // depend on whether text has a trailing \n (which would create an
    // empty string in split() and trip up length assertions).
    const expectedEarly = Array.from({ length: 5 }, (_, i) => line(i)).join('\n') + '\n';
    const expectedRecent = Array.from({ length: 15 }, (_, i) => line(i + 5)).join('\n');
    expect(result!.earlyText).toBe(expectedEarly);
    expect(result!.recentText).toBe(expectedRecent);
    // No content lost
    expect(result!.earlyText + result!.recentText).toBe(text);
  });

  test('20 lines of 1 char each: char offset = 5 × 2 = 10', () => {
    // The "easy" case: every line is 1 char, so char offset ≈ 2 × line count.
    //   20 lines × 1 char + 19 newlines = 39 chars
    //   skipLines = 5, charOffset = 5 × (1+1) = 10
    //   text.slice(0, 10) = "a\nb\nc\nd\ne\n" (5 lines + 5 \n)
    //   text.slice(10) = "f\ng\n...t" (15 lines, no trailing \n)
    const text = 'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt';
    expect(text.length).toBe(39);
    expect(text.split('\n').length).toBe(20);

    const result = splitTextForOverflow(text, MAX_LINES, MAX_CHARS);
    expect(result).not.toBeNull();
    expect(result!.earlyText).toBe('a\nb\nc\nd\ne\n');
    expect(result!.recentText).toBe('f\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt');
  });

  // This is the exact example documented in the source code comment.
  test('4 lines of varying length: "a\\nbb\\nccc\\ndddd"', () => {
    const text = 'a\nbb\nccc\ndddd';
    const result = splitTextForOverflow(text, 2, MAX_CHARS);
    expect(result).not.toBeNull();
    // skipLines = 4 - 2 = 2
    // charOffset = (1+1) + (2+1) = 5
    // text.slice(0, 5) = "a\nbb\n" ← correct early part
    // text.slice(5) = "ccc\ndddd" ← correct recent part
    expect(result!.earlyText).toBe('a\nbb\n');
    expect(result!.recentText).toBe('ccc\ndddd');
  });
});

describe('splitTextForOverflow — boundary conditions', () => {
  test('exactly 15 lines (at the boundary) returns null', () => {
    const text = Array.from({ length: 15 }, () => 'x').join('\n');
    expect(text.split('\n').length).toBe(15);
    // lines.length > maxLines (15 > 15) is false → no line-based split
    // text.length < maxChars → no char-based split
    expect(splitTextForOverflow(text, MAX_LINES, MAX_CHARS)).toBeNull();
  });

  test('16 lines (one over the boundary) triggers split', () => {
    const text = Array.from({ length: 16 }, () => 'x').join('\n');
    expect(text.split('\n').length).toBe(16);
    const result = splitTextForOverflow(text, MAX_LINES, MAX_CHARS);
    expect(result).not.toBeNull();
    // skipLines = 1, charOffset = (1+1) = 2
    // text.slice(0, 2) = "x\n" (1 line + \n)
    // text.slice(2) = "x\nx\n...x" (15 lines, no trailing \n)
    expect(result!.earlyText).toBe('x\n');
    expect(result!.recentText).toBe('x\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx');
    expect(result!.earlyText + result!.recentText).toBe(text);
  });

  test('empty text returns null', () => {
    expect(splitTextForOverflow('', MAX_LINES, MAX_CHARS)).toBeNull();
  });

  test('text well under both limits returns null', () => {
    const text = 'hello\nworld\nfoo\nbar';
    expect(splitTextForOverflow(text, MAX_LINES, MAX_CHARS)).toBeNull();
  });
});

describe('splitTextForOverflow — char-based split (single long line)', () => {
  test('10 lines × 300 chars (under line limit, over char limit) triggers char split', () => {
    // 10 lines × 300 chars + 9 newlines = 3000 + 9 = 3009 chars
    // lines.length = 10 < 15 → NOT line-based split
    // text.length = 3009 > 2000 → char-based split
    // charBudget = 3009 - 2000 = 1009
    // Walk from index 0 to 1009, find last \n
    //   line 0 = chars 0..299, \n at 300
    //   line 1 = chars 301..600, \n at 601
    //   line 2 = chars 602..901, \n at 902
    //   Last \n in [0, 1009) is at index 902
    //   splitAt = 902 + 1 = 903
    // earlyText = first 3 lines + their 3 \n = 903 chars
    // recentText = remaining 7 lines + 6 \n = 2106 chars
    // Total: 903 + 2106 = 3009 ✓
    const line = 'a'.repeat(300);
    const text = Array.from({ length: 10 }, () => line).join('\n');
    expect(text.split('\n').length).toBe(10);
    expect(text.length).toBe(3009);

    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    // Content comparison
    const expectedEarly = Array.from({ length: 3 }, () => line).join('\n') + '\n';
    const expectedRecent = Array.from({ length: 7 }, () => line).join('\n');
    expect(result!.earlyText).toBe(expectedEarly);
    expect(result!.recentText).toBe(expectedRecent);
    expect(result!.earlyText + result!.recentText).toBe(text);
  });

  test('single line 3000 chars with no newlines falls back to char slice', () => {
    // 1 line, no \n anywhere, so char-based split can't find a \n
    // charBudget = 3000 - 2000 = 1000
    // Fallback: splitAt = charBudget = 1000
    // earlyText = first 1000 chars, recentText = last 2000 chars
    const text = 'a'.repeat(3000);
    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    expect(result!.earlyText.length).toBe(1000);
    expect(result!.recentText.length).toBe(2000);
    expect(result!.earlyText + result!.recentText).toBe(text);
  });

  test('2000 chars (exactly at budget) returns null', () => {
    // text.length = 2000, NOT strictly greater than 2000
    // → no char-based split
    const text = 'a'.repeat(2000);
    expect(text.length).toBe(2000);
    expect(splitTextForOverflow(text, 15, 2000)).toBeNull();
  });

  test('2001 chars (one over char budget) triggers split', () => {
    const text = 'a'.repeat(2001);
    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    // No \n, so fallback to charBudget = 2001 - 2000 = 1
    expect(result!.earlyText.length).toBe(1);
    expect(result!.recentText.length).toBe(2000);
  });
});

describe('splitTextForOverflow — varying line widths (the bug-prone case)', () => {
  test('20 lines of mixed widths produces correct char offset', () => {
    // 20 lines, some short (1-5 chars) some long (40+ chars)
    // Each line has different length, so the bug-prone case is hardest
    // here — line index 5 with 50 chars/line = 250 char offset, but with
    // mixed widths the offset is unpredictable without summing correctly.
    const lines = [
      'short',                                                // 5
      'a much longer line with many many characters here',    // 47
      'medium line',                                          // 11
      'x',                                                    // 1
      'another longer line that has quite a lot of text',     // 50
      'tiny',                                                 // 4
      'p', 'q', 'r', 's', 't',                                // 5 × 1
      'one more',                                             // 8
      'two more',                                             // 8
      'last line',                                            // 9
      'final',                                                // 5
      'a', 'b', 'c', 'd', 'e',                                // 5 × 1
    ];
    const text = lines.join('\n');
    expect(text.split('\n').length).toBe(20);

    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    // skipLines = 5
    // charOffset = (5+1) + (47+1) + (11+1) + (1+1) + (50+1) = 119
    // earlyText = lines[0..5] joined by \n + trailing \n
    const expectedEarly = lines.slice(0, 5).join('\n') + '\n';
    const expectedRecent = lines.slice(5).join('\n');
    expect(result!.earlyText).toBe(expectedEarly);
    expect(result!.recentText).toBe(expectedRecent);
    expect(result!.earlyText + result!.recentText).toBe(text);
  });
});

describe('splitTextForOverflow — CJK text (char count vs byte count)', () => {
  // Chinese characters are 1 char but 3 bytes in UTF-8. A naive
  // byte-based offset would split in the middle of a multi-byte
  // sequence. Our function uses char count (text.length), so the
  // offset is always on a character boundary.

  test('CJK 20 lines of 12 chars each: char offset = 5 × 13 = 65, not line index 5', () => {
    // "你好世界你好世界你好世界" — count carefully:
    //   你(1)好(2)世(3)界(4)你(5)好(6)世(7)界(8)你(9)好(10)世(11)界(12) = 12 chars
    const line = '你好世界你好世界你好世界';  // 12 CJK chars
    expect(line.length).toBe(12);
    const text = Array.from({ length: 20 }, () => line).join('\n');
    // 20 lines × 12 chars + 19 newlines = 240 + 19 = 259 chars
    expect(text.split('\n').length).toBe(20);
    expect(text.length).toBe(259);

    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    // skipLines = 5, charOffset = 5 × (12+1) = 65 (NOT 55)
    const expectedEarly = Array.from({ length: 5 }, () => line).join('\n') + '\n';
    const expectedRecent = Array.from({ length: 15 }, () => line).join('\n');
    expect(result!.earlyText).toBe(expectedEarly);
    expect(result!.recentText).toBe(expectedRecent);
    expect(result!.earlyText + result!.recentText).toBe(text);
  });

  test('CJK short text stays intact (no split)', () => {
    const text = '你好\n世界\n测试';
    expect(splitTextForOverflow(text, 15, 2000)).toBeNull();
  });
});

describe('splitTextForOverflow — invariants', () => {
  test('earlyText + recentText === original text (no content lost)', () => {
    const cases = [
      'a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt',
      'short line\n' + 'a'.repeat(100) + '\nfinal',
      Array.from({ length: 20 }, () => 'x'.repeat(50)).join('\n'),
      '你好世界'.repeat(100),
    ];
    for (const text of cases) {
      const result = splitTextForOverflow(text, MAX_LINES, MAX_CHARS);
      if (result !== null) {
        expect(result.earlyText + result.recentText).toBe(text);
      }
    }
  });

  test('recentText never exceeds maxLines (when line-based split triggers)', () => {
    const text = Array.from({ length: 50 }, () => 'line').join('\n');
    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    // Filter out the empty string from trailing \n before counting
    const recentNonEmptyLines = result!.recentText.split('\n').filter(l => l.length > 0);
    expect(recentNonEmptyLines.length).toBe(15);
  });

  test('didSplit is true on the result struct (not just a truthy object)', () => {
    const text = Array.from({ length: 20 }, () => 'x').join('\n');
    const result = splitTextForOverflow(text, 15, 2000);
    expect(result).not.toBeNull();
    expect(result!.didSplit).toBe(true);
  });
});
