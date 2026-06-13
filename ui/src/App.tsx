// Main TUI application — Claude Code style
import React, { useState, useCallback, useRef } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
import Spinner from 'ink-spinner';
import { theme } from './theme.js';
import { MessageItem } from './components/MessageItem.js';
import { StatusLine } from './components/StatusLine.js';
import { PromptInput } from './components/PromptInput.js';
import type { DisplayMessage, ToolCallInfo, Phase, StreamEvent } from './types.js';
import { checkHealth, createSession, streamChat, cancelTurn, getModels } from './client.js';

// ── helpers ──
let msgCounter = 0;
function uid(): string { return `m-${Date.now()}-${msgCounter++}`; }

function getCwd(): string { return process.cwd(); }

function getGitBranchSync(): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD']);
    if (result.exitCode !== 0) return null;
    return new TextDecoder().decode(result.stdout).trim() || null;
  } catch { return null; }
}

// ── app ──
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

  // ── init ──
  React.useEffect(() => {
    (async () => {
      try {
        const ok = await checkHealth();
        if (!ok) {
          setError('Cannot reach luwu-server — is it running?');
          setPhase('error');
          return;
        }
        const id = await createSession();
        setSessionId(id);
        const branch = getGitBranchSync();
        setGitBranch(branch);
        const models = await getModels();
        if (models.length > 0 && models[0].id) setModel(models[0].id);
        setPhase('ready');
        setMessages([{
          id: uid(),
          role: 'system',
          content: '陆吾 v0.1.0 — 输入消息开始对话',
          timestamp: Date.now(),
        }]);
      } catch (e) {
        setError(String(e));
        setPhase('error');
      }
    })();
  }, []);

  // ── send message ──
  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    const userMsg: DisplayMessage = {
      id: uid(), role: 'user', content: text.trim(), timestamp: Date.now(),
    };
    const assistantId = uid();
    setMessages(prev => [...prev, userMsg, {
      id: assistantId, role: 'assistant', content: '', tools: [], timestamp: Date.now(),
    }]);

    setPhase('thinking');
    setIteration(0);

    const controller = new AbortController();
    abortRef.current = controller;

    let accText = '';
    const toolMap = new Map<string, ToolCallInfo>();

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        switch (ev.type) {
          case 'text_delta':
            accText += ev.content || '';
            setPhase('streaming');
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accText } : m
            ));
            break;

          case 'tool_call': {
            const tc: ToolCallInfo = {
              name: ev.name || 'unknown',
              args: ev.arguments || JSON.stringify(ev.args || {}),
              status: 'running',
            };
            toolMap.set(tc.name, tc);
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, tools: Array.from(toolMap.values()) } : m
            ));
            break;
          }

          case 'tool_started':
            break;

          case 'tool_completed': {
            const name = ev.name || '';
            const ex = toolMap.get(name);
            if (ex) { ex.result = ev.result; ex.status = 'done'; }
            else toolMap.set(name, { name, args: '', result: ev.result, status: 'done' });
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, tools: Array.from(toolMap.values()) } : m
            ));
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration || 0);
            setContextPct(Math.min(99, 15 + Math.floor(accText.length / 200)));
            break;

          case 'done':
            break;

          case 'cancelled':
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accText + '\n[cancelled]' } : m
            ));
            break;

          case 'error':
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accText + `\n⚠ ${ev.message}` } : m
            ));
            break;
        }
      }, controller.signal);
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        const msg = String(e);
        setError(msg);
        setMessages(prev => prev.map(m =>
          m.id === assistantId ? { ...m, content: accText + `\n⚠ ${msg}` } : m
        ));
      }
    } finally {
      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId]);

  // ── Ctrl+C: cancel or exit ──
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

  // ── error state ──
  if (phase === 'error') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text color={theme.error} bold>✗ 连接失败</Text>
        <Text color={theme.error}>{error}</Text>
        <Text color={theme.subtle}>请确认 luwu-server 正在运行: cargo run</Text>
        <Text color={theme.subtle}>按任意键退出…</Text>
      </Box>
    );
  }

  // ── connecting state ──
  if (phase === 'connecting') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text>
          <Text color={theme.claude}><Spinner type="dots" /></Text>
          <Text color={theme.subtle}> 正在连接 luwu-server…</Text>
        </Text>
      </Box>
    );
  }

  // ── main UI ──
  const thinking = phase === 'thinking' || phase === 'streaming';

  return (
    <Box flexDirection="column">
      {/* message history */}
      <Box flexDirection="column">
        {messages.map(m => (
          <MessageItem key={m.id} msg={m} />
        ))}

        {/* thinking spinner */}
        {phase === 'thinking' && (
          <Box marginLeft={2}>
            <Text color={theme.claude}><Spinner type="dots" /></Text>
            <Text color={theme.subtle}> thinking…</Text>
          </Box>
        )}

        {/* streaming cursor */}
        {phase === 'streaming' && (
          <Box marginLeft={2}>
            <Text color={theme.claude}>▎</Text>
          </Box>
        )}
      </Box>

      {/* input */}
      <PromptInput
        onSubmit={sendMessage}
        disabled={thinking}
        phase={phase}
      />

      {/* status line */}
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
