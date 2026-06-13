// components/Spinner.tsx — Claude Code-style spinner with shimmer + random verb + elapsed timer
import React, { useState, useEffect, useRef } from 'react';
import { Box, Text } from 'ink';
import InkSpinner from 'ink-spinner';
import { theme } from '../theme.js';
import { MessageResponse } from './MessageResponse.js';

const SPINNER_VERBS = [
  'Accomplishing', 'Activating', 'Allocating', 'Analyzing', 'Building',
  'Calculating', 'Casting', 'Coalescing', 'Collecting', 'Composing',
  'Computing', 'Consulting', 'Creating', 'Crunching', 'Deciding',
  'Doing', 'Dreaming', 'Elaborating', 'Estimating', 'Evaluating',
  'Extracting', 'Finding', 'Fixing', 'Formulating', 'Gathering',
  'Generating', 'Hashing', 'Herding', 'Hovering', 'Hustling',
  'Implementing', 'Inferring', 'Loading', 'Moseying', 'Mulling',
  'Mustering', 'Optimizing', 'Orchestrating', 'Pacing', 'Percolating',
  'Pivoting', 'Planning', 'Pondering', 'Processing', 'Reasoning',
  'Reckoning', 'Reflecting', 'Remembering', 'Reticulating', 'Roaming',
  'Ruminating', 'Searching', 'Simplifying', 'Solving', 'Sorting',
  'Spinning', 'Synthesizing', 'Testing', 'Thinking', 'Translating',
  'Validating', 'Verifying', 'Visualizing', 'Wondering',
];

const TIPS = [
  'Use /clear to start fresh when switching topics and free up context',
  '↑↓ browse history · / for commands · ctrl+c to cancel',
  'Type /help to see all available commands',
];

function randomVerb(): string {
  return SPINNER_VERBS[Math.floor(Math.random() * SPINNER_VERBS.length)];
}

function randomTip(): string {
  return TIPS[Math.floor(Math.random() * TIPS.length)];
}

interface Props {
  phase: string;
  verb?: string;
  startTime?: number | null;
}

export function Spinner({ phase, verb, startTime }: Props) {
  const [mountVerb] = useState(() => randomVerb());
  const [tip] = useState(() => randomTip());
  const [thinkingStatus, setThinkingStatus] = useState<'thinking' | number | null>(null);
  const thinkingStartRef = useRef<number | null>(null);

  useEffect(() => {
    let durTimer: ReturnType<typeof setTimeout> | null = null;
    let clearTimer: ReturnType<typeof setTimeout> | null = null;

    if (phase === 'thinking') {
      if (thinkingStartRef.current === null) {
        thinkingStartRef.current = Date.now();
        setThinkingStatus('thinking');
      }
    } else if (thinkingStartRef.current !== null) {
      const duration = Date.now() - thinkingStartRef.current;
      const remaining = Math.max(0, 2000 - duration);
      thinkingStartRef.current = null;

      const showDuration = () => {
        setThinkingStatus(duration);
        clearTimer = setTimeout(() => setThinkingStatus(null), 2000);
      };

      if (remaining > 0) {
        durTimer = setTimeout(showDuration, remaining);
      } else {
        showDuration();
      }
    }

    return () => {
      if (durTimer) clearTimeout(durTimer);
      if (clearTimer) clearTimeout(clearTimer);
    };
  }, [phase]);

  // Idle or showing duration
  if (phase !== 'thinking' && thinkingStatus === null) return null;

  // Showing elapsed time after thinking
  if (phase !== 'thinking' && typeof thinkingStatus === 'number') {
    return (
      <Box marginLeft={2} marginTop={0}>
        <Text color={theme.claude}>✻ </Text>
        <Text color={theme.inactive}>Thought for </Text>
        <Text color={theme.text} bold>{(thinkingStatus / 1000).toFixed(1)}s</Text>
      </Box>
    );
  }

  // Active spinner
  const effectiveVerb = verb || mountVerb;

  return (
    <Box flexDirection="column" width="100%" alignItems="flex-start">
      <Box marginTop={1}>
        <Text>
          <Text color={theme.claude}>✻ </Text>
          <Text color={theme.text}>{effectiveVerb}…</Text>
        </Text>
      </Box>
      <MessageResponse>
        <Text dimColor>{tip}</Text>
      </MessageResponse>
    </Box>
  );
}
