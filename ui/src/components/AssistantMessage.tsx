// AssistantMessage — Claude Code AssistantTextMessage.tsx layout
// Source: docs/02-assistant-text-ui.md, docs/04-tool-use-ui.md
// Structure: one visual block per message
//   [⏺ dot minWidth=2] [content column: Markdown + tools]
// Reasoning sits ABOVE the dot, in the same column
// Tools sit BELOW the dot text, each with their own ⏺ dot
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
  const hasContent = !!(msg.content && msg.content.trim());
  const hasTools = !!(msg.tools && msg.tools.length > 0);

  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0} width="100%">
      {/* Reasoning — above dot, same column width */}
      {hasReasoning && <ReasoningBlock reasoning={msg.reasoning!} addMargin={false} />}

      {/* Assistant text — dot + content */}
      {hasContent && (
        <Box
          alignItems="flex-start"
          flexDirection="row"
          marginTop={hasReasoning ? 1 : 0}
          width="100%"
        >
          <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
            <Text color={theme.text}>{LAYOUT.ASSISTANT_DOT}</Text>
          </Box>
          <Box flexDirection="column" flexShrink={1} flexGrow={1}>
            <Markdown>{msg.content}</Markdown>
          </Box>
        </Box>
      )}

      {/* Tools — each has own ⏺ dot, compact spacing (marginTop=1 only if there's content above) */}
      {hasTools && msg.tools!.map((tool, i) => (
        <ToolResult key={i} tool={tool} addMargin={hasContent || i > 0} />
      ))}
    </Box>
  );
}
