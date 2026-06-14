// ToolResult — Claude Code 1:1
// Source: docs/04-tool-use-ui.md
// Layout: SAME as assistant text — flat row [● dot minWidth=2] [content column]
// NOT wrapped in MessageResponse — tools are independent rows
// Status: BLACK_CIRCLE for ALL states, COLOR differentiates:
//   - running/unresolved: inactive color + BLINKING animation
//   - success: theme.success green
//   - error: theme.error red
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import type { ToolCallInfo } from '../core/types.js';

export function ToolResult({ tool }: { tool: ToolCallInfo }) {
  const isRunning = tool.status === 'running';
  const isError = tool.status === 'error';
  const isDone = tool.status === 'done';

  // Blink animation for running tools
  const [visible, setVisible] = useState(true);
  useEffect(() => {
    if (!isRunning) return;
    const t = setInterval(() => setVisible(v => !v), 500);
    return () => clearInterval(t);
  }, [isRunning]);

  // Circle color: running = inactive (dimmed), done = success, error = error
  const circleColor = isDone ? theme.success : isError ? theme.error : theme.inactive;
  const circleChar = (isRunning && !visible) ? ' ' : LAYOUT.ASSISTANT_DOT;

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
  } catch { paramDisplay = tool.args; }
  if (paramDisplay.length > 60) paramDisplay = paramDisplay.slice(0, 57) + '...';

  const resultDisplay = tool.result
    ? tool.result.length > 80 ? tool.result.slice(0, 77) + '...' : tool.result
    : '';

  return (
    <Box alignItems="flex-start" flexDirection="row" width="100%" marginTop={1}>
      {/* Status circle — same minWidth=2 as assistant dot */}
      <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
        <Text color={circleColor}>{circleChar}</Text>
      </Box>
      {/* Content column */}
      <Box flexDirection="column" flexShrink={1} flexGrow={1}>
        <Box flexDirection="row" flexWrap="nowrap">
          <Text bold color={theme.text} wrap="truncate-end">{tool.name}</Text>
          {paramDisplay && <Text color={theme.inactive}> ({paramDisplay})</Text>}
        </Box>
        {!isRunning && resultDisplay && (
          <Text color={isError ? theme.error : theme.subtle}>{resultDisplay}</Text>
        )}
      </Box>
    </Box>
  );
}
