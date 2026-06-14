/**
 * luwu TUI theme — 一比一复刻 Claude Code dark theme
 * Source: claude-code-sourcemap/restored-src/src/utils/theme.ts (darkTheme)
 */

export const theme = {
  // Brand
  claude: '#D77757',            // rgb(215,119,87) — Claude orange
  claudeShimmer: '#F09575',      // rgb(240,149,117)

  // Text
  text: '#FFFFFF',              // rgb(255,255,255) — pure white
  inverseText: '#000000',
  inactive: '#A0A0A0',          // rgb(160,160,160) — dimmed
  inactiveShimmer: '#AAAAAA',
  subtle: '#828282',            // rgb(130,130,130) — very dim

  // Semantic
  success: '#4EBA65',           // rgb(78,186,101)
  error: '#FF6B80',             // rgb(255,107,128)
  warning: '#FFC107',           // rgb(255,193,7)
  suggestion: '#5769F7',        // rgb(87,105,247) — medium blue
  permission: '#5769F7',

  // Borders
  promptBorder: '#666666',      // rgb(102,102,102)
  promptBorderShimmer: '#777777',
  bashBorder: '#FF6B80',

  // Backgrounds
  userMessageBackground: '#373737',     // rgb(55,55,55)
  userMessageBackgroundHover: '#464646',
  bashMessageBackground: '#413C41',
  memoryBackground: '#374146',

  // Diff
  diffAdded: '#225C2B',
  diffRemoved: '#7A2936',
  diffAddedWord: '#38A660',
  diffRemovedWord: '#B3596B',

  // Misc
  planMode: '#008B8B',
  ide: '#6A9BCC',
  fastMode: '#FF7814',

  // Agent colors
  agentRed: '#DC2626',
  agentBlue: '#2563EB',
  agentGreen: '#16A34A',
  agentYellow: '#CA8A04',
  agentPurple: '#9333EA',
  agentOrange: '#EA580C',
} as const;

export type ThemeKey = keyof typeof theme;

/**
 * Curried theme-aware color function.
 * Usage: paint('claude')('hello') → chalk.hex('#D77757')('hello')
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
