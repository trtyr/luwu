// ToolResult — Claude Code doc 26 §2-§5 1:1
// Layout: flat row [⏺ dot minWidth=2] [content column]
//   tool_use: ⬤ ToolName(params) — bold name + params in parens
//   tool_result: semantic one-liner via summarizeToolResult()
//   tool_error: error message in theme.error
// Status circle: running=inactive+blink, success=green, error=red (doc 26 §2.3)
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { LAYOUT } from '../core/constants.js';
import { parseToolArgs, summarizeToolResult, toolDisplayName } from '../core/toolUtils.js';
import type { ToolCallInfo } from '../core/types.js';

interface ToolResultProps {
  tool: ToolCallInfo;
  addMargin?: boolean;
}

export function ToolResult({ tool, addMargin = true }: ToolResultProps) {
  const isRunning = tool.status === 'running';
  const isError = tool.status === 'error';
  const isDone = tool.status === 'done';

  // Blink animation for running tools (doc 26 §2.3)
  const [visible, setVisible] = useState(true);
  useEffect(() => {
    if (!isRunning) return;
    const t = setInterval(() => setVisible(v => !v), 500);
    return () => clearInterval(t);
  }, [isRunning]);

  const circleColor = isDone ? theme.success : isError ? theme.error : theme.inactive;
  const circleChar = (isRunning && !visible) ? ' ' : LAYOUT.ASSISTANT_DOT;

  // Semantic display values
  const displayName = toolDisplayName(tool.name);
  const params = parseToolArgs(tool.name, tool.args);
  const summary = !isRunning ? summarizeToolResult(tool.name, tool.result) : null;

  return (
    <Box
      alignItems="flex-start"
      flexDirection="row"
      width="100%"
      marginTop={addMargin ? 1 : 0}
    >
      {/* Status circle — minWidth=2, aligned with assistant text dot */}
      <Box minWidth={LAYOUT.DOT_MIN_WIDTH}>
        <Text color={circleColor}>{circleChar}</Text>
      </Box>

      {/* Content column */}
      <Box flexDirection="column" flexShrink={1} flexGrow={1}>
        {/* Tool use line: bold name + params in parens (doc 26 §2.1) */}
        <Text bold color={theme.text} wrap="truncate-end">
          {displayName}{params ? ` (${params})` : ''}
        </Text>

        {/* Result line — semantic summary (doc 26 §3.5, §5) */}
        {!isRunning && summary && (
          <Text color={isError ? theme.error : theme.subtle}>
            {summary}
          </Text>
        )}
      </Box>
    </Box>
  );
}
