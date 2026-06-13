// components/MessageList.tsx — message dispatcher with addMargin spacing
import React from 'react';
import { Box } from 'ink';
import { UserMessage } from './UserMessage.js';
import { AssistantMessage } from './AssistantMessage.js';
import { SystemMessage } from './SystemMessage.js';
import type { DisplayMessage } from '../core/types.js';

interface Props {
  messages: DisplayMessage[];
}

export function MessageList({ messages }: Props) {
  return (
    <Box flexDirection="column">
      {messages.map((msg, i) => {
        // First message has no margin, subsequent messages get marginTop=1
        const addMargin = i > 0;

        if (msg.role === 'user') return <UserMessage key={msg.id} msg={msg} addMargin={addMargin} />;
        if (msg.role === 'assistant') return <AssistantMessage key={msg.id} msg={msg} addMargin={addMargin} />;
        return <SystemMessage key={msg.id} msg={msg} addMargin={addMargin} />;
      })}
    </Box>
  );
}
