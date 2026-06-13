// components/ToolResult.tsx — tool call result with ⎿ indent
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { MessageResponse } from './MessageResponse.js';
import type { ToolCallInfo } from '../core/types.js';

export function ToolResult({ tool }: { tool: ToolCallInfo }) {
  const icon = tool.status === 'running' ? '⟳' : tool.status === 'done' ? '✓' : '✗';
  const color = tool.status === 'running' ? theme.warning : tool.status === 'done' ? theme.success : theme.error;

  return (
    <MessageResponse>
      <Box>
        <Text color={color}>{icon} </Text>
        <Text color={theme.inactiveShimmer}>{tool.name}</Text>
        {tool.result && (
          <Text color={theme.subtle}> → {tool.result.length > 80 ? tool.result.slice(0, 77) + '...' : tool.result}</Text>
        )}
      </Box>
    </MessageResponse>
  );
}
