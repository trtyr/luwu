import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import type { DisplayMessage, ToolCallInfo } from '../types.js';

/**
 * Message dispatcher — routes by role, like Claude Code's Message.tsx
 */
export function MessageItem({ msg }: { msg: DisplayMessage }) {
  if (msg.role === 'user') return <UserMessage msg={msg} />;
  if (msg.role === 'assistant') return <AssistantMessage msg={msg} />;
  return <SystemMessage msg={msg} />;
}

/**
 * User message: "❯ " prefix in suggestion blue
 */
function UserMessage({ msg }: { msg: DisplayMessage }) {
  return (
    <Box flexDirection="column" marginY={0}>
      <Box>
        <Text color={theme.suggestion} bold>❯ </Text>
        <Text color={theme.text}>{msg.content}</Text>
      </Box>
    </Box>
  );
}

/**
 * Assistant message: "● " prefix in Claude orange + tool results
 */
function AssistantMessage({ msg }: { msg: DisplayMessage }) {
  return (
    <Box flexDirection="column" marginY={0}>
      {msg.content && (
        <Box>
          <Text color={theme.claude} bold>● </Text>
          <Text color={theme.text}>{msg.content}</Text>
        </Box>
      )}
      {msg.tools?.map((tool, i) => (
        <ToolBadge key={i} tool={tool} />
      ))}
    </Box>
  );
}

/**
 * System message: "○ " prefix in inactive gray
 */
function SystemMessage({ msg }: { msg: DisplayMessage }) {
  return (
    <Box>
      <Text color={theme.inactive}>○ </Text>
      <Text color={theme.inactive} italic>{msg.content}</Text>
    </Box>
  );
}

/**
 * Tool call badge: name + status indicator
 * Claude Code style: dimmed box with tool name, result collapsed
 */
function ToolBadge({ tool }: { tool: ToolCallInfo }) {
  const icon =
    tool.status === 'running' ? '⟳' :
    tool.status === 'done' ? '✓' :
    '✗';

  const color =
    tool.status === 'running' ? theme.warning :
    tool.status === 'done' ? theme.success :
    theme.error;

  return (
    <Box marginLeft={2} marginY={0}>
      <Text color={color}>{icon} </Text>
      <Text color={theme.subtle}>tool: </Text>
      <Text color={theme.inactiveShimmer}>{tool.name}</Text>
      {tool.args && tool.args !== '{}' && (
        <Text color={theme.subtle}> {truncate(tool.args, 80)}</Text>
      )}
      {tool.result && (
        <Text color={theme.subtle}> → {truncate(tool.result, 100)}</Text>
      )}
    </Box>
  );
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 3) + '...';
}
