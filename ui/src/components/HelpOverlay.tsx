// components/HelpOverlay.tsx — keyboard shortcuts + command list
// Source: Claude Code doc 29 §10.1 — PromptInputHelpMenu pattern
// Each item: shortcut (fixed width, inactive) + description (text)
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Overlay } from './Overlay.js';
import { COMMANDS } from '../core/constants.js';

const SHORTCUTS: Array<[string, string]> = [
  ['↑ ↓', 'Browse history'],
  ['/', 'Commands autocomplete'],
  ['Tab', 'Confirm autocomplete'],
  ['Esc', 'Interrupt request'],
  ['Ctrl+O', 'Toggle reasoning'],
  ['Ctrl+T', 'Toggle task list'],
  ['Ctrl+C', 'Cancel / Clear input / Exit'],
  ['Ctrl+U', 'Clear input'],
];

export function HelpOverlay({ onClose }: { onClose: () => void }) {
  return (
    <Overlay title="Help" hint="Esc to close">
      <Box flexDirection="column">
        <Text bold color={theme.text}>Keyboard Shortcuts</Text>
        {SHORTCUTS.map(([key, desc]) => (
          <Box key={key} flexDirection="row">
            <Box width={14}><Text color={theme.inactive}>{key}</Text></Box>
            <Text color={theme.text}>{desc}</Text>
          </Box>
        ))}

        <Box marginTop={1}>
          <Text bold color={theme.text}>Commands</Text>
        </Box>
        {COMMANDS.map(cmd => (
          <Box key={cmd.name} flexDirection="row">
            <Box width={14}><Text color={theme.inactive}>/{cmd.name}</Text></Box>
            <Text color={theme.text}>{cmd.description}</Text>
          </Box>
        ))}
      </Box>
    </Overlay>
  );
}
