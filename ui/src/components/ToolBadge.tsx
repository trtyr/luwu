// Tool call badge display
import React from 'react';
import { Box, Text } from 'ink';
import type { ToolCallInfo } from '../types';

export function ToolBadge({ tool }: { tool: ToolCallInfo }) {
  const icon = tool.status === 'running' ? '⠋' : tool.status === 'done' ? '✓' : '✗';
  const color = tool.status === 'running' ? 'yellow' : tool.status === 'done' ? 'green' : 'red';

  // truncate long args/results
  const argsShort = tool.args.length > 80 ? tool.args.slice(0, 80) + '…' : tool.args;
  const resultShort = tool.result
    ? tool.result.length > 200
      ? tool.result.slice(0, 200) + '…'
      : tool.result
    : null;

  return (
    <Box flexDirection="column" marginY={0}>
      <Box>
        <Text color={color}>{icon}</Text>
        <Text bold color={color}> {tool.name}</Text>
        <Text dimColor> {argsShort}</Text>
      </Box>
      {resultShort && (
        <Box marginLeft={2}>
          <Text dimColor>↳ {resultShort}</Text>
        </Box>
      )}
    </Box>
  );
}
