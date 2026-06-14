// hooks/useChatSession.ts — chat state + SSE stream processing
// Extracted from App.tsx so App becomes a pure composition layer.
// All business logic for connection, messaging, and stream processing lives here.
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
  // Read-only state
  messages: DisplayMessage[];
  phase: Phase;
  sessionId: string | null;
  error: string | null;
  model: string;
  gitBranch: string | null;
  contextPct: number;
  iteration: number;
  spinnerVerb: string | undefined;
  // Actions
  setMessages: React.Dispatch<React.SetStateAction<DisplayMessage[]>>;
  setModel: (m: string) => void;
  sendMessage: (text: string) => Promise<void>;
  cancel: () => void;
  restoreSession: (id: string) => void;
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
  const [iteration, setIteration] = useState(0);
  const [spinnerVerb, setSpinnerVerb] = useState<string | undefined>(undefined);
  const abortRef = useRef<AbortController | null>(null);

  // ── Init: health check → create session → get models ──
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

  // ── Cancel current request ──
  const cancel = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
  }, [sessionId]);

  // ── Restore/switch to an existing session ──
  const restoreSession = useCallback((id: string) => {
    // Cancel any in-flight request
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setSessionId(id);
    setContextPct(0);
    setIteration(0);
    setSpinnerVerb(undefined);
    setPhase('ready');
    setMessages([{
      id: uid(), role: 'system', timestamp: Date.now(),
      content: `已切换到 session ${id.slice(0, 8)}… · 服务器端保留完整对话历史`,
    }]);
  }, [sessionId]);

  // ── Send message: builds blocks[] in chronological order ──
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

    const controller = new AbortController();
    abortRef.current = controller;

    // Stream state — blocks[] built in chronological order
    const blocks: AssistantBlock[] = [];
    let currentText = '';
    let accReasoning = '';
    const toolIndexMap = new Map<string, number>();

    const flushText = () => {
      if (currentText.length === 0) return;
      const last = blocks[blocks.length - 1];
      if (last && last.type === 'text') {
        last.text = currentText;
      } else {
        blocks.push({ type: 'text', text: currentText });
      }
    };

    const syncBlocks = () => {
      const textContent = blocks
        .filter(b => b.type === 'text')
        .map(b => (b as { type: 'text'; text: string }).text)
        .join('\n\n');
      setMessages(prev => prev.map(m =>
        m.id === assistantId
          ? {
            ...m,
            blocks: blocks.map(b => ({ ...b })),
            content: textContent,
          }
          : m
      ));
    };

    try {
      await streamChat(sessionId, text, (ev: StreamEvent) => {
        switch (ev.type) {
          case 'text_delta':
            currentText += ev.delta || '';
            setPhase('streaming');
            flushText();
            syncBlocks();
            break;

          case 'reasoning_delta':
            accReasoning += ev.delta || '';
            setMessages(prev => prev.map(m =>
              m.id === assistantId ? { ...m, reasoning: accReasoning } : m
            ));
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
              flushText();
              currentText = '';
              blocks.push({
                type: 'tool',
                tool: {
                  name, args: '',
                  result: ev.result || ev.output,
                  status: 'done' as const,
                },
              });
              toolIndexMap.set(name, blocks.length - 1);
            }
            syncBlocks();
            break;
          }

          case 'iteration_end': {
            setIteration(ev.iteration || 0);
            // Real context % from LLM usage data.
            if (ev.usage?.prompt_tokens) {
              const max = contextWindowFor(model);
              setContextPct(Math.min(100, Math.round((ev.usage.prompt_tokens / max) * 100)));
            }
            break;
          }

          case 'done': {
            // Also update context % from final usage.
            if (ev.usage?.prompt_tokens) {
              const max = contextWindowFor(model);
              setContextPct(Math.min(100, Math.round((ev.usage.prompt_tokens / max) * 100)));
            }
            break;
          }
          case 'cancelled': syncBlocks(); break;
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

  return {
    messages, phase, sessionId, error, model, gitBranch,
    contextPct, iteration, spinnerVerb,
    setMessages, setModel, sendMessage, cancel, restoreSession, abortRef,
  };
}
