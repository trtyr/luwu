// components/AssistantMessage.tsx — ● dot in TEXT WHITE (not orange!), content in MessageResponse
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import { MessageResponse } from './MessageResponse.js';
import { Markdown } from './Markdown.js';
import { ToolResult } from './ToolResult.js';
import type { DisplayMessage } from '../core/types.js';

export function AssistantMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return (
    <Box flexDirection="column" marginTop={addMargin ? 1 : 0}>
      <Box flexDirection="row">
        <Text color={theme.text} bold>{LAYOUT.ASSISTANT_DOT}</Text>
        <Box flexDirection="column" marginLeft={1} flexShrink={1} flexGrow={1}>
          {msg.content ? (
            <MessageResponse><Markdown>{msg.content}</Markdown></MessageResponse>
          ) : null}
        </Box>
      </Box>
      {msg.tools?.map((tool, i) => <ToolResult key={i} tool={tool} />)}
    </Box>
  );
}
