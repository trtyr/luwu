// hooks/useChatSession.ts — chat state + SSE stream processing
import { useState, useCallback, useRef, useEffect } from 'react';
import type {
  DisplayMessage, ToolCallInfo, AssistantBlock, Phase, StreamEvent,
} from '../core/types.js';
import {
  checkHealth, createSession, streamChat, cancelTurn, getModels,
} from '../services/api.js';
import { contextWindowFor } from '../core/constants.js';

let msgCounter = 0;
const uid = (): string => `m-${Date.now()}-${msgCounter++}`;

function getGitBranchSync(): string | null {
  try {
    const r = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD']);
    if (r.exitCode !== 0) return null;
    return new TextDecoder().decode(r.stdout).trim() || null;
  } catch { return null; }
}

export interface ChatSession {
  messages: DisplayMessage[];
  phase: Phase;
  sessionId: string | null;
  error: string | null;
  model: string;
  gitBranch: string | null;
  contextPct: number;
  contextTokens: number;
  iteration: number;
  spinnerVerb: string | undefined;
  lastActivityRef: React.MutableRefObject<number>;
  setMessages: React.Dispatch<React.SetStateAction<DisplayMessage[]>>;
  setModel: (m: string) => void;
  sendMessage: (text: string) => Promise<void>;
  cancel: () => void;
  restoreSession: (id: string) => void;
  newSession: () => Promise<void>;
  abortRef: React.MutableRefObject<AbortController | null>;
}

export function useChatSession(): ChatSession {
  const [messages, setMessages] = useState<DisplayMessage[]>([]);
  const [phase, setPhase] = useState<Phase>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState('glm-4.7');
  const [gitBranch, setGitBranch] = useState<string | null>(null);
  const [contextPct, setContextPct] = useState(0);
  const [contextTokens, setContextTokens] = useState(0);
  const [iteration, setIteration] = useState(0);
  const [spinnerVerb, setSpinnerVerb] = useState<string | undefined>(undefined);
  const abortRef = useRef<AbortController | null>(null);
  const lastActivityRef = useRef(Date.now());

  const updateContext = useCallback((promptTokens: number, currentModel: string) => {
    const max = contextWindowFor(currentModel);
    setContextTokens(promptTokens);
    setContextPct(Math.min(100, Math.round((promptTokens / max) * 100)));
  }, []);

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

  // ── Heartbeat: ping every 10s so daemon knows we're alive ──
  useEffect(() => {
    const timer = setInterval(() => {
      checkHealth().catch(() => {});
    }, 10_000);
    return () => clearInterval(timer);
  }, []);

  const cancel = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
  }, [sessionId]);

  const restoreSession = useCallback((id: string) => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setSessionId(id);
    setContextPct(0); setContextTokens(0); setIteration(0); setSpinnerVerb(undefined);
    setPhase('ready');
    setMessages([{
      id: uid(), role: 'system', timestamp: Date.now(),
      content: `已切换到 session ${id.slice(0, 8)}… · 服务器端保留完整对话历史`,
    }]);
  }, [sessionId]);

  const newSession = useCallback(async () => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setContextPct(0); setContextTokens(0); setIteration(0); setSpinnerVerb(undefined);
    setPhase('connecting');
    setMessages([{
      id: uid(), role: 'system', timestamp: Date.now(),
      content: '正在创建新会话…',
    }]);
    try {
      const newId = await createSession();
      setSessionId(newId);
      setPhase('ready');
      setMessages([{
        id: uid(), role: 'system', timestamp: Date.now(),
        content: `新会话 ${newId.slice(0, 8)}… 已创建 · 开始对话吧`,
      }]);
    } catch (e) {
      setPhase('error');
      setError(`创建新会话失败: ${String(e)}`);
    }
  }, [sessionId]);

  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    const userMsg: DisplayMessage = {
      id: uid(), role: 'user', content: text.trim(), timestamp: Date.now(),
    };
    const assistantId = uid();
    setMessages(prev => [...prev, userMsg, {
      id: assistantId, role: 'assistant', content: '', blocks: [], timestamp: Date.now(),
    }]);

    setPhase('thinking');
    setIteration(0);
    setSpinnerVerb(undefined);
    lastActivityRef.current = Date.now();

    const controller = new AbortController();
    abortRef.current = controller;

    // ── Stream state ──
    const blocks: AssistantBlock[] = [];
    let currentText = '';
    let accReasoning = '';
    const toolIndexMap = new Map<string, number>();

    // ── Throttle: batch React state updates to prevent flickering ──
    const SYNC_MS = 60;
    let lastSyncTime = 0;
    let syncTimer: ReturnType<typeof setTimeout> | null = null;

    const flushText = () => {
      if (currentText.trim().length === 0) { currentText = ''; return; }
      const last = blocks[blocks.length - 1];
      if (last && last.type === 'text') {
        last.text = currentText;
      } else {
        blocks.push({ type: 'text', text: currentText });
      }
    };

    // Core state mutation — updates both blocks and reasoning in one shot
    const syncToReact = () => {
      const textContent = blocks
        .filter(b => b.type === 'text')
        .map(b => (b as { type: 'text'; text: string }).text)
        .join('\n\n');
      const reasoning = accReasoning;
      setMessages(prev => prev.map(m =>
        m.id === assistantId
          ? { ...m, blocks: blocks.map(b => ({ ...b })), content: textContent, reasoning }
          : m
      ));
    };

    const throttledSync = () => {
      const now = Date.now();
      const elapsed = now - lastSyncTime;
      if (elapsed >= SYNC_MS) {
        lastSyncTime = now;
        syncToReact();
      } else if (!syncTimer) {
        syncTimer = setTimeout(() => {
          syncTimer = null;
          lastSyncTime = Date.now();
          syncToReact();
        }, SYNC_MS - elapsed);
      }
    };

    // Force immediate sync (for tool events, done, cancel, error)
    const immediateSync = () => {
      if (syncTimer) { clearTimeout(syncTimer); syncTimer = null; }
      lastSyncTime = Date.now();
      syncToReact();
    };

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        lastActivityRef.current = Date.now();

        switch (ev.type) {
          case 'text_delta':
            currentText += ev.delta || '';
            setPhase('streaming');
            flushText();
            throttledSync();
            break;

          case 'reasoning_delta':
            accReasoning += ev.delta || '';
            throttledSync();
            break;

          case 'tool_call': {
            flushText();
            currentText = '';
            setSpinnerVerb(ev.name || ev.tool_name || 'tool');
            const name = ev.name || ev.tool_name || 'unknown';
            const args = ev.arguments
              ? (typeof ev.arguments === 'string' ? ev.arguments : JSON.stringify(ev.arguments))
              : '';
            const toolInfo: ToolCallInfo = { name, args, status: 'running' };
            blocks.push({ type: 'tool', tool: toolInfo });
            toolIndexMap.set(name, blocks.length - 1);
            immediateSync();
            break;
          }

          case 'tool_completed': {
            const name = ev.name || ev.tool_name || '';
            const rawResult = ev.result || ev.output || '';
            const lower = rawResult.toLowerCase();
            const isErr = lower.includes('error')
              || lower.includes('panicked')
              || lower.includes('no such file')
              || lower.includes('not found')
              || lower.includes('failed')
              || lower.includes('permission denied');
            const idx = toolIndexMap.get(name);
            if (idx !== undefined && blocks[idx]?.type === 'tool') {
              blocks[idx].tool.result = rawResult;
              blocks[idx].tool.status = isErr ? 'error' : 'done';
            } else {
              flushText();
              currentText = '';
              blocks.push({
                type: 'tool',
                tool: {
                  name, args: '', result: rawResult,
                  status: (isErr ? 'error' : 'done') as 'error' | 'done',
                },
              });
              toolIndexMap.set(name, blocks.length - 1);
            }
            immediateSync();
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration || 0);
            if (ev.usage?.prompt_tokens) updateContext(ev.usage.prompt_tokens, model);
            break;

          case 'done':
            immediateSync();
            if (ev.usage?.prompt_tokens) updateContext(ev.usage.prompt_tokens, model);
            break;

          case 'cancelled':
            immediateSync();
            break;

          case 'error':
            currentText += `\n⚠ ${ev.message || 'Unknown error'}`;
            flushText();
            immediateSync();
            break;
        }
      }, controller.signal);
    } catch (e) {
      if (syncTimer) clearTimeout(syncTimer);
      if ((e as Error).name !== 'AbortError') {
        currentText += `\n⚠ ${String(e)}`;
        flushText();
        syncToReact();
      } else {
        syncToReact();
      }
    } finally {
      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId, model, updateContext]);

  return {
    messages, phase, sessionId, error, model, gitBranch,
    contextPct, contextTokens, iteration, spinnerVerb,
    lastActivityRef,
    setMessages, setModel, sendMessage, cancel, restoreSession, newSession, abortRef,
  };
}
