// core/highlight.ts — syntax highlighting for TUI
// cli-highlight produces ANSI escape strings; we parse them into Ink <Text> nodes.
import React from 'react';
import { Text } from 'ink';

// ── Language detection from file extension ──
const EXT_LANG: Record<string, string> = {
  rs: 'rust', ts: 'typescript', tsx: 'typescript',
  js: 'javascript', jsx: 'javascript',
  py: 'python', go: 'go', java: 'java',
  c: 'c', cpp: 'cpp', h: 'c', hpp: 'cpp',
  rb: 'ruby', php: 'php', swift: 'swift',
  kt: 'kotlin', scala: 'scala', sh: 'bash',
  bash: 'bash', zsh: 'bash', yml: 'yaml',
  yaml: 'yaml', json: 'json', toml: 'ini',
  xml: 'xml', html: 'xml', css: 'css',
  md: 'markdown', sql: 'sql',
};

export function detectLang(filePath?: string): string | undefined {
  if (!filePath) return undefined;
  const ext = filePath.split('.').pop()?.toLowerCase();
  if (!ext) return undefined;
  return EXT_LANG[ext];
}

// ── ANSI → React nodes parser ──
// cli-highlight uses these SGR codes:
//   \x1b[38;2;R;G;Bm  24-bit fg color
//   \x1b[39m          reset fg
//   \x1b[1m           bold
//   \x1b[2m           dim (faint)
//   \x1b[3m           italic
//   \x1b[22m          reset bold/dim
//   \x1b[23m          reset italic
//   \x1b[0m           reset all

interface AnsiStyle {
  color?: string;
  bold?: boolean;
  italic?: boolean;
  dim?: boolean;
}

interface AnsiSegment {
  text: string;
  style: AnsiStyle;
}

function parseAnsiToSegments(ansi: string): AnsiSegment[] {
  const segments: AnsiSegment[] = [];
  let current: AnsiStyle = {};
  let textBuffer = '';

  // Regex: matches ANSI escape sequences
  const ansiRegex = /\x1b\[([\d;]*)m/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = ansiRegex.exec(ansi)) !== null) {
    // Text before this escape code
    if (match.index > lastIndex) {
      textBuffer = ansi.slice(lastIndex, match.index);
      if (textBuffer) {
        segments.push({ text: textBuffer, style: { ...current } });
      }
    }

    // Parse the SGR params
    const params = match[1].split(';').map(Number);
    if (params[0] === 0 || match[1] === '') {
      current = {};
    } else if (params[0] === 1) {
      current.bold = true;
    } else if (params[0] === 2) {
      current.dim = true;
    } else if (params[0] === 3) {
      current.italic = true;
    } else if (params[0] === 22) {
      current.bold = false;
      current.dim = false;
    } else if (params[0] === 23) {
      current.italic = false;
    } else if (params[0] === 38 && params[1] === 2) {
      // 24-bit: 38;2;R;G;B
      current.color = `rgb(${params[2]},${params[3]},${params[4]})`;
    } else if (params[0] === 39) {
      current.color = undefined;
    }

    lastIndex = match.index + match[0].length;
  }

  // Remaining text after last escape
  if (lastIndex < ansi.length) {
    textBuffer = ansi.slice(lastIndex);
    if (textBuffer) segments.push({ text: textBuffer, style: { ...current } });
  }

  return segments;
}

/** Convert ANSI string → array of Ink <Text> elements */
export function ansiToNodes(ansi: string): React.ReactNode[] {
  const segments = parseAnsiToSegments(ansi);
  return segments.map((seg, i) => (
    <Text
      key={i}
      color={seg.style.color}
      bold={seg.style.bold}
      italic={seg.style.italic}
    >
      {seg.text}
    </Text>
  ));
}

// ── cli-highlight wrapper with lazy import + graceful fallback ──
let _highlight: ((code: string, opts?: Record<string, unknown>) => string) | null | undefined;

async function getHighlight() {
  if (_highlight !== undefined) return _highlight;
  try {
    const mod = await import('cli-highlight');
    _highlight = mod.highlight;
  } catch {
    _highlight = null;
  }
  return _highlight;
}

/**
 * Highlight code → ANSI string.
 * Falls back to plain text if cli-highlight or language is unavailable.
 */
export async function highlightCode(code: string, lang?: string): Promise<string> {
  const hl = await getHighlight();
  if (!hl || !lang) return code;
  try {
    return hl(code, { language: lang });
  } catch {
    return code;
  }
}

/**
 * Highlight code → React nodes (synchronous, uses cached cli-highlight).
 * For use in render functions where async is not possible.
 */
export function highlightToNodes(code: string, lang?: string): React.ReactNode {
  if (!lang) return code;
  try {
    // Dynamic require — cli-highlight is already installed
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const { highlight } = require('cli-highlight');
    const ansi = highlight(code, { language: lang });
    return ansiToNodes(ansi);
  } catch {
    return code;
  }
}
