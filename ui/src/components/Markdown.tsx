// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color)
// - blockquote: ▎ prefix + italic
// - codespan: permission color
// - strikethrough: DISABLED
// - dimColor prop: theme.inactive instead of theme.text
// - SPACING: paragraphs share the same Text block, separated by \n only.
//   Structural blocks (code/list/blockquote/heading) get their own Box with marginTop.
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

// Structural blocks that need their own Box with marginTop
const STRUCTURAL_TYPES = new Set(['heading', 'code', 'list', 'blockquote', 'hr', 'table']);

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
  // so they flow as continuous text separated by blank lines (from \n\n in content)
  const groups: { type: 'paragraph-group' | string; tokens: AnyToken[] }[] = [];
  let paraBuffer: AnyToken[] = [];

  for (const tok of tokens) {
    if (tok.type === 'space') continue; // skip spaces entirely
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
              // Render all paragraphs as a single continuous text block
              // Each paragraph separated by \n (not \n\n — the marginTop handles spacing)
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
          {items.map((item: AnyToken, i: number) => (
            <Box key={i} flexDirection="row">
              <Text color={tc}>{'  '.repeat(token.depth || 0)}{token.ordered ? `${i + 1}. ` : '- '}</Text>
              <Text color={tc}>{renderInline(item.tokens || [{ type: 'text', text: item.text }], tc, cc)}</Text>
            </Box>
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
    case 'table':
      return <Text color={tc}>{token.text || ''}</Text>;
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
