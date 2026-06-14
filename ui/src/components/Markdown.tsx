// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color)
// - blockquote: ▎ prefix + italic
// - codespan: permission color
// - strikethrough: DISABLED
// - dimColor prop: theme.inactive instead of theme.text
// - SPACING: paragraphs share the same Text block, separated by \n only.
//   Structural blocks (code/list/blockquote/heading) get their own Box with marginTop.
// - LIST ITEMS: marked v18 nests block-level wrappers (paragraph) inside list_item.tokens.
//   We must flatten to extract inline tokens (strong/em/codespan/text) for rendering.
// - TABLE: rendered from token.header + token.rows (token.text is empty in marked!)
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
                    {renderInline(ptok.tokens || [{ type: 'text', text: ptok.text }], tc, cc)}
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
            // marked v18: item.tokens contains block-level wrappers (paragraph, list_item, etc.)
            // We must extract the inner inline tokens from each wrapper, otherwise
            // **bold**, `code`, *italic* inside list items won't be parsed.
            let inlineTokens: AnyToken[] = [];
            if (item.tokens) {
              for (const t of item.tokens) {
                if (t.type === 'text' || t.type === 'paragraph') {
                  inlineTokens = inlineTokens.concat(t.tokens || [{ type: 'text', text: t.text }]);
                } else {
                  inlineTokens.push(t);
                }
              }
            } else {
              inlineTokens = [{ type: 'text', text: item.text || '' }];
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
      // marked table token: header=[...], rows=[[...], ...], align=[...]
      // token.text is EMPTY — must build from header + rows
      const header: string[] = token.header || [];
      const rows: string[][] = token.rows || [];
      if (header.length === 0 && rows.length === 0) return null;

      // Render as aligned text columns (no fancy box-drawing — keep it simple for TUI)
      const colCount = header.length || (rows[0]?.length ?? 0);
      if (colCount === 0) return null;

      // Calculate column widths
      const widths: number[] = new Array(colCount).fill(0);
      const headerTexts = header.map((h: AnyToken) => typeof h === 'string' ? h : (h.text || ''));
      headerTexts.forEach((t: string, i: number) => { if (i < colCount) widths[i] = Math.max(widths[i], t.length); });
      rows.forEach((row: AnyToken[]) => {
        row.forEach((cell: AnyToken, i: number) => {
          const text = typeof cell === 'string' ? cell : (cell?.text || '');
          if (i < colCount) widths[i] = Math.max(widths[i], text.length);
        });
      });

      const pad = (s: string, i: number) => s.padEnd(widths[i]);

      return (
        <Box flexDirection="column">
          {/* Header row */}
          <Text bold color={tc}>
            {headerTexts.map((t: string, i: number) => pad(t, i)).join('  ')}
          </Text>
          {/* Separator */}
          <Text color={theme.inactive}>
            {widths.map((w: number) => '─'.repeat(w)).join('──')}
          </Text>
          {/* Data rows */}
          {rows.map((row: AnyToken[], ri: number) => (
            <Text key={ri} color={tc}>
              {row.map((cell: AnyToken, ci: number) => {
                const text = typeof cell === 'string' ? cell : (cell?.text || '');
                return pad(text, ci);
              }).join('  ')}
            </Text>
          ))}
        </Box>
      );
    }
    case 'blockquote':
      return (
        <Box flexDirection="column">
          {(token.text || '').split('\n').filter(Boolean).map((line: string, i: number) => (
            <Box key={i} flexDirection="row">
              <Text color={theme.inactive}>{'▎ '}</Text>
              <Text color={tc} italic>{line}</Text>
            </Box>
          ))}
        </Box>
      );
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
        return <Text key={i} color={dimColorSafe(cc)}>{tok.text || tok.href}</Text>;
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

function dimColorSafe(cc: string): string {
  return cc === theme.permission ? theme.inactive : cc;
}
