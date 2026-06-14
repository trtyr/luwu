/**
 * luwu TUI theme — Claude Code dark theme exact RGB values
 * Source: claude-code-sourcemap/restored-src/docs/00-color-theme-system.md
 */

export const theme = {
  // Brand
  claude: '#D77757',            // rgb(215,119,87) — Claude orange
  claudeShimmer: '#EB9F7F',      // rgb(235,159,127)

  // Text
  text: '#FFFFFF',              // rgb(255,255,255) — pure white
  inverseText: '#000000',
  inactive: '#999999',          // rgb(153,153,153) — dimmed text
  inactiveShimmer: '#C1C1C1',   // rgb(193,193,193)
  subtle: '#505050',            // rgb(80,80,80) — very dim auxiliary

  // Semantic
  success: '#4EBA65',           // rgb(78,186,101)
  error: '#FF6B80',             // rgb(255,107,128)
  warning: '#FFC107',           // rgb(255,193,7)
  warningShimmer: '#FFDF39',    // rgb(255,223,57)
  suggestion: '#B1B9F9',        // rgb(177,185,249) — light blue-purple
  permission: '#B1B9F9',        // rgb(177,185,249) — same as suggestion
  remember: '#B1B9F9',          // rgb(177,185,249)

  // Borders
  promptBorder: '#888888',      // rgb(136,136,136)
  promptBorderShimmer: '#A6A6A6', // rgb(166,166,166)
  bashBorder: '#FD5DB1',        // rgb(253,93,177) — bright pink
  planMode: '#48968C',          // rgb(72,150,140) — soft teal
  ide: '#4782C8',               // rgb(71,130,200)

  // Backgrounds
  userMessageBackground: '#373737',     // rgb(55,55,55)
  userMessageBackgroundHover: '#464646', // rgb(70,70,70)
  messageActionsBackground: '#2C323E',   // rgb(44,50,62) — cold blue-gray
  bashMessageBackground: '#413C41',     // rgb(65,60,65)
  memoryBackground: '#374146',          // rgb(55,65,70)
  selectionBg: '#264F78',               // rgb(38,79,120)

  // Diff
  diffAdded: '#225C2B',         // rgb(34,92,43)
  diffRemoved: '#7A2936',       // rgb(122,41,54)
  diffAddedDimmed: '#47584A',   // rgb(71,88,74)
  diffRemovedDimmed: '#69484D', // rgb(105,72,77)
  diffAddedWord: '#38A660',     // rgb(56,166,96)
  diffRemovedWord: '#B3596B',   // rgb(179,89,107)

  // Misc
  fastMode: '#FF7814',          // rgb(255,120,20)
  autoAccept: '#AF87FF',        // rgb(175,135,255) — electric purple
  merged: '#AF87FF',

  // Agent colors (Tailwind 600-level)
  agentRed: '#DC2626',
  agentBlue: '#2563EB',
  agentGreen: '#16A34A',
  agentYellow: '#CA8A04',
  agentPurple: '#9333EA',
  agentOrange: '#EA580C',
  agentPink: '#DB2777',
  agentCyan: '#0891B2',

  // Rate limit
  rateLimitFill: '#B1B9F9',
  rateLimitEmpty: '#505370',
} as const;

export type ThemeKey = keyof typeof theme;

/**
 * Curried theme-aware color function.
 * Usage: paint('claude')('hello') → ANSI colored string
 */
export function paint(key: ThemeKey): (text: string) => string {
  const color = theme[key];
  return (text: string) => `\x1b[38;2;${hexToRgb(color)}m${text}\x1b[39m`;
}

/**
 * Background color function.
 */
export function bgPaint(key: ThemeKey): (text: string) => string {
  const color = theme[key];
  return (text: string) => `\x1b[48;2;${hexToRgb(color)}m${text}\x1b[49m`;
}

/**
 * Raw RGB escape sequence for a theme key.
 */
export function rgb(key: ThemeKey): string {
  return hexToRgb(theme[key]);
}

function hexToRgb(hex: string): string {
  const h = hex.replace('#', '');
  const r = parseInt(h.substring(0, 2), 16);
  const g = parseInt(h.substring(2, 4), 16);
  const b = parseInt(h.substring(4, 6), 16);
  return `${r};${g};${b}`;
}
