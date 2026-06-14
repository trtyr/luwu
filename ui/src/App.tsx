// App.tsx — pure composition layer
// Wires hooks → components. No business logic, no stream processing.
// Overlay system: all interactive commands render in overlays (doc 29)
import React, { useState, useCallback, useEffect } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
import { theme } from './theme.js';
import { MessageList } from './components/MessageList.js';
import { StatusLine } from './components/StatusLine.js';
import { PromptInput } from './components/PromptInput.js';
import { Spinner } from './components/Spinner.js';
import { Welcome } from './components/Welcome.js';
import { ModelPicker } from './components/ModelPicker.js';
import { HelpOverlay } from './components/HelpOverlay.js';
import { StatsOverlay } from './components/StatsOverlay.js';
import { SkillsOverlay } from './components/SkillsOverlay.js';
import { SessionsOverlay } from './components/SessionsOverlay.js';
import { isBusy } from './core/state.js';
import type { TaskItem } from './core/types.js';
import type { OverlayType } from './hooks/useCommands.js';
import { useChatSession } from './hooks/useChatSession.js';
import { useCommands } from './hooks/useCommands.js';
import { getTasks } from './services/api.js';

export function App() {
  const { exit } = useApp();
  const chat = useChatSession();
  const { executeCommand } = useCommands(chat.model, chat.setModel);

  const [overlay, setOverlay] = useState<OverlayType | null>(null);
  const [notification, setNotification] = useState<string | null>(null);
  const [inputValue, setInputValue] = useState('');
  const [showTasks, setShowTasks] = useState(false);
  const [tasks, setTasks] = useState<TaskItem[]>([]);

  // Notification auto-dismiss (3s, doc 20 §3.3)
  useEffect(() => {
    if (!notification) return;
    const t = setTimeout(() => setNotification(null), 3000);
    return () => clearTimeout(t);
  }, [notification]);

  // Fetch tasks when showTasks is on and agent is busy (doc 27)
  useEffect(() => {
    if (!showTasks || !chat.sessionId) return;
    if (!isBusy(chat.phase)) return;
    let active = true;
    const fetch = () => { if (active && chat.sessionId) getTasks(chat.sessionId).then(setTasks).catch(() => {}); };
    fetch();
    const timer = setInterval(fetch, 2000);
    return () => { active = false; clearInterval(timer); };
  }, [showTasks, chat.sessionId, chat.phase]);

  // Also fetch once when a todo tool completes
  useEffect(() => {
    if (chat.phase !== 'ready' || !chat.sessionId) return;
    getTasks(chat.sessionId).then(setTasks).catch(() => {});
  }, [chat.phase, chat.sessionId]);

  // Slash command handler
  const handleCommand = useCallback(async (raw: string) => {
    const result = await executeCommand(raw);
    if (result.type === 'clear') { chat.setMessages([]); return; }
    if (result.type === 'exit') { exit(); return; }
    if (result.type === 'newSession') { await chat.newSession(); return; }
    if (result.type === 'overlay') { setOverlay(result.overlay); return; }
    if (result.type === 'setModel') {
      chat.setModel(result.model);
      setNotification(`Model set to ${result.model}`);
      return;
    }
  }, [executeCommand, exit, chat]);

  // Global keyboard
  useInput((input, key) => {
    if (overlay) {
      if (key.escape) { setOverlay(null); return; }
      if (key.ctrl && input === 'c') exit();
      return;
    }
    // Esc interrupts streaming
    if (key.escape && chat.abortRef.current) {
      chat.cancel();
      return;
    }
    // Ctrl+T: toggle task list visibility (doc 27 §3.2)
    if (key.ctrl && input === 't') {
      setShowTasks(prev => !prev);
      return;
    }
    // Ctrl+C: streaming → cancel | has text → clear | empty → exit
    if (key.ctrl && input === 'c') {
      if (chat.abortRef.current) { chat.cancel(); return; }
      if (inputValue.length > 0) { setInputValue(''); return; }
      exit();
    }
  });

  // ── Error state ──
  if (chat.phase === 'error') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text color={theme.error} bold>✗ 连接失败</Text>
        <Text color={theme.error}>{chat.error}</Text>
        <Text color={theme.subtle}>请确认 luwu-server 正在运行: cargo run</Text>
      </Box>
    );
  }

  // ── Connecting state ──
  if (chat.phase === 'connecting') {
    return <ConnectingView />;
  }

  // ── Render overlay content ──
  function renderOverlay() {
    switch (overlay) {
      case 'help':
        return <HelpOverlay onClose={() => setOverlay(null)} />;
      case 'stats':
        return <StatsOverlay sessionId={chat.sessionId ?? ''} model={chat.model} contextPercent={chat.contextPct} />;
      case 'skills':
        return <SkillsOverlay />;
      case 'sessions':
        return (
          <SessionsOverlay
            onRestore={(id) => {
              chat.restoreSession(id);
              setOverlay(null);
            }}
          />
        );
      case 'model':
        return (
          <ModelPicker
            currentModel={chat.model}
            onSelect={(m) => {
              chat.setModel(m);
              setOverlay(null);
              setNotification(`Model set to ${m}`);
            }}
            onCancel={() => setOverlay(null)}
          />
        );
      default:
        return null;
    }
  }

  // ── Main composition ──
  return (
    <Box flexDirection="column">
      <Welcome />
      <MessageList messages={chat.messages} />
      <Spinner
        phase={chat.phase}
        verb={chat.spinnerVerb}
        tasks={tasks}
        showTasks={showTasks}
      />

      {overlay ? (
        renderOverlay()
      ) : (
        <PromptInput
          value={inputValue}
          onValueChange={setInputValue}
          onSubmit={chat.sendMessage}
          onCommand={handleCommand}
          disabled={isBusy(chat.phase)}
          phase={chat.phase}
        />
      )}

      <StatusLine
        model={chat.model}
        sessionId={chat.sessionId}
        cwd={process.cwd()}
        gitBranch={chat.gitBranch}
        contextPercent={chat.contextPct}
        phase={chat.phase}
        iteration={chat.iteration}
      />

      {notification && (
        <Text color={theme.inactive}>{notification}</Text>
      )}
    </Box>
  );
}

// ── Connecting spinner (Claude Code bouncing frames) ──
const CONNECT_FRAMES = ['·', '✢', '✳', '✶', '✻', '✽'];

function ConnectingView() {
  const [frame, setFrame] = React.useState(0);
  React.useEffect(() => {
    const t = setInterval(() => setFrame(f => (f + 1) % CONNECT_FRAMES.length), 50);
    return () => clearInterval(t);
  }, []);
  return (
    <Box flexDirection="column" padding={1}>
      <Box>
        <Text color={theme.claude}>{CONNECT_FRAMES[frame]} </Text>
        <Text color={theme.text}>Connecting</Text>
        <Text color={theme.subtle}>…</Text>
      </Box>
    </Box>
  );
}
