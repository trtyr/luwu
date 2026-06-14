// AssistantMessage — Claude Code AssistantTextMessage.tsx layout
// Source: docs/02-assistant-text-ui.md, docs/04-tool-use-ui.md, docs/15-message-list-ui.md
//
// Structure: one assistant message = interleaved text + tool blocks
// rendered in CHRONOLOGICAL ORDER (not text-first-then-tools).
//
// Each block gets its own row:
//   [⏺ dot minWidth=2] [content column]
//
// Reasoning sits ABOVE everything, in the same column width.
// First block gets addMargin, subsequent blocks are marginTop=0
// (they're in the same turn — doc 15 §5.2 addMargin logic).
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import { Markdown } from './Markdown.js';
import { ToolResult } from './ToolResult.js';
import { ReasoningBlock } from './ReasoningBlock.js';
import type { DisplayMessage } from '../core/types.js';

export function AssistantMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  const hasReasoning = !!(msg.reasoning && msg.reasoning.trim());
  const blocks = msg.blocks || [];

  // Fallback: if no blocks but has content, render as single text block
  const renderBlocks = blocks.length > 0
    ? blocks
    : (msg.content?.trim()
      ? [{ type: 'text' as const, text: msg.content }]
      : []);

  if (renderBlocks.length === 0 && !hasReasoning) return null;

  let firstBlockRendered = false;

  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0} width="100%">
      {/* Reasoning — above dot, same column width */}
      {hasReasoning && (
        <ReasoningBlock reasoning={msg.reasoning!} addMargin={false} />
      )}

      {/* Interleaved blocks — text and tool, in chronological order */}
      {renderBlocks.map((block, i) => {
        // Each block is a flat row: [⏺ dot] [content]
        // Per doc 15 §5.2: same-turn blocks use addMargin=false (marginTop=0)
        // EXCEPT the very first visible block after reasoning gets marginTop=1
        const needsGap = firstBlockRendered;
        firstBlockRendered = true;
        const marginTop = needsGap
          ? (hasReasoning && i === 0 ? 1 : 0)
          : (hasReasoning ? 1 : 0);

        if (block.type === 'text') {
          return (
            <Box
              key={i}
              alignItems="flex-start"
              flexDirection="row"
              marginTop={marginTop}
              width="100%"
            >
              <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
                <Text color={theme.text}>{LAYOUT.ASSISTANT_DOT}</Text>
              </Box>
              <Box flexDirection="column" flexShrink={1} flexGrow={1}>
                <Markdown>{block.text}</Markdown>
              </Box>
            </Box>
          );
        }

        // Tool block — same dot+content row structure
        return (
          <Box key={i} marginTop={marginTop}>
            <ToolResult tool={block.tool} addMargin={false} />
          </Box>
        );
      })}
    </Box>
  );
}
