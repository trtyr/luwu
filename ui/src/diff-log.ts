// diff-log.ts — Cell-level diff renderer (replaces Ink's logUpdate)
//
// Inspired by Claude Code's rendering engine (doc 31-anti-flicker-rendering.md).
//
// Pipeline:
//   Ink output string
//       ↓  ansi.ts: parseOutput()
//   Screen buffer (Int32Array: charIds + styleIds)
//       ↓  diff against previous frame
//   Changed cells list
//       ↓  VirtualScreen cursor tracking
//   ANSI diff output (cursor moves + char writes)
//       ↓  stdout
//
// Standard Ink: eraseLines(N) + write(ALL lines) every frame → FLICKER when N > rows
// Our diff:     only write CHANGED CELLS → cursor-up always tiny → NO FLICKER
//
// Example: spinner changes 1 char on last line of 50-line output
//   Standard: eraseLines(50) + write(50 lines) = ~4KB → cursor clamps at top → FLICKER
//   Our diff: cursor up 1 + erase line + write 1 line = ~30 bytes → NO FLICKER

import { Screen, findNextDiff, getChar, getCharWidth, getStyleAnsi } from './renderer/screen.js';
import { parseOutput } from './renderer/ansi.js';

const ESC = '\x1b';

export interface DiffLog {
  (str: string): void;
  clear(): void;
  done(): void;
}

export function createDiffLog(stream: NodeJS.WriteStream): DiffLog {
  let prevScreen: Screen | null = null;
  let prevStr = '';
  let prevWidth = 0;
  let cursorHidden = false;

  const hideCursor = () => {
    if (!cursorHidden) {
      stream.write(`${ESC}[?25l`);
      cursorHidden = true;
    }
  };

  // ── Virtual cursor tracking ──
  // After each render, cursor is at (col=0, row=contentLines).
  // This matches Ink's convention: output ends with implicit row advancement.
  let curX = 0;
  let curY = 0;
  let curStyleId = -1; // unknown

  const render = (str: string) => {
    hideCursor();

    if (str === prevStr) return;

    const width = stream.columns || 80;
    const nextScreen = parseOutput(str, width);
    const nextHeight = nextScreen.height;

    // ── First render: write everything ──
    if (prevScreen === null || prevWidth !== width) {
      stream.write(str + '\n');
      prevScreen = nextScreen;
      prevStr = str;
      prevWidth = width;
      curX = 0;
      curY = nextHeight;
      curStyleId = -1;
      return;
    }

    const prevHeight = prevScreen.height;
    const maxHeight = Math.max(prevHeight, nextHeight);
    const out: string[] = [];

    // ── Diff: scan all cells, write only changed ones ──
    // Most cells are unchanged → findNextDiff skips them in bulk.
    for (let y = 0; y < maxHeight; y++) {
      const rowStart = y * width;
      const inPrev = y < prevHeight;
      const inNext = y < nextHeight;

      let x = 0;
      while (x < width) {
        const idx = rowStart + x;
        const pChar = inPrev ? prevScreen!.charIds[idx] : 0;
        const pStyle = inPrev ? prevScreen!.styleIds[idx] : 0;
        const nChar = inNext ? nextScreen.charIds[idx] : 0;
        const nStyle = inNext ? nextScreen.styleIds[idx] : 0;

        // Skip identical cells using findNextDiff for both arrays
        if (pChar === nChar && pStyle === nStyle) {
          // Fast-forward past identical cells
          const skip = findNextDiff(
            prevScreen!.charIds,
            nextScreen.charIds,
            rowStart + x,
            Math.min(width - x, (inPrev ? prevHeight : y + 1) - y),
          );
          // Also check styles match
          const skipStyle = findNextDiff(
            prevScreen!.styleIds,
            nextScreen.styleIds,
            rowStart + x,
            Math.min(width - x, (inPrev ? prevHeight : y + 1) - y),
          );
          const realSkip = Math.min(skip, skipStyle);
          x += realSkip > 0 ? realSkip : 1;
          continue;
        }

        // ── This cell changed — move cursor here ──
        const dy = curY - y;
        if (dy > 0) out.push(`${ESC}[${dy}A`);
        else if (dy < 0) out.push(`${ESC}[${-dy}B`);

        const dx = x - curX;
        if (dx > 0) out.push(`${ESC}[${dx}C`);
        else if (dx < 0) out.push(`${ESC}[${-dx}D`);

        curY = y;
        curX = x;

        // ── Set style if changed ──
        if (nStyle !== curStyleId) {
          out.push(getStyleAnsi(nStyle));
          curStyleId = nStyle;
        }

        // ── Write character ──
        const char = getChar(nChar);
        out.push(char);
        const w = getCharWidth(nChar);
        curX += w;

        // Handle terminal auto-wrap at last column
        if (curX >= width) {
          curX = 0;
          curY++;
        }

        x++;
      }
    }

    // ── Position cursor at (0, nextHeight) for next frame ──
    out.push('\r');
    const finalDy = nextHeight - curY;
    if (finalDy > 0) out.push(`${ESC}[${finalDy}B`);
    else if (finalDy < 0) out.push(`${ESC}[${-finalDy}A`);
    curX = 0;
    curY = nextHeight;

    if (out.length > 0) {
      stream.write(out.join(''));
    }

    // ── Swap buffers (double buffering) ──
    prevScreen = nextScreen;
    prevStr = str;
  };

  render.clear = () => {
    if (prevScreen !== null) {
      // Erase all previous lines
      const h = prevScreen.height;
      stream.write(`${ESC}[${h}A`);
      for (let i = 0; i < h; i++) {
        stream.write(`${ESC}[2K`);
        if (i < h - 1) stream.write(`${ESC}[1B`);
      }
      stream.write('\r');
    }
    prevScreen = null;
    prevStr = '';
    curX = 0;
    curY = 0;
    curStyleId = -1;
  };

  render.done = () => {
    if (cursorHidden) {
      stream.write(`${ESC}[?25h`);
      cursorHidden = false;
    }
    prevScreen = null;
    prevStr = '';
    curX = 0;
    curY = 0;
    curStyleId = -1;
  };

  return render;
}
