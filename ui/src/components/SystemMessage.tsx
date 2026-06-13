// components/SystemMessage.tsx — system messages in dim gray
import React from 'react';
import { Text } from 'ink';
import { theme } from '../theme.js';
import { truncateText } from '../core/constants.js';
import type { DisplayMessage } from '../core/types.js';

export function SystemMessage({ msg, addMargin }: { msg: DisplayMessage; addMargin: boolean }) {
  return <Text color={theme.inactive} dimColor>{truncateText(msg.content)}</Text>;
}
