// Simplified Markdown renderer for Ink TUI
// Borrowed from Claude Code's Markdown.tsx: fast-path plain text detection + marked token mapping
import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked } from 'marked';
import { theme } from '../theme.js';

const MD_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /;

export function hasMarkdownSyntax(s: string): boolean {
  return MD_RE.test(s.length > 500 ? s.slice(0, 500) : s);
}

// Use any for tokens since marked's Token union is complex and varies by version
type AnyToken = any;

export function Markdown({ children }: { children: string }) {
  const tokens = useMemo<AnyToken[]>(() => {
    if (!hasMarkdownSyntax(children)) {
      return [{ type: 'paragraph', raw: children, text: children, tokens: [{ type: 'text', raw: children, text: children }] }];
    }
    return marked.lexer(children);
  }, [children]);

  return (
    <Box flexDirection="column">
      {tokens.map((tok: AnyToken, i: number) => (
        <TokenRenderer key={i} token={tok} />
      ))}
    </Box>
  );
}

function TokenRenderer({ token }: { token: AnyToken }) {
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
      return <Text color={theme.text}>{token.text || ''}</Text>;

    case 'list': {
      const items = token.items || [];
      return (
        <Box flexDirection="column">
          {items.map((item: AnyToken, i: number) => (
            <Box key={i}>
              <Text color={theme.suggestion}>{token.ordered ? `${i + 1}.` : '•'} </Text>
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
