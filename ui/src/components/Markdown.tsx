// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color unless syntax highlighting)
// - blockquote: ▎ BLOCKQUOTE_BAR prefix + italic (NOT dim — "chalk.dim nearly invisible on dark themes")
// - codespan: permission color (rgb(177,185,249) blue-purple), NOT success green
// - strikethrough: DISABLED (model uses ~ for "approximate")
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

export function Markdown({ children }: { children: string }) {
  const tokens = useMemo<AnyToken[]>(() => {
    if (!hasMarkdownSyntax(children)) {
      return [{ type: 'paragraph', raw: children, text: children, tokens: [{ type: 'text', raw: children, text: children }] }];
    }
    ensureMarkedConfig();
    return marked.lexer(children);
  }, [children]);

  return (
    <Box flexDirection="column">
      {tokens.map((tok: AnyToken, i: number) => (
        <Box key={i} marginTop={i > 0 ? 1 : 0}>
          <TokenRenderer token={tok} />
        </Box>
      ))}
    </Box>
  );
}

function TokenRenderer({ token }: { token: AnyToken }) {
  switch (token.type) {
    case 'heading': {
      // H1 = bold + italic + underline; H2/H3+ = bold only
      const isH1 = token.depth === 1;
      return (
        <Text bold={true} italic={isH1} underline={isH1} color={theme.text}>
          {renderInline(token.tokens || [{ type: 'text', text: token.text }])}
        </Text>
      );
    }
    case 'code':
      // Claude Code: code without highlighting = plain text, no fence
      return <Text color={theme.text}>{token.text}</Text>;
    case 'paragraph':
      return <Text color={theme.text}>{renderInline(token.tokens || [{ type: 'text', text: token.text }])}</Text>;
    case 'list': {
      const items = token.items || [];
      return (
        <Box flexDirection="column">
          {items.map((item: AnyToken, i: number) => (
            <Box key={i} flexDirection="row">
              <Text>{'  '.repeat(token.depth || 0)}</Text>
              <Text color={theme.text}>{token.ordered ? `${i + 1}. ` : '- '}</Text>
              <Text color={theme.text}>{renderInline(item.tokens || [{ type: 'text', text: item.text }])}</Text>
            </Box>
          ))}
        </Box>
      );
    }
    case 'blockquote':
      // Claude Code: ▎ prefix (dim) + content italic (NOT dim — dim invisible on dark themes)
      return (
        <Box flexDirection="column">
          {(token.text || '').split('\n').filter(Boolean).map((line: string, i: number) => (
            <Box key={i} flexDirection="row">
              <Text color={theme.inactive}>{'▎ '}</Text>
              <Text color={theme.text} italic>{line}</Text>
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
      return <Text color={theme.text}>{token.text || ''}</Text>;
    default:
      return <Text color={theme.text}>{token.text || ''}</Text>;
  }
}

function renderInline(tokens: AnyToken[]): React.ReactNode {
  if (!tokens || tokens.length === 0) return '';
  return tokens.map((tok: AnyToken, i: number) => {
    switch (tok.type) {
      case 'strong':
        return <Text key={i} bold color={theme.text}>{renderInline(tok.tokens || [{ type: 'text', text: tok.text }])}</Text>;
      case 'em':
        return <Text key={i} italic color={theme.text}>{renderInline(tok.tokens || [{ type: 'text', text: tok.text }])}</Text>;
      case 'codespan':
        // Claude Code: codespan = permission color (blue-purple), NOT success green
        return <Text key={i} color={theme.permission}>{tok.text}</Text>;
      case 'link':
        return <Text key={i} color={theme.suggestion}>{tok.text || tok.href}</Text>;
      case 'text':
        if (tok.tokens && tok.tokens.length > 1) {
          return <React.Fragment key={i}>{renderInline(tok.tokens)}</React.Fragment>;
        }
        return <Text key={i} color={theme.text}>{tok.text}</Text>;
      case 'br':
        return '\n';
      case 'escape':
        return <Text key={i}>{tok.text}</Text>;
      default:
        return <Text key={i} color={theme.text}>{tok.text || ''}</Text>;
    }
  });
}
