// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color)
// - blockquote: ▎ prefix + italic + PARSED inline tokens (not raw text)
// - codespan: permission color
// - strikethrough: DISABLED
// - dimColor prop: theme.inactive instead of theme.text
// - SPACING: paragraphs share the same Text block, separated by \n only.
//   Structural blocks (code/list/blockquote/heading) get their own Box with marginTop.
// - LIST ITEMS: marked v18 nests block-level wrappers (paragraph) inside list_item.tokens.
//   We must flatten to extract inline tokens (strong/em/codespan/text) for rendering.
// - TABLE: rendered from token.header + token.rows with inline parsing + CJK-aware padding
// - TASK LIST: [x] → ✓ green, [ ] → ☐ inactive
import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked } from 'marked';
import { theme } from '../theme.js';

const MD_SYNTAX_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /;

export function hasMarkdownSyntax(s: string): boolean {
  return MD_SYNTAX_RE.test(s.length > 500 ? s.slice(0, 500) : s);
}

type AnyToken = any;

let markedConfigured = false;
function ensureMarkedConfig() {
  if (markedConfigured) return;
  markedConfigured = true;
  marked.use({ tokenizer: { del() { return undefined; } } });
}

// ── CJK display width — East Asian chars take 2 terminal columns ──
const DOUBLE_WIDTH_RANGES: [number, number][] = [
  [0x1100, 0x115f], [0x2329, 0x232a], [0x2e80, 0x9fff], [0xa000, 0xa4cf],
  [0xac00, 0xd7a3], [0xf900, 0xfaff], [0xfe30, 0xfe4f], [0xff00, 0xff60],
  [0xffe0, 0xffe6], [0x1f300, 0x1faff], [0x20000, 0x2fffd], [0x30000, 0x3fffd],
];

function displayWidth(s: string): number {
  let w = 0;
  for (const ch of s) {
    const cp = ch.codePointAt(0) || 0;
    w += DOUBLE_WIDTH_RANGES.some(([lo, hi]) => cp >= lo && cp <= hi) ? 2 : 1;
  }
  return w;
}

function padEndDisplay(s: string, targetWidth: number): string {
  const pad = targetWidth - displayWidth(s);
  return pad > 0 ? s + ' '.repeat(pad) : s;
}

// ── Flatten marked v18 block-level wrappers to inline tokens ──
function flattenInline(tokens: AnyToken[]): AnyToken[] {
  let result: AnyToken[] = [];
  for (const t of tokens || []) {
    if (t.type === 'text' || t.type === 'paragraph') {
      result = result.concat(t.tokens ? flattenInline(t.tokens) : [{ type: 'text', text: t.text || '' }]);
    } else {
      result.push(t);
    }
  }
  return result;
}

// ── Task list checkbox detection ──
function checkTaskItem(text: string): { checked: boolean; text: string } | null {
  const m = text.match(/^\[([ xX])\]\s*(.*)/);
  if (!m) return null;
  return { checked: m[1].toLowerCase() === 'x', text: m[2] };
}

// ── Extract plain text from a cell token (for width calculation) ──
function cellToText(cell: AnyToken): string {
  if (typeof cell === 'string') return cell;
  const toks = cell?.tokens || [{ type: 'text', text: cell?.text || '' }];
  return toks.map((t: AnyToken) => t.text || '').join('');
}

interface MarkdownProps {
  children: string;
  dimColor?: boolean;
}

export function Markdown({ children, dimColor = false }: MarkdownProps) {
  const tokens = useMemo<AnyToken[]>(() => {
    const trimmed = children.replace(/^\n+/, '').replace(/\n+$/, '');
    if (!trimmed) return [];
    if (!hasMarkdownSyntax(trimmed)) {
      return [{ type: 'text', raw: trimmed, text: trimmed, tokens: [{ type: 'text', raw: trimmed, text: trimmed }] }];
    }
    ensureMarkedConfig();
    return marked.lexer(trimmed);
  }, [children]);

  const tc = dimColor ? theme.inactive : theme.text;
  const cc = dimColor ? theme.inactive : theme.permission;

  // Group consecutive paragraphs into a single rendered block
  const groups: { type: 'paragraph-group' | string; tokens: AnyToken[] }[] = [];
  let paraBuffer: AnyToken[] = [];

  for (const tok of tokens) {
    if (tok.type === 'space') continue;
    if (tok.type === 'paragraph') {
      paraBuffer.push(tok);
    } else {
      if (paraBuffer.length > 0) {
        groups.push({ type: 'paragraph-group', tokens: paraBuffer });
        paraBuffer = [];
      }
      groups.push({ type: tok.type, tokens: [tok] });
    }
  }
  if (paraBuffer.length > 0) {
    groups.push({ type: 'paragraph-group', tokens: paraBuffer });
  }

  return (
    <Box flexDirection="column">
      {groups.map((group, i) => {
        const needsSpace = i > 0;
        return (
          <Box key={i} marginTop={needsSpace ? 1 : 0}>
            {group.type === 'paragraph-group' ? (
              <Text color={tc}>
                {group.tokens.map((ptok: AnyToken, pi: number) => (
                  <React.Fragment key={pi}>
                    {pi > 0 ? '\n' : ''}
                    {renderInline(flattenInline([ptok]), tc, cc)}
                  </React.Fragment>
                ))}
              </Text>
            ) : (
              <TokenRenderer token={group.tokens[0]} tc={tc} cc={cc} dimColor={dimColor} />
            )}
          </Box>
        );
      })}
    </Box>
  );
}

function TokenRenderer({ token, tc, cc, dimColor }: { token: AnyToken; tc: string; cc: string; dimColor: boolean }) {
  switch (token.type) {
    case 'heading': {
      const isH1 = token.depth === 1;
      return (
        <Text bold={true} italic={isH1} underline={isH1} color={tc}>
          {renderInline(token.tokens || [{ type: 'text', text: token.text }], tc, cc)}
        </Text>
      );
    }
    case 'code':
      return <Text color={tc}>{token.text}</Text>;
    case 'list': {
      const items = token.items || [];
      return (
        <Box flexDirection="column">
          {items.map((item: AnyToken, i: number) => {
            const inlineTokens = flattenInline(item.tokens || [{ type: 'text', text: item.text || '' }]);

            // Check for task list item: [x] or [ ]
            const rawText = inlineTokens.map((t: AnyToken) => t.text || '').join('');
            const task = checkTaskItem(rawText);
            if (task) {
              if (inlineTokens[0]?.text) inlineTokens[0].text = task.text;
              return (
                <Box key={i} flexDirection="row">
                  <Text color={tc}>{'  '.repeat(token.depth || 0)}</Text>
                  <Text color={task.checked ? theme.success : theme.inactive}>
                    {task.checked ? '✓ ' : '☐ '}
                  </Text>
                  <Text color={tc}>{renderInline(inlineTokens, tc, cc)}</Text>
                </Box>
              );
            }

            return (
              <Box key={i} flexDirection="row">
                <Text color={tc}>{'  '.repeat(token.depth || 0)}{token.ordered ? `${i + 1}. ` : '- '}</Text>
                <Text color={tc}>{renderInline(inlineTokens, tc, cc)}</Text>
              </Box>
            );
          })}
        </Box>
      );
    }
    case 'table': {
      const header: AnyToken[] = token.header || [];
      const rows: AnyToken[][] = token.rows || [];
      if (header.length === 0 && rows.length === 0) return null;

      const colCount = header.length || (rows[0]?.length ?? 0);
      if (colCount === 0) return null;

      // Column widths using DISPLAY width (CJK = 2)
      const widths: number[] = new Array(colCount).fill(0);
      header.forEach((h, i) => { if (i < colCount) widths[i] = Math.max(widths[i], displayWidth(cellToText(h))); });
      rows.forEach((row: AnyToken[]) => {
        row.forEach((cell: AnyToken, i: number) => {
          if (i < colCount) widths[i] = Math.max(widths[i], displayWidth(cellToText(cell)));
        });
      });

      return (
        <Box flexDirection="column">
          {/* Header */}
          <Text bold color={tc}>
            {header.map((h: AnyToken, i: number) => padEndDisplay(cellToText(h), widths[i])).join('  ')}
          </Text>
          {/* Separator */}
          <Text color={theme.inactive}>
            {widths.map((w: number) => '─'.repeat(w)).join('──')}
          </Text>
          {/* Data rows — inline rendering + trailing pad */}
          {rows.map((row: AnyToken[], ri: number) => (
            <Box key={ri} flexDirection="row">
              {row.map((cell: AnyToken, ci: number) => (
                <React.Fragment key={ci}>
                  {ci > 0 && <Text color={tc}>{'  '}</Text>}
                  <Text color={tc}>
                    {renderInline(
                      typeof cell === 'string'
                        ? [{ type: 'text', text: cell }]
                        : (cell?.tokens || [{ type: 'text', text: cell?.text || '' }]),
                      tc, cc
                    )}
                  </Text>
                  <Text color={tc}>{' '.repeat(Math.max(0, widths[ci] - displayWidth(cellToText(cell))))}</Text>
                </React.Fragment>
              ))}
            </Box>
          ))}
        </Box>
      );
    }
    case 'blockquote': {
      // token.tokens contains block-level tokens (paragraph, etc.)
      // Split inline tokens at \n boundaries → each visual line gets ▎ prefix
      const bqBlocks: AnyToken[] = token.tokens || [];
      type BqLine = { inlineToks: AnyToken[]; subBlock?: AnyToken };
      const bqLines: BqLine[] = [];

      for (const blk of bqBlocks) {
        if (blk.type === 'paragraph' || blk.type === 'text') {
          const inlineToks = flattenInline([blk]);
          let current: AnyToken[] = [];
          for (const tok of inlineToks) {
            if (tok.type === 'text' && tok.text?.includes('\n')) {
              for (const [pi, part] of tok.text.split('\n').entries()) {
                if (pi > 0) { bqLines.push({ inlineToks: current }); current = []; }
                if (part) current.push({ type: 'text', text: part });
              }
            } else {
              current.push(tok);
            }
          }
          if (current.length > 0) bqLines.push({ inlineToks: current });
        } else {
          bqLines.push({ inlineToks: [], subBlock: blk });
        }
      }

      return (
        <Box flexDirection="column">
          {bqLines.map((line, li) => (
            <Box key={li} flexDirection="row">
              <Text color={theme.inactive}>{'▎ '}</Text>
              {line.subBlock ? (
                <TokenRenderer token={line.subBlock} tc={tc} cc={cc} dimColor={dimColor} />
              ) : (
                <Text color={tc} italic>{renderInline(line.inlineToks, tc, cc)}</Text>
              )}
            </Box>
          ))}
        </Box>
      );
    }
    case 'hr':
      return <Text color={theme.inactive}>{'─'.repeat(40)}</Text>;
    case 'html':
      return <Text color={theme.inactive}>{token.text}</Text>;
    default:
      return <Text color={tc}>{token.text || ''}</Text>;
  }
}

function renderInline(tokens: AnyToken[], tc: string, cc: string): React.ReactNode {
  if (!tokens || tokens.length === 0) return '';
  return tokens.map((tok: AnyToken, i: number) => {
    switch (tok.type) {
      case 'strong':
        return <Text key={i} bold color={tc}>{renderInline(tok.tokens || [{ type: 'text', text: tok.text }], tc, cc)}</Text>;
      case 'em':
        return <Text key={i} italic color={tc}>{renderInline(tok.tokens || [{ type: 'text', text: tok.text }], tc, cc)}</Text>;
      case 'codespan':
        return <Text key={i} color={cc}>{tok.text}</Text>;
      case 'link':
        return <Text key={i} color={cc}>{tok.text || tok.href}</Text>;
      case 'text':
        if (tok.tokens && tok.tokens.length > 1) {
          return <React.Fragment key={i}>{renderInline(tok.tokens, tc, cc)}</React.Fragment>;
        }
        return <Text key={i} color={tc}>{tok.text}</Text>;
      case 'br':
        return '\n';
      case 'escape':
        return <Text key={i}>{tok.text}</Text>;
      default:
        return <Text key={i} color={tc}>{tok.text || ''}</Text>;
    }
  });
}
