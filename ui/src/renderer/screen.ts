// screen.ts — Cell-level screen buffer with string interning pools
//
// Architecture inspired by Claude Code's screen.ts (1487 lines).
// Simplified: no hyperlink pool, no selection overlay, no damage tracking.
//
// Each cell stores 2 int32 values:
//   charIds[idx]  = character pool index (0 = space)
//   styleIds[idx] = style pool index (0 = default/no style)
//
// Diff is O(width × height) integer comparisons — microsecond-level
// for an 80×50 terminal (4000 cells × 2 arrays = 8000 int32 compares).

// ─────────────────────────────────────────────────────────────────
// Character Pool — string interning for O(1) cross-frame comparison
// ─────────────────────────────────────────────────────────────────

const charPool: string[] = [' ', ''];
// 0 = space (default fill), 1 = continuation cell (wide char 2nd half)
const charMap = new Map<string, number>([[' ', 0], ['', 1]]);
const charWidths: number[] = [1, 0]; // display width parallel to charPool

export function internChar(c: string): number {
  let id = charMap.get(c);
  if (id === undefined) {
    id = charPool.length;
    charPool.push(c);
    charMap.set(c, id);
    charWidths.push(c.length === 0 ? 0 : eastAsianWidth(c.codePointAt(0)!));
  }
  return id;
}

export function getChar(id: number): string {
  return charPool[id] ?? ' ';
}

export function getCharWidth(id: number): number {
  return charWidths[id] ?? 1;
}

/**
 * East Asian character width detection.
 * Returns 2 for CJK/Hangul/Katakana/Fullwidth/Emoji, 0 for combining marks, 1 otherwise.
 */
function eastAsianWidth(code: number): number {
  // Zero-width: combining marks, variation selectors, ZWJ
  if (code === 0x200d || (code >= 0xfe00 && code <= 0xfe0f)) return 0;
  if (code >= 0x0300 && code <= 0x036f) return 0;

  // Wide (2 cells)
  if (
    (code >= 0x1100 && code <= 0x115f) ||
    (code >= 0x2329 && code <= 0x232a) ||
    (code >= 0x2e80 && code <= 0x303e) ||
    (code >= 0x3040 && code <= 0x33bf) ||
    (code >= 0x3400 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0xa4cf) ||
    (code >= 0xa960 && code <= 0xa97f) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe30 && code <= 0xfe4f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f000 && code <= 0x1faff) ||
    (code >= 0x20000 && code <= 0x3fffd)
  )
    return 2;

  return 1;
}

// ─────────────────────────────────────────────────────────────────
// Style Pool — each unique style combination gets an integer ID
// ─────────────────────────────────────────────────────────────────

const styleKeys: string[] = [''];
const styleAnsis: string[] = ['\x1b[0m']; // index 0 = reset/default
const styleMap = new Map<string, number>([['', 0]]);

export function internStyle(key: string, ansi: string): number {
  let id = styleMap.get(key);
  if (id === undefined) {
    id = styleKeys.length;
    styleKeys.push(key);
    styleAnsis.push(ansi);
    styleMap.set(key, id);
  }
  return id;
}

export function getStyleAnsi(id: number): string {
  return styleAnsis[id] ?? '\x1b[0m';
}

// ─────────────────────────────────────────────────────────────────
// Screen — grid of cells backed by Int32Array
// ─────────────────────────────────────────────────────────────────

export class Screen {
  width: number;
  height: number;
  charIds: Int32Array;
  styleIds: Int32Array;

  constructor(width: number, height: number) {
    this.width = width;
    this.height = height;
    const size = width * height;
    this.charIds = new Int32Array(size); // all 0 (space)
    this.styleIds = new Int32Array(size); // all 0 (default)
  }

  setCell(x: number, y: number, charId: number, styleId: number): void {
    if (x < 0 || x >= this.width || y < 0 || y >= this.height) return;
    const idx = y * this.width + x;
    this.charIds[idx] = charId;
    this.styleIds[idx] = styleId;
  }
}

// ─────────────────────────────────────────────────────────────────
// findNextDiff — skip consecutive identical cells (Claude Code pattern)
// ─────────────────────────────────────────────────────────────────

/**
 * Starting at offset `start` in both arrays, find the first index where
 * they differ. Returns count of identical cells scanned.
 */
export function findNextDiff(
  a: Int32Array,
  b: Int32Array,
  start: number,
  count: number,
): number {
  for (let i = 0; i < count; i++) {
    const idx = start + i;
    if (a[idx] !== b[idx]) return i;
  }
  return count;
}
