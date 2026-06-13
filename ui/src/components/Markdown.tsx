// Simplified Markdown renderer for Ink TUI
// Borrowed from Claude Code's Markdown.tsx: fast-path plain text detection + marked token mapping
import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked, type Token } from 'marked';
import { theme } from '../theme.js';

const MD_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /;

export function hasMarkdownSyntax(s: string): boolean {
  return MD_RE.test(s.length > 500 ? s.slice(0, 500) : s);
}

export function Markdown({ children }: { children: string }) {
  const tokens = useMemo<Token[]>(() => {
    if (!hasMarkdownSyntax(children)) {
      return [{ type: 'paragraph', raw: children, text: children, tokens: [{ type: 'text', raw: children, text: children }] } as Token];
    }
    return marked.lexer(children);
  }, [children]);

  return (
    <Box flexDirection="column">
      {tokens.map((tok, i) => (
        <TokenRenderer key={i} token={tok} />
      ))}
    </Box>
  );
}

function TokenRenderer({ token }: { token: Token }) {
  switch (token.type) {
    case 'heading':
      return <Text bold color={theme.claude}>{token.text}</Text>;

    case 'code':
      return (
        <Box flexDirection="column" marginY={0}>
          <Text color={theme.subtle}>```{token.lang || ''}</Text>
          <Text color={theme.success}>{token.text}</Text>
          <Text color={theme.subtle}>```</Text>
        </Box>
      );

    case 'paragraph':
      return <Text color={theme.text}>{renderInline(token)}</Text>;

    case 'list': {
      const items = (token as Token.List).items || [];
      return (
        <Box flexDirection="column">
          {items.map((item, i) => (
            <Box key={i}>
              <Text color={theme.suggestion}>{(token as Token.List).ordered ? `${i + 1}.` : '•'} </Text>
              <Text color={theme.text}>{item.text}</Text>
            </Box>
          ))}
        </Box>
      );
    }

    case 'blockquote':
      return <Text color={theme.inactive} italic>  {token.text}</Text>;

    case 'hr':
      return <Text color={theme.subtle}>{'─'.repeat(40)}</Text>;

    case 'space':
      return <Text> </Text>;

    default:
      return <Text color={theme.text}>{token.text || ''}</Text>;
  }
}

/** Render inline tokens (bold, italic, code spans, links) */
function renderInline(token: Token): string {
  if (!token.tokens) return token.text || '';
  // For Ink, we render inline as plain text — ANSI coloring per-span would
  // require splitting into <Text> fragments. Keep it simple for MVP.
  return token.text || '';
}
