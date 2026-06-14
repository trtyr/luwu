// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color unless syntax highlighting)
// - blockquote: ▎ BLOCKQUOTE_BAR prefix + italic (NOT dim — "chalk.dim nearly invisible on dark themes")
// - codespan: permission color (rgb(177,185,249) blue-purple), NOT success green
// - strikethrough: DISABLED (model uses ~ for "approximate")
// - dimColor prop: when true, all content uses theme.inactive instead of theme.text
// - SPACING: Only structural blocks (code/list/blockquote/heading) get marginTop.
//   Consecutive paragraphs flow naturally with NO extra blank line.
//   Space tokens are filtered out entirely.
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

// Block types that need spacing before them
const STRUCTURAL_TYPES = new Set(['heading', 'code', 'list', 'blockquote', 'hr', 'table']);

export function Markdown({ children, dimColor = false }: MarkdownProps) {
  const tokens = useMemo<AnyToken[]>(() => {
    const trimmed = children.replace(/^\n+/, '').replace(/\n+$/, '');
    if (!trimmed) return [];
    if (!hasMarkdownSyntax(trimmed)) {
      return [{ type: 'paragraph', raw: trimmed, text: trimmed, tokens: [{ type: 'text', raw: trimmed, text: trimmed }] }];
    }
    ensureMarkedConfig();
    return marked.lexer(trimmed);
  }, [children]);

  const tc = dimColor ? theme.inactive : theme.text;
  const cc = dimColor ? theme.inactive : theme.permission;

  // Filter out space tokens — they cause double-spacing with marginTop
  const contentTokens = tokens.filter((t: AnyToken) => t.type !== 'space');

  return (
    <Box flexDirection="column">
      {contentTokens.map((tok: AnyToken, i: number) => {
        // Only add marginTop before structural blocks (code, list, heading, etc.)
        // NOT between consecutive paragraphs — they flow naturally
        const needsSpace = i > 0 && STRUCTURAL_TYPES.has(tok.type);
        return (
          <Box key={i} marginTop={needsSpace ? 1 : 0}>
            <TokenRenderer token={tok} tc={tc} cc={cc} dimColor={dimColor} />
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
    case 'paragraph':
      return <Text color={tc}>{renderInline(token.tokens || [{ type: 'text', text: token.text }], tc, cc)}</Text>;
    case 'list': {
      const items = token.items || [];
      return (
        <Box flexDirection="column">
          {items.map((item: AnyToken, i: number) => (
            <Box key={i} flexDirection="row">
              <Text>{'  '.repeat(token.depth || 0)}</Text>
              <Text color={tc}>{token.ordered ? `${i + 1}. ` : '- '}</Text>
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
    case 'space':
      return null;
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
