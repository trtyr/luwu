// components/DiffViewer.tsx — Claude Code StructuredDiffFallback simplified
// Source: docs/11-diff-viewer-ui.md §3
// Line-level diff: green bg for added, red bg for removed, dim for unchanged
// Syntax highlighting via cli-highlight when file language is detected
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { diffLines } from 'diff';
import { highlightToNodes, detectLang } from '../core/highlight.js';

interface DiffViewerProps {
  oldText: string;
  newText: string;
  filePath?: string;
}

interface DiffLineObj {
  code: string;
  type: 'add' | 'remove' | 'nochange';
}

function buildDiffLines(oldText: string, newText: string): DiffLineObj[] {
  const parts = diffLines(oldText, newText);
  const result: DiffLineObj[] = [];

  for (const part of parts) {
    const lines = part.value.split('\n');
    if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();

    for (const line of lines) {
      if (part.added) result.push({ code: line, type: 'add' });
      else if (part.removed) result.push({ code: line, type: 'remove' });
      else result.push({ code: line, type: 'nochange' });
    }
  }
  return result;
}

// Claude Code: only show 3 context lines around changes (CONTEXT_LINES = 3)
const CONTEXT_LINES = 3;

function trimContext(lines: DiffLineObj[]): DiffLineObj[] {
  const changedIdx = new Set<number>();
  lines.forEach((l, i) => { if (l.type !== 'nochange') changedIdx.add(i); });
  if (changedIdx.size === 0) return lines.slice(0, 20);

  const visible = new Set<number>();
  for (const idx of changedIdx) {
    for (let d = -CONTEXT_LINES; d <= CONTEXT_LINES; d++) {
      const i = idx + d;
      if (i >= 0 && i < lines.length) visible.add(i);
    }
  }

  const result: DiffLineObj[] = [];
  let prevVisible = false;
  for (let i = 0; i < lines.length; i++) {
    if (visible.has(i)) {
      result.push(lines[i]);
      prevVisible = true;
    } else if (prevVisible) {
      result.push({ code: '...', type: 'nochange' });
      prevVisible = false;
    }
  }
  return result;
}

export function DiffViewer({ oldText, newText, filePath }: DiffViewerProps) {
  const lang = detectLang(filePath);
  const lines = buildDiffLines(oldText, newText);
  const trimmed = trimContext(lines);

  let oldLn = 1, newLn = 1;

  return (
    <Box flexDirection="column" paddingLeft={2}>
      {filePath && (
        <Text color={theme.inactive} italic>{filePath}</Text>
      )}
      {trimmed.map((line, i) => {
        if (line.code === '...' && line.type === 'nochange') {
          return <Text key={i} color={theme.subtle}>  ...</Text>;
        }

        const bg = line.type === 'add' ? theme.diffAdded
          : line.type === 'remove' ? theme.diffRemoved
          : null;

        const sigil = line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' ';

        let lineNum: string;
        if (line.type === 'add') lineNum = (newLn++).toString();
        else if (line.type === 'remove') lineNum = (oldLn++).toString();
        else { lineNum = `${oldLn++}`; newLn++; }

        // Syntax-highlighted code nodes (foreground colors from cli-highlight)
        // Background color from diff (green/red) applied via parent Text
        const highlighted = lang
          ? highlightToNodes(line.code, lang)
          : line.code;

        return (
          <Box key={i} flexDirection="row">
            <Text backgroundColor={bg || undefined}>
              <Text color={line.type === 'nochange' ? theme.inactive : theme.subtle}>
                {sigil}{' '}
              </Text>
              <Text
                backgroundColor={bg || undefined}
                color={line.type === 'nochange' ? theme.inactive : undefined}
              >
                {highlighted}
              </Text>
            </Text>
          </Box>
        );
      })}
    </Box>
  );
}
