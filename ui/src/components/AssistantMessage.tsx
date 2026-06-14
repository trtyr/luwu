// AssistantMessage — ● dot + content INLINE (not wrapped in MessageResponse!)
// Claude Code layout: [●] [content flows directly to the right of dot]
// MessageResponse (⎿ indent) is ONLY for tool results, NOT main text
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import { Markdown } from './Markdown.js';
import { ToolResult } from './ToolResult.js';
import { ReasoningBlock } from './ReasoningBlock.js';
import type { DisplayMessage } from '../core/types.js';

export function AssistantMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0}>
      {msg.reasoning && (
        <ReasoningBlock reasoning={msg.reasoning} />
      )}
      {msg.content && (
        <Box flexDirection="row" marginTop={msg.reasoning ? 0 : 0}>
          {/* ● dot: minWidth=2, text white */}
          <Text color={theme.text}>{LAYOUT.ASSISTANT_DOT} </Text>
          {/* Content flows directly right of dot, NO MessageResponse wrapper */}
          <Box flexDirection="column" flexShrink={1} flexGrow={1}>
            <Markdown>{msg.content}</Markdown>
          </Box>
        </Box>
      )}
      {/* Tool results DO use MessageResponse (⎿ indent) */}
      {msg.tools?.map((tool, i) => <ToolResult key={i} tool={tool} />)}
    </Box>
  );
}
