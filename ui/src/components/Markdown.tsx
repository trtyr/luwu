// Markdown renderer for Ink TUI
// Block: heading, code, paragraph, list, blockquote, hr
// Inline: bold (**), italic (*), inline code (`), links [text](url)
import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked } from 'marked';
import { theme } from '../theme.js';

const MD_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /;

export function hasMarkdownSyntax(s: string): boolean {
  return MD_RE.test(s.length > 500 ? s.slice(0, 500) : s);
}

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
      return (
        <Text bold color={theme.claude}>
          {renderInline(token.tokens || [{ type: 'text', text: token.text }])}
        </Text>
      );
    case 'code':
      return (
        <Box flexDirection="column" marginY={0}>
          <Text color={theme.subtle}>```{token.lang || ''}</Text>
          <Text color={theme.success}>{token.text}</Text>
          <Text color={theme.subtle}>```</Text>
        </Box>
      );
    case 'paragraph':
      return <Text color={theme.text}>{renderInline(token.tokens || [{ type: 'text', text: token.text }])}</Text>;
    case 'list': {
      const items = token.items || [];
      return (
        <Box flexDirection="column">
          {items.map((item: AnyToken, i: number) => (
            <Box key={i}>
              <Text color={theme.suggestion}>{token.ordered ? `${i + 1}.` : '•'} </Text>
              <Text color={theme.text}>{renderInline(item.tokens || [{ type: 'text', text: item.text }])}</Text>
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
    case 'html':
      return <Text color={theme.inactive}>{token.text}</Text>;
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
        return <Text key={i} color={theme.success}>`{tok.text}`</Text>;
      case 'link':
        return <Text key={i} color={theme.suggestion}>{tok.text || tok.href}</Text>;
      case 'del':
        return <Text key={i} dimColor>{tok.text}</Text>;
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
