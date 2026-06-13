// Main TUI application — orchestrates chat, input, streaming, status
import React, { useState, useCallback } from 'react';
import { Box, Text, useApp, useInput } from 'ink';
import { CustomInput } from './components/CustomInput';
import Spinner from 'ink-spinner';

import { MessageItem } from './components/MessageItem';
import { StatusBar } from './components/StatusBar';
import type { DisplayMessage, ToolCallInfo, Phase, StreamEvent } from './types';
import { checkHealth, createSession, streamChat, cancelTurn } from './client';

// ── helpers ──

let msgCounter = 0;
function uid(): string { return `m-${Date.now()}-${msgCounter++}`; }

function getCwd(): string {
  return process.cwd();
}

function getGitBranchSync(): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD']);
    if (result.exitCode !== 0) return null;
    const text = new TextDecoder().decode(result.stdout);
    return text.trim() || null;
  } catch {
    return null;
  }
}

// ── app ──

export function App() {
  const { exit } = useApp();
  const [input, setInput] = useState('');
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [phase, setPhase] = useState<Phase>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState('MiniMax-M3');
  const [gitBranch, setGitBranch] = useState<string | null>(null);
  const [contextPct, setContextPct] = useState(0);
  const abortRef = React.useRef<AbortController | null>(null);

  // ── init: health check + session create ──
  React.useEffect(() => {
    (async () => {
      try {
        const ok = await checkHealth();
        if (!ok) {
          setError('Cannot reach luwu-server at http://127.0.0.1:51740 — is it running?');
          setPhase('error');
          return;
        }
        const id = await createSession();
        setSessionId(id);
        const branch = getGitBranchSync();
        setGitBranch(branch);
        setPhase('ready');

        // welcome message
        setMessages([{
          id: uid(),
          role: 'system',
          content: '陆吾 TUI 已连接 — 输入消息开始对话，Ctrl+C 退出',
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

    // add user message
    const userMsg: DisplayMessage = {
      id: uid(), role: 'user', content: text.trim(), timestamp: Date.now(),
    };
    setMessages(prev => [...prev, userMsg]);

    // create placeholder assistant message
    const assistantId = uid();
    setMessages(prev => [...prev, {
      id: assistantId, role: 'assistant', content: '', tools: [], timestamp: Date.now(),
    }]);

    setPhase('thinking');
    setInput('');

    const controller = new AbortController();
    abortRef.current = controller;

    let accumulatingText = '';
    const toolMap = new Map<string, ToolCallInfo>();

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        switch (ev.type) {
          case 'text_delta':
            accumulatingText += ev.content || '';
            setPhase('streaming');
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accumulatingText } : m
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
            const existing = toolMap.get(name);
            if (existing) {
              existing.result = ev.result;
              existing.status = 'done';
            } else {
              toolMap.set(name, {
                name, args: '', result: ev.result, status: 'done',
              });
            }
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, tools: Array.from(toolMap.values()) } : m
            ));
            break;
          }

          case 'iteration_end':
            // bump context estimate
            setContextPct(Math.min(99, accumulatingText.length > 0 ? 15 + Math.floor(accumulatingText.length / 200) : 15));
            break;

          case 'checkpoint':
          case 'consolidation':
          case 'rebuild':
            // could show inline notification
            break;

          case 'done':
            break;

          case 'cancelled':
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accumulatingText + '\n[已取消]' } : m
            ));
            break;

          case 'error':
            setError(ev.message || '未知错误');
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, content: accumulatingText + `\n⚠ ${ev.message}` } : m
            ));
            break;
        }
      }, controller.signal);
    } catch (e) {
      if ((e as Error).name === 'AbortError') {
        // user cancelled — already handled
      } else {
        const errMsg = String(e);
        setError(errMsg);
        setMessages(prev => prev.map(m =>
          m.id === assistantId ? { ...m, content: accumulatingText + `\n⚠ ${errMsg}` } : m
        ));
      }
    } finally {
      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId]);

  // ── keyboard ──
  useInput((inputChar, key) => {
    // Ctrl+C: cancel streaming or exit
    if (key.ctrl && inputChar === 'c') {
      if (abortRef.current) {
        abortRef.current.abort();
        if (sessionId) cancelTurn(sessionId).catch(() => {});
        return;
      }
      exit();
    }
    // Enter handled by TextInput onSubmit
  });

  // ── error state ──
  if (phase === 'error') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text bold color="red">✗ 连接失败</Text>
        <Text color="red">{error}</Text>
        <Text dimColor>请确认 luwu-server 正在运行: cargo run -p luwu-server</Text>
        <Text dimColor>按任意键退出…</Text>
      </Box>
    );
  }

  // ── connecting state ──
  if (phase === 'connecting') {
    return (
      <Box flexDirection="column" padding={1}>
        <Text><Text color="cyan"><Spinner type="dots" /></Text> 正在连接 luwu-server…</Text>
      </Box>
    );
  }

  // ── main UI ──
  const thinking = phase === 'thinking' || phase === 'streaming';

  return (
    <Box flexDirection="column">
      {/* message history — rendered statically (scrollback) */}
      <Box flexDirection="column">
        {messages.map(m => (
          <MessageItem key={m.id} message={m} />
        ))}
        {/* thinking spinner */}
        {phase === 'thinking' && (
          <Box marginLeft={2}>
            <Text color="cyan"><Spinner type="dots" /></Text>
            <Text dimColor> 思考中…</Text>
          </Box>
        )}
        {/* streaming cursor */}
        {phase === 'streaming' && (
          <Box marginLeft={2}>
            <Text color="cyan">▎</Text>
          </Box>
        )}
      </Box>

      {/* blank line */}
      <Text>{' '}</Text>

      {/* input area */}
      <Box>
        <Text bold color="green">❯ </Text>
        <CustomInput
          value={input}
          onChange={setInput}
          onSubmit={sendMessage}
          placeholder={thinking ? '正在回复… (Ctrl+C 取消)' : '输入消息，Enter 发送'}
        />
      </Box>

      {/* status bar */}
      <StatusBar
        model={model}
        cwd={getCwd()}
        gitBranch={gitBranch}
        contextPct={contextPct}
        sessionCount={0}
        thinking={thinking}
      />
    </Box>
  );
}
