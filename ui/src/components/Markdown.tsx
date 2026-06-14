// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color unless syntax highlighting)
// - blockquote: ▎ BLOCKQUOTE_BAR prefix + italic (NOT dim — "chalk.dim nearly invisible on dark themes")
// - codespan: permission color (rgb(177,185,249) blue-purple), NOT success green
// - strikethrough: DISABLED (model uses ~ for "approximate")
// - dimColor prop: when true, all content uses theme.inactive instead of theme.text
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
    if (!hasMarkdownSyntax(children)) {
      return [{ type: 'paragraph', raw: children, text: children, tokens: [{ type: 'text', raw: children, text: children }] }];
    }
    ensureMarkedConfig();
    return marked.lexer(children);
  }, [children]);

  // When dimColor, use theme.inactive for all text
  const tc = dimColor ? theme.inactive : theme.text;
  const cc = dimColor ? theme.inactive : theme.permission;

  return (
    <Box flexDirection="column">
      {tokens.map((tok: AnyToken, i: number) => (
        <Box key={i} marginTop={i > 0 ? 1 : 0}>
          <TokenRenderer token={tok} tc={tc} cc={cc} dimColor={dimColor} />
        </Box>
      ))}
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
      // ▎ BLOCKQUOTE_BAR (U+258E) prefix in inactive + content italic
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

// When dimColor, link color should also be inactive
function dimColorSafe(cc: string): string {
  return cc === theme.permission ? theme.inactive : cc;
}
