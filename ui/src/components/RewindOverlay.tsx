// RewindOverlay — two-phase UI matching Claude Code's MessageSelector
// Phase 1: user message list (up/down to navigate, Enter to select, Esc to close)
// Phase 2: restore options panel (Restore both/code/conversation, Summarize, Never mind)
import React, { useState, useEffect, useCallback } from 'react';
import { Box, Text, useInput } from 'ink';
import { theme } from '../theme.js';
import {
  getRewindMessages, rewindSession, summarizeFrom,
  type RewindMessageInfo,
} from '../services/api.js';

interface Props {
  sessionId: string;
  onClose: () => void;
  onRewind: (restoredText: string, remaining: number) => void;
}

type Phase = 'list' | 'confirm' | 'busy';

const MAX_VISIBLE = 7;

const RESTORE_OPTIONS = [
  { value: 'both', label: 'Restore code and conversation' },
  { value: 'conversation', label: 'Restore conversation only' },
  { value: 'code', label: 'Restore code only' },
  { value: 'summarize', label: 'Summarize from here' },
  { value: 'nevermind', label: 'Never mind' },
] as const;

export function RewindOverlay({ sessionId, onClose, onRewind }: Props) {
  const [phase, setPhase] = useState<Phase>('list');
  const [messages, setMessages] = useState<RewindMessageInfo[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [optionIdx, setOptionIdx] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [resultMsg, setResultMsg] = useState<string | null>(null);

  useEffect(() => {
    getRewindMessages(sessionId)
      .then(msgs => {
        setMessages(msgs);
        setLoading(false);
        if (msgs.length === 0) setError('No messages to rewind to');
      })
      .catch(e => {
        setError(String(e));
        setLoading(false);
      });
  }, [sessionId]);

  useInput(useCallback((_input: string, key: any) => {
    if (phase !== 'list') return;
    if (key.upArrow) setSelectedIdx(i => Math.max(0, i - 1));
    else if (key.downArrow) setSelectedIdx(i => Math.min(messages.length - 1, i + 1));
    else if (key.return) { setPhase('confirm'); setOptionIdx(0); }
    else if (key.escape) onClose();
  }, [phase, messages.length, selectedIdx, onClose]));

  useInput(useCallback((_input: string, key: any) => {
    if (phase !== 'confirm') return;
    if (key.upArrow) setOptionIdx(i => Math.max(0, i - 1));
    else if (key.downArrow) setOptionIdx(i => Math.min(RESTORE_OPTIONS.length - 1, i + 1));
    else if (key.return) handleRestore(RESTORE_OPTIONS[optionIdx].value);
    else if (key.escape) setPhase('list');
  }, [phase, optionIdx]));

  async function handleRestore(option: string) {
    setPhase('busy');
    const msg = messages[selectedIdx];
    try {
      if (option === 'nevermind') { setPhase('list'); return; }

      if (option === 'both') {
        const r = await rewindSession(sessionId, msg.index, true, true);
        if (r) {
          setResultMsg(`Restored ${r.files_changed.length} file(s)`);
          onRewind(r.restored_text, r.remaining_messages);
          return;
        }
      }
      if (option === 'conversation') {
        const r = await rewindSession(sessionId, msg.index, false, true);
        if (r) { onRewind(r.restored_text, r.remaining_messages); return; }
      }
      if (option === 'code') {
        const r = await rewindSession(sessionId, msg.index, true, false);
        if (r) {
          setResultMsg(`Restored ${r.files_changed.length} file(s)`);
          setPhase('list');
          return;
        }
      }
      if (option === 'summarize') {
        const r = await summarizeFrom(sessionId, msg.index, 'from');
        if (r) {
          setResultMsg(`Summarized ${r.messages_removed} messages`);
          onRewind('', 0);
          return;
        }
      }
      setError('Rewind failed');
      setPhase('confirm');
    } catch (e) {
      setError(String(e));
      setPhase('confirm');
    }
  }

  if (loading) {
    return (
      <Box flexDirection="column" paddingX={1}>
        <Text color={theme.permission}>{'▔'.repeat(60)}</Text>
        <Text bold color={theme.suggestion}>{'  Rewind'}</Text>
        <Text color={theme.inactive}>  Loading messages…</Text>
      </Box>
    );
  }

  if (error) {
    return (
      <Box flexDirection="column" paddingX={1}>
        <Text color={theme.permission}>{'▔'.repeat(60)}</Text>
        <Text bold color={theme.suggestion}>{'  Rewind'}</Text>
        <Text color={theme.error}>  ✗ {error}</Text>
        <Text color={theme.inactive}>  Esc to close</Text>
      </Box>
    );
  }

  if (phase === 'busy') {
    return (
      <Box flexDirection="column" paddingX={1}>
        <Text color={theme.permission}>{'▔'.repeat(60)}</Text>
        <Text bold color={theme.suggestion}>{'  Rewind'}</Text>
        <Text color={theme.inactive}>  Working… {resultMsg}</Text>
      </Box>
    );
  }

  // Phase 1: Message list
  if (phase === 'list') {
    const first = Math.max(0, Math.min(
      selectedIdx - Math.floor(MAX_VISIBLE / 2),
      messages.length - MAX_VISIBLE,
    ));
    const visible = messages.slice(first, first + MAX_VISIBLE);

    return (
      <Box flexDirection="column">
        <Text color={theme.permission}>{'▔'.repeat(60)}</Text>
        <Box flexDirection="column" marginX={1}>
          <Text bold color={theme.suggestion}>Rewind</Text>
          <Text color={theme.inactive}>Restore the code and/or conversation to the point before…</Text>
          <Box flexDirection="column" marginTop={1}>
            {visible.map((msg, vi) => {
              const realIdx = first + vi;
              const isSelected = realIdx === selectedIdx;
              return (
                <Box key={realIdx} flexDirection="column">
                  <Box>
                    <Box width={2}>
                      {isSelected
                        ? <Text color={theme.permission} bold>{'❯'}</Text>
                        : <Text>{'  '}</Text>}
                    </Box>
                    <Text color={isSelected ? theme.suggestion : theme.inactive}>
                      {msg.text.length > 70 ? msg.text.slice(0, 70) + '…' : msg.text}
                    </Text>
                  </Box>
                  {msg.diff_stats && (msg.diff_stats.files_changed > 0 || msg.diff_stats.insertions > 0) && (
                    <Box paddingLeft={2}>
                      <Text color={theme.subtle}>
                        {msg.diff_stats.files_changed > 0 ? `${msg.diff_stats.files_changed} file(s) ` : ''}
                        <Text color={theme.success}>+{msg.diff_stats.insertions}</Text>
                        {' '}
                        <Text color={theme.error}>-{msg.diff_stats.deletions}</Text>
                      </Text>
                    </Box>
                  )}
                </Box>
              );
            })}
          </Box>
          <Box marginTop={1}>
            <Box width={2}><Text>{'  '}</Text></Box>
            <Text italic color={theme.inactive}>(current)</Text>
          </Box>
          <Box marginTop={1}>
            <Text color={theme.inactive} italic>
              Enter to continue · Esc to exit
            </Text>
          </Box>
        </Box>
      </Box>
    );
  }

  // Phase 2: Confirm restore options
  const msg = messages[selectedIdx];
  const descMap: Record<string, string> = {
    both: 'Conversation will be forked. Code files will be restored.',
    conversation: 'Conversation will be forked. Code will be unchanged.',
    code: 'Conversation will be unchanged. Code files will be restored.',
    summarize: 'Messages after this point will be compressed into a summary.',
    nevermind: 'Nothing will change.',
  };

  return (
    <Box flexDirection="column">
      <Text color={theme.permission}>{'▔'.repeat(60)}</Text>
      <Box flexDirection="column" marginX={1}>
        <Text bold color={theme.suggestion}>Rewind</Text>
        <Text color={theme.text}>Confirm restore to the point before you sent:</Text>
        <Box flexDirection="column" paddingLeft={1} borderStyle="single" borderRight={false} borderTop={false} borderBottom={false} borderLeft={true}>
          <Text color={theme.text}>
            {msg.text.length > 80 ? msg.text.slice(0, 80) + '…' : msg.text}
          </Text>
        </Box>
        <Box flexDirection="column" marginTop={1}>
          {RESTORE_OPTIONS.map((opt, i) => (
            <Box key={opt.value}>
              <Box width={2}>
                {i === optionIdx
                  ? <Text color={theme.permission} bold>{'❯'}</Text>
                  : <Text>{'  '}</Text>}
              </Box>
              <Text color={i === optionIdx ? theme.suggestion : theme.inactive}>{opt.label}</Text>
            </Box>
          ))}
        </Box>
        <Box marginTop={1}>
          <Text color={theme.subtle}>{descMap[RESTORE_OPTIONS[optionIdx].value]}</Text>
        </Box>
        <Box marginTop={1}>
          <Text color={theme.inactive} italic>
            Enter to confirm · Esc to go back
          </Text>
        </Box>
      </Box>
    </Box>
  );
}
