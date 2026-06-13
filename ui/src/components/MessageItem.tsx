// Single message display
import React from 'react';
import { Box, Text } from 'ink';
import { Markdown } from './Markdown';
import { ToolBadge } from './ToolBadge';
import type { DisplayMessage } from '../types';

export function MessageItem({ message }: { message: DisplayMessage }) {
  if (message.role === 'system') {
    // System messages — render as dim preformatted text
    return (
      <Box flexDirection="column" marginY={0}>
        {message.content.split('\n').map((line, i) => (
          <Text key={i} dimColor>{line}</Text>
        ))}
      </Box>
    );
  }

  if (message.role === 'user') {
    return (
      <Box flexDirection="column" marginY={0}>
        <Box>
          <Text bold color="cyan">❯ </Text>
          <Text>{message.content}</Text>
        </Box>
      </Box>
    );
  }

  // assistant
  return (
    <Box flexDirection="column" marginY={0}>
      {message.tools && message.tools.length > 0 && (
        <Box flexDirection="column">
          {message.tools.map((tool, i) => (
            <ToolBadge key={i} tool={tool} />
          ))}
        </Box>
      )}
      {message.content && (
        <Box marginLeft={2}>
          <Markdown>{message.content}</Markdown>
        </Box>
      )}
    </Box>
  );
}
