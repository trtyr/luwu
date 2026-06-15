// diff-log.ts — Custom diff-based log-update for Ink
//
// Inspired by Claude Code's rendering engine (doc 31-anti-flicker-rendering.md).
//
// Standard Ink log-update: eraseLines(N) + write(ALL lines) every frame.
//   → When N > terminal rows, cursor-up clamps at top → FLICKER.
//
// Our diff log-update: find common prefix, only rewrite changed lines.
//   → Cursor-up is always small (distance to first change) → NO FLICKER.
//   → Output volume is minimal (only changed lines) → FAST.
//
// Example: spinner changes on last line of a 50-line output
//   Standard: eraseLines(50) + write(50 lines) = ~4KB per frame
//   Diff:     cursor up 1 + erase line + write 1 line = ~30 bytes per frame

/* eslint-disable @typescript-eslint/no-explicit-any */

const ESC = '\x1B';

export interface DiffLog {
  (str: string): void;
  clear(): void;
  done(): void;
}

export function createDiffLog(stream: NodeJS.WriteStream): DiffLog {
  let prevLines: string[] = [];
  let prevOutput = '';
  let cursorHidden = false;

  const hideCursor = () => {
    if (!cursorHidden) {
      stream.write(`${ESC}[?25l`);
      cursorHidden = true;
    }
  };

  const render = (str: string) => {
    hideCursor();

    const output = str + '\n';
    if (output === prevOutput) return;

    const nextLines = str.split('\n');

    // ── First render or after clear(): write everything ──
    if (prevLines.length === 0) {
      stream.write(output);
      prevLines = nextLines;
      prevOutput = output;
      return;
    }

    // ── Find common prefix (the key optimization) ──
    // Old messages never change → prefixLen is high → cursor-up is tiny
    let prefixLen = 0;
    const minLen = Math.min(prevLines.length, nextLines.length);
    while (
      prefixLen < minLen &&
      prevLines[prefixLen] === nextLines[prefixLen]
    ) {
      prefixLen++;
    }

    const linesToWrite = nextLines.length - prefixLen;
    const linesToErase = prevLines.length - prefixLen;

    if (linesToWrite === 0 && linesToErase === 0) return;

    const ops: string[] = [];

    // ── 1. Move cursor up to the first changed line ──
    // Cursor is at row prevLines.length (after last line).
    // First changed line is at row prefixLen.
    // Distance = prevLines.length - prefixLen = linesToErase.
    if (linesToErase > 0) {
      ops.push(`${ESC}[${linesToErase}A`);
    }

    // ── 2. Overwrite from prefixLen to end of nextLines ──
    for (let i = 0; i < linesToWrite; i++) {
      ops.push(`${ESC}[2K`); // Erase entire line
      ops.push(nextLines[prefixLen + i]);
      if (i < linesToWrite - 1) {
        ops.push('\n'); // Move to next row (down 1)
      }
    }

    // ── 3. Erase extra lines if content shrank ──
    if (linesToErase > linesToWrite) {
      const extra = linesToErase - linesToWrite;
      for (let i = 0; i < extra; i++) {
        ops.push('\n'); // Move down to the extra line
        ops.push(`${ESC}[2K`); // Erase it
      }
      // Move cursor back up to the last content line
      ops.push(`${ESC}[${extra}A`);
    }

    // ── 4. Final newline to position cursor after last line ──
    // Matches Ink's convention: output ends with \n so cursor is
    // at the start of a new line after all content.
    if (linesToWrite > 0) {
      ops.push('\n');
    }

    stream.write(ops.join(''));
    prevLines = nextLines;
    prevOutput = output;
  };

  render.clear = () => {
    if (prevLines.length > 0) {
      // Erase all previous dynamic lines
      stream.write(`${ESC}[${prevLines.length}A`);
      for (let i = 0; i < prevLines.length; i++) {
        stream.write(`${ESC}[2K`);
      }
      stream.write(`${ESC}[G`); // Cursor to column 1
    }
    prevLines = [];
    prevOutput = '';
  };

  render.done = () => {
    if (cursorHidden) {
      stream.write(`${ESC}[?25h`); // Show cursor
      cursorHidden = false;
    }
    prevLines = [];
    prevOutput = '';
  };

  return render;
}
