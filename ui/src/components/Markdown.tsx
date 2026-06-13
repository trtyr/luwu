// Simple markdown renderer for Ink
import React from 'react';
import { Box, Text } from 'ink';

// ── types ──

type Block =
  | { type: 'code'; lang: string; content: string }
  | { type: 'heading'; level: number; content: string }
  | { type: 'list'; items: string[]; ordered: boolean }
  | { type: 'blockquote'; content: string }
  | { type: 'paragraph'; content: string }
  | { type: 'hr' };

// ── block parser ──

function parseBlocks(text: string): Block[] {
  const blocks: Block[] = [];
  const lines = text.split('\n');
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // skip empty lines
    if (line.trim() === '') { i++; continue; }

    // horizontal rule
    if (/^---+\s*$/.test(line) || /^\*\*\*+\s*$/.test(line)) {
      blocks.push({ type: 'hr' });
      i++; continue;
    }

    // code fence
    const fenceMatch = line.match(/^```\s*(\w*)/);
    if (fenceMatch) {
      const lang = fenceMatch[1] || '';
      const codeLines: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      i++; // skip closing ```
      blocks.push({ type: 'code', lang, content: codeLines.join('\n') });
      continue;
    }

    // heading
    const headingMatch = line.match(/^(#{1,6})\s+(.*)/);
    if (headingMatch) {
      blocks.push({ type: 'heading', level: headingMatch[1].length, content: headingMatch[2] });
      i++; continue;
    }

    // unordered list
    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*[-*]\s+/, ''));
        i++;
      }
      blocks.push({ type: 'list', items, ordered: false });
      continue;
    }

    // ordered list
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*\d+\.\s+/, ''));
        i++;
      }
      blocks.push({ type: 'list', items, ordered: true });
      continue;
    }

    // blockquote
    if (/^>\s?/.test(line)) {
      const quoteLines: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        quoteLines.push(lines[i].replace(/^>\s?/, ''));
        i++;
      }
      blocks.push({ type: 'blockquote', content: quoteLines.join('\n') });
      continue;
    }

    // paragraph — collect consecutive non-special lines
    const paraLines: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !lines[i].trim().startsWith('```') &&
      !/^#{1,6}\s/.test(lines[i]) &&
      !/^\s*[-*]\s+/.test(lines[i]) &&
      !/^\s*\d+\.\s+/.test(lines[i]) &&
      !/^>\s?/.test(lines[i]) &&
      !/^---+\s*$/.test(lines[i])
    ) {
      paraLines.push(lines[i]);
      i++;
    }
    if (paraLines.length > 0) {
      blocks.push({ type: 'paragraph', content: paraLines.join(' ') });
    }
  }

  return blocks;
}

// ── inline renderer ──

function renderInline(text: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  // Match **bold**, *italic*, `code`, [link](url), ~~strike~~
  const regex = /(\*\*(.+?)\*\*|\*(.+?)\*|`(.+?)`|\[(.+?)\]\((.+?)\)|~~(.+?)~~)/g;
  let lastIdx = 0;
  let m: RegExpExecArray | null;
  let key = 0;

  while ((m = regex.exec(text)) !== null) {
    if (m.index > lastIdx) {
      nodes.push(text.slice(lastIdx, m.index));
    }
    if (m[2]) {
      nodes.push(<Text key={`${keyPrefix}-${key++}`} bold>{m[2]}</Text>);
    } else if (m[3]) {
      nodes.push(<Text key={`${keyPrefix}-${key++}`} italic>{m[3]}</Text>);
    } else if (m[4]) {
      nodes.push(<Text key={`${keyPrefix}-${key++}`} color="cyan">{m[4]}</Text>);
    } else if (m[5]) {
      nodes.push(<Text key={`${keyPrefix}-${key++}`} color="blue" underline>{m[5]}</Text>);
    } else if (m[7]) {
      nodes.push(<Text key={`${keyPrefix}-${key++}`} strikethrough color="gray">{m[7]}</Text>);
    }
    lastIdx = regex.lastIndex;
  }
  if (lastIdx < text.length) {
    nodes.push(text.slice(lastIdx));
  }
  return nodes;
}

// ── block renderer ──

function CodeBlock({ lang, content }: { lang: string; content: string }) {
  return (
    <Box flexDirection="column" marginY={0}>
      {lang && <Text dimColor>  {lang}</Text>}
      <Box borderStyle="round" borderColor="gray" paddingX={1}>
        <Text>{content}</Text>
      </Box>
    </Box>
  );
}

function BlockView({ block, index }: { block: Block; index: number }) {
  switch (block.type) {
    case 'code':
      return <CodeBlock lang={block.lang} content={block.content} />;

    case 'heading': {
      const colors: Record<number, string> = { 1: 'cyan', 2: 'cyan', 3: 'white', 4: 'white' };
      return (
        <Text bold color={colors[block.level] || 'white'}>
          {block.content}
        </Text>
      );
    }

    case 'list':
      return (
        <Box flexDirection="column">
          {block.items.map((item, i) => (
            <Text key={i}>
              <Text dimColor>{block.ordered ? `${i + 1}.` : '•'}</Text>{' '}
              {renderInline(item, `l-${index}-${i}`)}
            </Text>
          ))}
        </Box>
      );

    case 'blockquote':
      return (
        <Box marginLeft={2}>
          <Text dimColor>│ </Text>
          <Text dimColor italic>{block.content}</Text>
        </Box>
      );

    case 'hr':
      return <Text dimColor>───────────────────</Text>;

    case 'paragraph':
      return <Text>{renderInline(block.content, `p-${index}`)}</Text>;

    default:
      return null;
  }
}

// ── main component ──

export function Markdown({ children }: { children: string }) {
  const blocks = React.useMemo(() => parseBlocks(children), [children]);

  return (
    <Box flexDirection="column" gap={0}>
      {blocks.map((block, i) => (
        <BlockView key={i} block={block} index={i} />
      ))}
    </Box>
  );
}
