// ansi.ts — ANSI SGR parser
//
// Parses Ink's render output (ANSI-styled string) into Screen cells.
// Tracks SGR state (foreground/background color, bold/italic/etc.)
// and writes (charId, styleId) into Screen for each visible character.
//
// Handles:
// - 24-bit true color: ESC[38;2;R;G;Bm / ESC[48;2;R;G;Bm
// - 256-color: ESC[38;5;Nm / ESC[48;5;Nm
// - Basic 8/16 colors: ESC[30-37m, ESC[90-97m, etc.
// - Text attributes: bold(1), dim(2), italic(3), underline(4), inverse(7)
// - Resets: ESC[0m, ESC[22m, ESC[23m, ESC[24m, ESC[27m, ESC[39m, ESC[49m
// - CJK wide characters (2 cells), combining marks (0 cells)
// - Truncation/padding to terminal width

import {
  Screen,
  internChar,
  internStyle,
  getCharWidth,
} from './screen.js';

// ── Style state tracked during parsing ──

interface StyleState {
  fg: number; // packed RGB: (r<<16)|(g<<8)|b, 0 = default
  bg: number;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
  inverse: boolean;
}

function defaultState(): StyleState {
  return {
    fg: 0,
    bg: 0,
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    inverse: false,
  };
}

function packRGB(r: number, g: number, b: number): number {
  return ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff);
}

// Basic ANSI colors (0-15) as packed RGB
const BASIC_COLORS: number[] = [
  0x000000, 0xcc0000, 0x00cd00, 0xcdcd00, // 0-3: black, red, green, yellow
  0x0000ee, 0xcd00cd, 0x00cdcd, 0xe8e8e8, // 4-7: blue, magenta, cyan, white
  0x4d4d4d, 0xff0000, 0x00ff00, 0xffff00, // 8-11: bright variants
  0x4682b4, 0xff00ff, 0x00ffff, 0xffffff, // 12-15
];

// Convert xterm-256 color index to packed RGB
function color256ToRGB(n: number): number {
  if (n < 16) return BASIC_COLORS[n] ?? 0;
  if (n >= 232) {
    const v = 8 + (n - 232) * 10;
    return packRGB(v, v, v);
  }
  n -= 16;
  const conv = (v: number) => (v === 0 ? 0 : 55 + v * 40);
  return packRGB(
    conv(Math.floor(n / 36) % 6),
    conv(Math.floor(n / 6) % 6),
    conv(n % 6),
  );
}

// ── SGR parameter handler ──

function applySGR(state: StyleState, params: number[]): void {
  if (params.length === 0) {
    Object.assign(state, defaultState());
    return;
  }

  let i = 0;
  while (i < params.length) {
    const c = params[i]!;

    switch (c) {
      case 0: Object.assign(state, defaultState()); break;
      case 1: state.bold = true; break;
      case 2: state.dim = true; break;
      case 3: state.italic = true; break;
      case 4: state.underline = true; break;
      case 7: state.inverse = true; break;
      case 22: state.bold = false; state.dim = false; break;
      case 23: state.italic = false; break;
      case 24: state.underline = false; break;
      case 27: state.inverse = false; break;
      case 38:
        if (params[i + 1] === 2) {
          state.fg = packRGB(params[i + 2] ?? 0, params[i + 3] ?? 0, params[i + 4] ?? 0);
          i += 4;
        } else if (params[i + 1] === 5) {
          state.fg = color256ToRGB(params[i + 2] ?? 0);
          i += 2;
        }
        break;
      case 39: state.fg = 0; break;
      case 48:
        if (params[i + 1] === 2) {
          state.bg = packRGB(params[i + 2] ?? 0, params[i + 3] ?? 0, params[i + 4] ?? 0);
          i += 4;
        } else if (params[i + 1] === 5) {
          state.bg = color256ToRGB(params[i + 2] ?? 0);
          i += 2;
        }
        break;
      case 49: state.bg = 0; break;
      default:
        if (c >= 30 && c <= 37) state.fg = BASIC_COLORS[c - 30]!;
        else if (c >= 40 && c <= 47) state.bg = BASIC_COLORS[c - 40]!;
        else if (c >= 90 && c <= 97) state.fg = BASIC_COLORS[c - 90 + 8]!;
        else if (c >= 100 && c <= 107) state.bg = BASIC_COLORS[c - 100 + 8]!;
        break;
    }
    i++;
  }
}

// ── Style → key + ANSI ──

function styleKeyOf(s: StyleState): string {
  return `${s.fg},${s.bg},${s.bold ? 1 : 0}${s.dim ? 1 : 0}${s.italic ? 1 : 0}${s.underline ? 1 : 0}${s.inverse ? 1 : 0}`;
}

function styleAnsiOf(s: StyleState): string {
  const parts: string[] = ['\x1b[0m']; // always reset first
  if (s.fg > 0) {
    parts.push(`\x1b[38;2;${(s.fg >> 16) & 0xff};${(s.fg >> 8) & 0xff};${s.fg & 0xff}m`);
  }
  if (s.bg > 0) {
    parts.push(`\x1b[48;2;${(s.bg >> 16) & 0xff};${(s.bg >> 8) & 0xff};${s.bg & 0xff}m`);
  }
  if (s.bold) parts.push('\x1b[1m');
  if (s.dim) parts.push('\x1b[2m');
  if (s.italic) parts.push('\x1b[3m');
  if (s.underline) parts.push('\x1b[4m');
  if (s.inverse) parts.push('\x1b[7m');
  return parts.join('');
}

// ── Line parser: ANSI string → Screen cells ──

/**
 * Parse one line of ANSI-styled text into Screen cells at row y.
 * Cells are padded with spaces up to `width` columns.
 */
export function parseLine(
  line: string,
  screen: Screen,
  y: number,
  width: number,
): void {
  const state = defaultState();
  let x = 0;
  let i = 0;

  while (i < line.length && x < width) {
    const ch = line[i]!;

    // ── ANSI escape sequence ──
    if (ch === '\x1b') {
      if (line[i + 1] === '[') {
        // CSI: ESC [ params m
        i += 2;
        let paramStr = '';
        while (i < line.length && line[i] !== 'm') {
          paramStr += line[i];
          i++;
        }
        i++; // skip 'm'
        const params =
          paramStr.length > 0
            ? paramStr.split(';').map((n) => parseInt(n, 10) || 0)
            : [];
        applySGR(state, params);
        continue;
      }
      // Non-CSI escape — skip the escape char
      i++;
      continue;
    }

    // ── Control characters (except ESC handled above) ──
    const code = line.codePointAt(i)!;
    if (code < 32) {
      i++;
      continue;
    }

    // ── Visible character ──
    const char = code > 0xffff ? String.fromCodePoint(code) : ch;
    const charId = internChar(char);
    const w = getCharWidth(charId);

    if (w === 0) {
      // Zero-width combining mark — skip
      i += char.length;
      continue;
    }

    if (x + w > width) {
      // Character doesn't fit — fill rest with spaces and stop
      break;
    }

    const sKey = styleKeyOf(state);
    const sAnsi = styleAnsiOf(state);
    const styleId = internStyle(sKey, sAnsi);

    screen.setCell(x, y, charId, styleId);
    if (w === 2 && x + 1 < width) {
      screen.setCell(x + 1, y, 1, styleId); // continuation cell
    }

    x += w;
    i += char.length;
  }

  // Remaining cells in this row are already 0 (space) from Screen init
}

// ── Full output parser: multi-line string → Screen ──

export function parseOutput(str: string, width: number): Screen {
  const lines = str.split('\n');
  const height = lines.length;
  const screen = new Screen(width, height);

  for (let y = 0; y < height; y++) {
    parseLine(lines[y]!, screen, y, width);
  }

  return screen;
}
