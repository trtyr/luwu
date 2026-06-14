// Markdown renderer for Ink TUI
// Based on Claude Code's formatToken (markdown.ts):
// - code: plain text (no fence, no color unless syntax highlighting)
// - blockquote: ▎ BLOCKQUOTE_BAR prefix + italic
// - strikethrough: DISABLED (model uses ~ for "approximate")
import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked } from 'marked';
import { theme } from '../theme.js';

// Same regex as Claude Code — matches any MD marker
const MD_SYNTAX_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /;

export function hasMarkdownSyntax(s: string): boolean {
  return MD_SYNTAX_RE.test(s.length > 500 ? s.slice(0, 500) : s);
}

type AnyToken = any;

// Disable strikethrough parsing (model uses ~ for approximate)
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
        // marginTop on non-first block tokens creates paragraph spacing
        // Claude Code achieves this via EOL appends in formatToken
        <Box key={i} marginTop={i > 0 ? 1 : 0}>
          <TokenRenderer token={tok} />
        </Box>
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
      // Claude Code: code without highlighting = plain text, no fence
      return <Text color={theme.text}>{token.text}</Text>;
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
      // Claude Code uses BLOCKQUOTE_BAR (▎) prefix, italic text
      return <Text color={theme.inactive} italic>▎ {token.text}</Text>;
    case 'hr':
      return <Text color={theme.subtle}>{'─'.repeat(40)}</Text>;
    case 'space':
      // space token = blank line between paragraphs
      return null;
    case 'html':
      return <Text color={theme.inactive}>{token.text}</Text>;
    case 'table':
      // Simplified table rendering
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
        return <Text key={i} color={theme.suggestion}>{tok.text}</Text>;
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
