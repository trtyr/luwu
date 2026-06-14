// components/SessionsOverlay.tsx — session list with Select navigation
// Source: Claude Code doc 29 §4.1 — LogSelector pattern
// Shows sessions, Enter to resume (switch), Esc to close
import React, { useState, useEffect } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';
import { Overlay } from './Overlay.js';
import { listSessions } from '../services/api.js';

interface SessionInfo {
  id: string;
  model: string;
  message_count: number;
  is_running: boolean;
}

const MAX_VISIBLE = 8;

export function SessionsOverlay({ onRestore }: { onRestore: (id: string) => void }) {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState(0);

  useEffect(() => {
    listSessions().then(s => {
      setSessions(s);
      setLoading(false);
    }).catch(() => setLoading(false));
  }, []);

  useInput((_, key) => {
    if (sessions.length === 0) return;
    if (key.upArrow) setSelected(p => Math.max(0, p - 1));
    if (key.downArrow) setSelected(p => Math.min(sessions.length - 1, p + 1));
    if (key.return) {
      const session = sessions[selected];
      if (session) onRestore(session.id);
    }
  });

  const start = Math.max(0, selected - Math.floor(MAX_VISIBLE / 2));
  const end = Math.min(sessions.length, start + MAX_VISIBLE);
  const visible = sessions.slice(start, end);

  return (
    <Overlay title="Sessions" hint="↑↓ select · Enter restore · Esc close">
      {loading ? (
        <Text color={theme.inactive}>Loading…</Text>
      ) : sessions.length === 0 ? (
        <Text color={theme.inactive}>(No sessions)</Text>
      ) : (
        <Box flexDirection="column">
          {start > 0 && (
            <Text color={theme.subtle}>↑ {start} more</Text>
          )}
          {visible.map((s, i) => {
            const realIdx = start + i;
            const isSelected = realIdx === selected;
            return (
              <Box key={s.id} flexDirection="row">
                <Text color={isSelected ? theme.text : theme.inactive}>
                  {isSelected ? '❯ ' : '  '}
                </Text>
                <Box width={10}>
                  <Text color={theme.claude}>{s.id.slice(0, 8)}</Text>
                </Box>
                <Box width={20}>
                  <Text color={theme.text} bold={isSelected}>{s.model}</Text>
                </Box>
                <Text color={theme.inactive}>{s.message_count} msgs</Text>
                {s.is_running && <Text color={theme.success}> ●running</Text>}
              </Box>
            );
          })}
          {end < sessions.length && (
            <Text color={theme.subtle}>↓ {sessions.length - end} more</Text>
          )}
        </Box>
      )}
    </Overlay>
  );
}
