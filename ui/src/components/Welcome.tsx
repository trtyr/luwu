// Welcome.tsx — welcome banner
// Claude Code uses dimColor = theme.inactive for hints
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';

export function Welcome({ version = '0.1.0' }: { version?: string }) {
  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text>
        <Text color={theme.claude} bold>陆吾</Text>
        <Text color={theme.subtle}> v{version}</Text>
        <Text color={theme.subtle}> — 昆仑山的管家</Text>
      </Text>
      <Text color={theme.subtle}>{'━'.repeat(50)}</Text>
      <Text color={theme.inactive}>
        <Text color={theme.suggestion}>↑↓</Text> 浏览历史 ·{' '}
        <Text color={theme.suggestion}>/</Text> 查看命令 ·{' '}
        <Text color={theme.suggestion}>esc</Text> 中断 ·{' '}
        <Text color={theme.suggestion}>ctrl+c</Text> 退出
      </Text>
    </Box>
  );
}
