// components/StatsOverlay.tsx — server runtime stats panel
// Source: Claude Code doc 29 §5.4 — Stats component
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Overlay } from './Overlay.js';
import { getStats } from '../services/api.js';
import type { StatsResponse } from '../core/types.js';

export function StatsOverlay({ sessionId, model, contextPercent }: {
  sessionId: string;
  model: string;
  contextPercent: number;
}) {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(() => setError('Failed to fetch stats'));
  }, []);

  const rows: Array<[string, string]> = [
    ['Model', model],
    ['Session', sessionId ? sessionId.slice(0, 8) + '…' : '(none)'],
    ['Context', `${contextPercent.toFixed(1)}%`],
    ['Sessions', stats ? `${stats.sessions.total} total · ${stats.sessions.running} running` : '…'],
    ['Workers', stats ? String(stats.workers) : '…'],
  ];

  return (
    <Overlay title="Stats" hint="Esc to close">
      {error ? (
        <Text color={theme.error}>{error}</Text>
      ) : (
        <Box flexDirection="column">
          {rows.map(([label, value]) => (
            <Box key={label} flexDirection="row">
              <Box width={14}><Text color={theme.inactive}>{label}</Text></Box>
              <Text color={theme.text} bold>{value}</Text>
            </Box>
          ))}
        </Box>
      )}
    </Overlay>
  );
}
