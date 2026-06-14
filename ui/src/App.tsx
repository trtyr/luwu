// App.tsx — composition layer: wires hooks → components
// No business logic lives here. All logic is in hooks/ and services/.
import React, { useState, useCallback, useRef, useEffect } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
import InkSpinner from 'ink-spinner';
import { theme } from './theme.js';
import { MessageList } from './components/MessageList.js';
import { StatusLine } from './components/StatusLine.js';
import { PromptInput } from './components/PromptInput.js';
import { Spinner } from './components/Spinner.js';
import { Welcome } from './components/Welcome.js';
import type { DisplayMessage, ToolCallInfo, Phase, StreamEvent } from './core/types.js';
import { isBusy } from './core/state.js';
import { useCommands } from './hooks/useCommands.js';
import {
  checkHealth, createSession, streamChat, cancelTurn, getModels,
} from './services/api.js';

let msgCounter = 0;
const uid = (): string => `m-${Date.now()}-${msgCounter++}`;

function getCwd(): string { return process.cwd(); }

function getGitBranchSync(): string | null {
  try {
    const r = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD']);
    if (r.exitCode !== 0) return null;
    return new TextDecoder().decode(r.stdout).trim() || null;
  } catch { return null; }
}

export function App() {
  const { exit } = useApp();
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [phase, setPhase] = useState<Phase>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState('glm-4.7');
  const [gitBranch, setGitBranch] = useState<string | null>(null);
  const [contextPct, setContextPct] = useState(0);
  const [iteration, setIteration] = useState(0);
  const abortRef = useRef<AbortController | null>(null);
  const spinnerVerbRef = useRef<string | undefined>(undefined);

  const { executeCommand } = useCommands(model);

  // Init
  useEffect(() => {
    (async () => {
      try {
        const ok = await checkHealth();
        if (!ok) { setError('Cannot reach luwu-server'); setPhase('error'); return; }
        const id = await createSession();
        setSessionId(id);
        setGitBranch(getGitBranchSync());
        try {
          const models = await getModels();
          if (models.length > 0 && models[0].id) setModel(models[0].id);
        } catch { /* default */ }
        setPhase('ready');
        setMessages([{
          id: uid(), role: 'system', timestamp: Date.now(),
          content: '陆吾 v0.1.0 — 输入消息开始对话 · ↑↓ 浏览历史 · / 查看命令',
        }]);
      } catch (e) {
        setError(String(e));
        setPhase('error');
      }
    })();
  }, []);

  // Slash command handler
  const handleCommand = useCallback(async (raw: string) => {
    const result = await executeCommand(raw);
    if (result.type === 'clear') { setMessages([]); return; }
    if (result.type === 'exit') { exit(); return; }
    setMessages(prev => [...prev, {
      id: uid(), role: 'system', timestamp: Date.now(), content: result.content,
    }]);
  }, [executeCommand, exit]);

  // Send message
  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    const userMsg: DisplayMessage = { id: uid(), role: 'user', content: text.trim(), timestamp: Date.now() };
    const assistantId = uid();
    setMessages(prev => [...prev, userMsg, {
      id: assistantId, role: 'assistant', content: '', tools: [], timestamp: Date.now(),
    }]);

    setPhase('thinking');
    setIteration(0);
    spinnerVerbRef.current = undefined;

    const controller = new AbortController();
    abortRef.current = controller;

    let accText = '';
    let accReasoning = '';
    const toolMap = new Map<string, ToolCallInfo>();

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        switch (ev.type) {
          case 'text_delta':
            accText += ev.delta || '';
            setPhase('streaming');
            setMessages(prev => prev.map(m => m.id === assistantId ? { ...m, content: accText } : m));
            break;

          case 'reasoning_delta':
            accReasoning += ev.delta || '';
            setMessages(prev => prev.map(m => m.id === assistantId ? { ...m, reasoning: accReasoning } : m));
            break;

          case 'tool_call': {
            spinnerVerbRef.current = ev.name || ev.tool_name || 'tool';
            const name = ev.name || ev.tool_name || 'unknown';
            const args = ev.arguments
              ? (typeof ev.arguments === 'string' ? ev.arguments : JSON.stringify(ev.arguments))
              : '';
            toolMap.set(name, { name, args, status: 'running' });
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, tools: Array.from(toolMap.values()) } : m));
            break;
          }

          case 'tool_completed': {
            const name = ev.name || ev.tool_name || '';
            const ex = toolMap.get(name);
            if (ex) { ex.result = ev.result || ev.output; ex.status = 'done'; }
            else toolMap.set(name, { name, args: '', result: ev.result || ev.output, status: 'done' });
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, tools: Array.from(toolMap.values()) } : m));
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration || 0);
            setContextPct(Math.min(99, 15 + Math.floor(accText.length / 200)));
            break;

          case 'done': break;
          case 'cancelled':
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accText } : m));
            break;
          case 'error':
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accText + `\n⚠ ${ev.message}` } : m));
            break;
        }
      }, controller.signal);
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        setMessages(prev => prev.map(m =>
          m.id === assistantId ? { ...m, content: accText + `\n⚠ ${String(e)}` } : m));
      }
    } finally {
      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId]);

  // Ctrl+C
  useInput((input, key) => {
    if (key.ctrl && input === 'c') {
      if (abortRef.current) {
        abortRef.current.abort();
        if (sessionId) cancelTurn(sessionId).catch(() => {});
        return;
      }
      exit();
    }
  });

  // Error state
  if (phase === 'error') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text color={theme.error} bold>✗ 连接失败</Text>
        <Text color={theme.error}>{error}</Text>
        <Text color={theme.subtle}>请确认 luwu-server 正在运行: cargo run</Text>
      </Box>
    );
  }

  // Connecting state
  if (phase === 'connecting') {
    return (
      <Box flexDirection="column" padding={1}>
        <Box>
          <Text color={theme.claude}><InkSpinner type="dots" /></Text>
          <Text color={theme.subtle}> 正在连接 luwu-server…</Text>
        </Box>
      </Box>
    );
  }

  const busy = isBusy(phase);

  return (
    <Box flexDirection="column">
      <Welcome />
      <MessageList messages={messages} />

      <Spinner phase={phase} verb={spinnerVerbRef.current} />

      {phase === 'streaming' && (
        <Box marginTop={0}><Text color={theme.claude}>▎</Text></Box>
      )}

      <PromptInput
        onSubmit={sendMessage}
        onCommand={handleCommand}
        disabled={busy}
        phase={phase}
      />

      <StatusLine
        model={model}
        cwd={getCwd()}
        gitBranch={gitBranch}
        contextPercent={contextPct}
        phase={phase}
        iteration={iteration}
      />
    </Box>
  );
}
