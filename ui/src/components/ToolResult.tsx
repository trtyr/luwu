// ToolResult — Claude Code 1:1
// Source: docs/04-tool-use-ui.md
// Layout: SAME structure as assistant text — flat row [⏺ dot minWidth=2] [content column]
// Status by COLOR: running=inactive+blink, success=green, error=red
// Spacing: addMargin prop controls marginTop (false when紧跟 assistant text in same turn)
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import type { ToolCallInfo } from '../core/types.js';

interface ToolResultProps {
  tool: ToolCallInfo;
  addMargin?: boolean;
}

export function ToolResult({ tool, addMargin = true }: ToolResultProps) {
  const isRunning = tool.status === 'running';
  const isError = tool.status === 'error';
  const isDone = tool.status === 'done';

  const [visible, setVisible] = useState(true);
  useEffect(() => {
    if (!isRunning) return;
    const t = setInterval(() => setVisible(v => !v), 500);
    return () => clearInterval(t);
  }, [isRunning]);

  const circleColor = isDone ? theme.success : isError ? theme.error : theme.inactive;
  const circleChar = (isRunning && !visible) ? ' ' : LAYOUT.ASSISTANT_DOT;

  // Parse args
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
    <Box
      alignItems="flex-start"
      flexDirection="row"
      width="100%"
      marginTop={addMargin ? 1 : 0}
    >
      {/* Status circle — same minWidth=2 as assistant dot, aligned */}
      <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
        <Text color={circleColor}>{circleChar}</Text>
      </Box>
      {/* Content column */}
      <Box flexDirection="column" flexShrink={1} flexGrow={1}>
        <Text bold color={theme.text} wrap="truncate-end">
          {tool.name}{paramDisplay ? ` (${paramDisplay})` : ''}
        </Text>
        {!isRunning && resultDisplay && (
          <Text color={isError ? theme.error : theme.subtle}>{resultDisplay}</Text>
        )}
      </Box>
    </Box>
  );
}
