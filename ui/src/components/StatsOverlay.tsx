// components/StatsOverlay.tsx — server runtime stats panel
// Source: Claude Code doc 29 §5.4 — Stats component
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import { Overlay } from './Overlay.js';
import { getStats } from '../services/api.js';
import type { StatsResponse } from '../core/types.js';
import { formatCost, getModelCost } from '../core/constants.js';

export function StatsOverlay({ sessionId, model, contextPercent, costTotal, costSaved }: {
  sessionId: string;
  model: string;
  contextPercent: number;
  costTotal: number;
  costSaved: number;
}) {
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getStats().then(setStats).catch(() => setError('Failed to fetch stats'));
  }, []);

  // Compute the cache-saved percentage for the cost row. Mirrors the
  // IIFE in StatusLine.tsx so the two readouts stay consistent.
  const costRaw = costTotal + costSaved;
  const costSavedPct = costRaw > 0
    ? Math.min(100, Math.round((costSaved / costRaw) * 100))
    : 0;
  // Show the per-provider rate so the user can verify the cost math
  // (e.g. DeepSeek V4 hit is $0.0028/MTok, miss is $0.14/MTok).
  const costRate = getModelCost(model);

  const rows: Array<[string, string]> = [
    ['Model', model],
    ['Session', sessionId ? sessionId.slice(0, 8) + '…' : '(none)'],
    ['Context', `${contextPercent.toFixed(1)}%`],
    ['Cost (this turn)', costTotal > 0
      ? `${formatCost(costTotal)}${costSavedPct > 0 ? ` (${costSavedPct}% saved)` : ''}`
      : '—'],
    ['Cost saved', costSaved > 0 ? formatCost(costSaved) : '—'],
    ['Rate (hit/miss)', `$${costRate.hit}/$${costRate.miss} per MTok`],
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
              <Box width={18}><Text color={theme.inactive}>{label}</Text></Box>
              <Text color={theme.text} bold>{value}</Text>
            </Box>
          ))}
        </Box>
      )}
    </Overlay>
  );
}
