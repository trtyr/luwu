// App.tsx — composition layer: wires hooks → components
// No business logic lives here. All logic is in hooks/ and services/.
import React, { useState, useCallback, useRef, useEffect } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
import { theme } from './theme.js';
import { MessageList } from './components/MessageList.js';
import { StatusLine } from './components/StatusLine.js';
import { PromptInput } from './components/PromptInput.js';
import { Spinner } from './components/Spinner.js';
import { Welcome } from './components/Welcome.js';
import { ModelPicker } from './components/ModelPicker.js';
import type { DisplayMessage, ToolCallInfo, AssistantBlock, Phase, StreamEvent } from './core/types.js';
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

// Connecting spinner — Claude Code bouncing frames
const CONNECT_FRAMES = ['·', '✢', '✳', '✶', '✻', '✽'];

type Overlay = null | 'model';

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
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [connFrame, setConnFrame] = useState(0);
  // Transient notification — auto-dismiss after 3s (doc 20 §3.3)
  const [notification, setNotification] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const spinnerVerbRef = useRef<string | undefined>(undefined);

  const { executeCommand } = useCommands(model, setModel);

  // Notification auto-dismiss (3 seconds, per Claude Code doc 20 §3.3)
  useEffect(() => {
    if (!notification) return;
    const t = setTimeout(() => setNotification(null), 3000);
    return () => clearTimeout(t);
  }, [notification]);

  // Connecting animation
  useEffect(() => {
    if (phase !== 'connecting') return;
    const t = setInterval(() => setConnFrame(f => (f + 1) % CONNECT_FRAMES.length), 50);
    return () => clearInterval(t);
  }, [phase]);

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

  // Slash command handler — model/setting changes are SILENT (no chat message)
  const handleCommand = useCallback(async (raw: string) => {
    const result = await executeCommand(raw);
    if (result.type === 'clear') { setMessages([]); return; }
    if (result.type === 'exit') { exit(); return; }
    if (result.type === 'overlay') { setOverlay(result.overlay); return; }
    if (result.type === 'setModel') {
      // Silent switch — status bar reflects it, no chat pollution
      setModel(result.model);
      setNotification(`Model set to ${result.model}`);
      return;
    }
    // For text-output commands (stats, skills, sessions, help), show as system message
    setMessages(prev => [...prev, {
      id: uid(), role: 'system', timestamp: Date.now(), content: result.content,
    }]);
  }, [executeCommand, exit]);

  // Send message — builds blocks[] in chronological order (text + tool interleaved)
  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    const userMsg: DisplayMessage = { id: uid(), role: 'user', content: text.trim(), timestamp: Date.now() };
    const assistantId = uid();
    setMessages(prev => [...prev, userMsg, {
      id: assistantId, role: 'assistant', content: '', blocks: [], timestamp: Date.now(),
    }]);

    setPhase('thinking');
    setIteration(0);
    spinnerVerbRef.current = undefined;

    const controller = new AbortController();
    abortRef.current = controller;

    // Track state for building blocks[] in order
    let blocks: AssistantBlock[] = [];
    let currentText = '';      // accumulates into the last text block
    let accReasoning = '';
    // Track tool blocks by name for status updates
    const toolIndexMap = new Map<string, number>(); // toolName → index in blocks[]

    // Helper: flush currentText into blocks[] as a text block (or update last one)
    const flushText = () => {
      if (currentText.length === 0) return;
      // Check if last block is already a text block — if so, update it
      const last = blocks[blocks.length - 1];
      if (last && last.type === 'text') {
        last.text = currentText;
      } else {
        blocks.push({ type: 'text', text: currentText });
      }
    };

    // Helper: sync blocks[] into the assistant message
    const syncBlocks = () => {
      // content is derived from text blocks for backward compat
      const textContent = blocks
        .filter(b => b.type === 'text')
        .map(b => (b as { type: 'text'; text: string }).text)
        .join('\n\n');
      setMessages(prev => prev.map(m =>
        m.id === assistantId
          ? { ...m, blocks: [...blocks.map(b => b.type === 'text' ? { ...b } : { ...b })], content: textContent }
          : m
      ));
    };

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        switch (ev.type) {
          case 'text_delta': {
            currentText += ev.delta || '';
            setPhase('streaming');
            flushText();
            syncBlocks();
            break;
          }

          case 'reasoning_delta': {
            accReasoning += ev.delta || '';
            setMessages(prev => prev.map(m => m.id === assistantId ? { ...m, reasoning: accReasoning } : m));
            break;
          }

          case 'tool_call': {
            // Flush any accumulated text before starting a tool block
            flushText();
            currentText = ''; // reset text accumulator — next text_delta starts a new block

            spinnerVerbRef.current = ev.name || ev.tool_name || 'tool';
            const name = ev.name || ev.tool_name || 'unknown';
            const args = ev.arguments
              ? (typeof ev.arguments === 'string' ? ev.arguments : JSON.stringify(ev.arguments))
              : '';
            const toolInfo: ToolCallInfo = { name, args, status: 'running' };
            blocks.push({ type: 'tool', tool: toolInfo });
            toolIndexMap.set(name, blocks.length - 1);
            syncBlocks();
            break;
          }

          case 'tool_completed': {
            const name = ev.name || ev.tool_name || '';
            const idx = toolIndexMap.get(name);
            if (idx !== undefined && blocks[idx]?.type === 'tool') {
              blocks[idx].tool.result = ev.result || ev.output;
              blocks[idx].tool.status = 'done';
            } else {
              // Tool completed without a prior tool_call event — add it
              flushText();
              currentText = '';
              const toolInfo: ToolCallInfo = {
                name, args: '',
                result: ev.result || ev.output,
                status: 'done',
              };
              blocks.push({ type: 'tool', tool: toolInfo });
              toolIndexMap.set(name, blocks.length - 1);
            }
            syncBlocks();
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration || 0);
            setContextPct(Math.min(99, 15 + Math.floor(
              blocks.filter(b => b.type === 'text')
                .reduce((sum, b) => sum + (b as { type: 'text'; text: string }).text.length, 0) / 200
            )));
            break;

          case 'done': break;
          case 'cancelled':
            syncBlocks();
            break;
          case 'error':
            currentText += `\n⚠ ${ev.message || 'Unknown error'}`;
            flushText();
            syncBlocks();
            break;
        }
      }, controller.signal);
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        currentText += `\n⚠ ${String(e)}`;
        flushText();
        syncBlocks();
      }
    } finally {
      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId]);

  // Esc = interrupt current request (only when NOT in overlay)
  // Ctrl+C = always works (interrupt or exit)
  useInput((input, key) => {
    // When overlay is active, overlay components handle their own keys
    if (overlay) {
      if (key.ctrl && input === 'c') exit();
      return;
    }
    if (key.escape && abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
      return;
    }
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
          <Text color={theme.claude}>{CONNECT_FRAMES[connFrame]} </Text>
          <Text color={theme.text}>Connecting</Text>
          <Text color={theme.subtle}>…</Text>
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

      {overlay === 'model' ? (
        <ModelPicker
          currentModel={model}
          onSelect={(m) => {
            // Silent switch — status bar shows it, transient notification confirms
            setModel(m);
            setOverlay(null);
            setNotification(`Model set to ${m}`);
          }}
          onCancel={() => setOverlay(null)}
        />
      ) : (
        <PromptInput
          onSubmit={sendMessage}
          onCommand={handleCommand}
          disabled={busy}
          phase={phase}
        />
      )}

      <StatusLine
        model={model}
        cwd={getCwd()}
        gitBranch={gitBranch}
        contextPercent={contextPct}
        phase={phase}
        iteration={iteration}
      />

      {/* Transient notification — auto-dismiss 3s (doc 20 §3.3) */}
      {notification && (
        <Text color={theme.inactive}>{notification}</Text>
      )}
    </Box>
  );
}
