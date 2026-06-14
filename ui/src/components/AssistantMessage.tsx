// AssistantMessage — Claude Code AssistantTextMessage.tsx L228-266 layout:
// [dot minWidth=2] [Markdown content column] inside flexDirection=row, width=100%
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

  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0} width="100%">
      {hasReasoning && <ReasoningBlock reasoning={msg.reasoning!} addMargin={false} />}
      {msg.content && (
        <Box alignItems="flex-start" flexDirection="row" width="100%" marginTop={hasReasoning ? 1 : 0}>
          <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
            <Text color={theme.text}>{LAYOUT.ASSISTANT_DOT}</Text>
          </Box>
          <Box flexDirection="column" flexShrink={1} flexGrow={1}>
            <Markdown>{msg.content}</Markdown>
          </Box>
        </Box>
      )}
      {msg.tools?.map((tool, i) => <ToolResult key={i} tool={tool} />)}
    </Box>
  );
}
