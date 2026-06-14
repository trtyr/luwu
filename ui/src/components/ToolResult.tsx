// ToolResult — Claude Code-style: ⎿ ToolName(params) → result
// Structure: MessageResponse wraps the whole thing, inner rows are NOT double-indented
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { MessageResponse } from './MessageResponse.js';
import type { ToolCallInfo } from '../core/types.js';

const STATUS_ICONS = { running: '⟳', done: '✓', error: '✗' } as const;

export function ToolResult({ tool }: { tool: ToolCallInfo }) {
  const icon = STATUS_ICONS[tool.status];
  const iconColor =
    tool.status === 'running' ? theme.warning :
    tool.status === 'done' ? theme.success :
    theme.error;

  // Parse args — extract readable parameter
  let paramDisplay = '';
  try {
    const parsed = JSON.parse(tool.args);
    if (parsed.path) paramDisplay = parsed.path;
    else if (parsed.file_path) paramDisplay = parsed.file_path;
    else if (parsed.command) paramDisplay = parsed.command;
    else if (parsed.pattern) paramDisplay = parsed.pattern;
    else if (typeof parsed === 'string') paramDisplay = parsed;
    else paramDisplay = tool.args;
  } catch {
    paramDisplay = tool.args;
  }

  if (paramDisplay.length > 60) {
    paramDisplay = paramDisplay.slice(0, 57) + '...';
  }

  const resultDisplay = tool.result
    ? tool.result.length > 80
      ? tool.result.slice(0, 77) + '...'
      : tool.result
    : '';

  return (
    <MessageResponse>
      <Box flexDirection="row" flexWrap="nowrap">
        <Text color={iconColor}>{icon} </Text>
        <Text bold color={theme.text} wrap="truncate-end">{tool.name}</Text>
        {paramDisplay && (
          <Text color={theme.inactive}>({paramDisplay})</Text>
        )}
      </Box>
      {tool.status !== 'running' && resultDisplay && (
        <Text color={tool.status === 'error' ? theme.error : theme.subtle}>
          {resultDisplay}
        </Text>
      )}
    </MessageResponse>
  );
}
