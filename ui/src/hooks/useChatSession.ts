// hooks/useChatSession.ts — chat state + SSE stream processing
//
// ARCHITECTURE: Two-tier message state for flicker-free rendering.
//   committedMessages → <Static> (written to terminal once, enters scrollback)
//   streamingMessage  → dynamic area (re-rendered every frame, always small)
//
// PROGRESSIVE COMMIT: When tool calls complete, their blocks are immediately
// committed to <Static> as partial assistant messages. This keeps the dynamic
// area bounded (~10-15 lines: current text + active tool + spinner + input).
// When a turn completes, the remaining streaming blocks are committed.

import { useState, useCallback, useRef, useEffect } from 'react';
import type {
  DisplayMessage, ToolCallInfo, AssistantBlock, Phase, StreamEvent,
} from '../core/types.js';
import {
  checkHealth, createSession, streamChat, cancelTurn, getModels,
} from '../services/api.js';
import { contextWindowFor, estimateCost } from '../core/constants.js';
import { useConnection } from './useConnection.js';

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
  committedMessages: DisplayMessage[];
  streamingMessage: DisplayMessage | null;
  staticKey: number;
  phase: Phase;
  sessionId: string | null;
  error: string | null;
  model: string;
  gitBranch: string | null;
  contextPct: number;
  contextTokens: number;
  /// Running total of prefix-cache hits across the session. Surfaces in
  /// the status bar as "⚡ XX% cached" when prompt_tokens > 0.
  cacheHit: number;
  iteration: number;
  spinnerVerb: string | undefined;
  connected: boolean;
  lastActivityRef: React.MutableRefObject<number>;
  setModel: (m: string) => void;
  sendMessage: (text: string) => Promise<void>;
  cancel: () => void;
  restoreSession: (id: string) => void;
  newSession: () => Promise<void>;
  clearMessages: () => void;
  abortRef: React.MutableRefObject<AbortController | null>;
}

function sysMsg(text: string): DisplayMessage {
  return { id: uid(), role: 'system', content: text, timestamp: Date.now() };
}

export function useChatSession(): ChatSession {
  const [committedMessages, setCommittedMessages] = useState<DisplayMessage[]>([]);
  const [streamingMessage, setStreamingMessage] = useState<DisplayMessage | null>(null);
  const [staticKey, setStaticKey] = useState(0);
  const [phase, setPhase] = useState<Phase>('connecting');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState('glm-4.7');
  const [contextPct, setContextPct] = useState(0);
  const [contextTokens, setContextTokens] = useState(0);
  const [cacheHit, setCacheHit] = useState(0);
  const [costTotal, setCostTotal] = useState(0);
  const [costSaved, setCostSaved] = useState(0);
  const [iteration, setIteration] = useState(0);
  const [spinnerVerb, setSpinnerVerb] = useState<string | undefined>(undefined);

  // Connection management extracted to useConnection (heartbeat + git branch).
  const { connected, gitBranch } = useConnection();
  const abortRef = useRef<AbortController | null>(null);
  const lastActivityRef = useRef(Date.now());
  const streamingRef = useRef<DisplayMessage | null>(null);

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
        try {
          const models = await getModels();
          if (models.length > 0 && models[0].id) setModel(models[0].id);
        } catch { /* default */ }
        setPhase('ready');
        setCommittedMessages([sysMsg(
          '陆吾 v0.1.0 — 输入消息开始对话 · ↑↓ 浏览历史 · / 查看命令'
        )]);
      } catch (e) {
        setError(String(e));
        setPhase('error');
      }
    })();
  }, []);

  const cancel = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
  }, [sessionId]);

  const clearMessages = useCallback(() => {
    streamingRef.current = null;
    setStreamingMessage(null);
    setCommittedMessages([]);
    setStaticKey(k => k + 1);
  }, []);

  const restoreSession = useCallback((id: string) => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setSessionId(id);
    setContextPct(0); setContextTokens(0); setIteration(0); setSpinnerVerb(undefined);
    setPhase('ready');
    streamingRef.current = null;
    setStreamingMessage(null);
    setCommittedMessages([sysMsg(
      `已切换到 session ${id.slice(0, 8)}… · 服务器端保留完整对话历史`
    )]);
    setStaticKey(k => k + 1);
  }, [sessionId]);

  const newSession = useCallback(async () => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setContextPct(0); setContextTokens(0); setIteration(0); setSpinnerVerb(undefined);
    setPhase('connecting');
    streamingRef.current = null;
    setStreamingMessage(null);
    setCommittedMessages([sysMsg('正在创建新会话…')]);
    setStaticKey(k => k + 1);
    try {
      const newId = await createSession();
      setSessionId(newId);
      setPhase('ready');
      setCommittedMessages([sysMsg(`新会话 ${newId.slice(0, 8)}… 已创建 · 开始对话吧`)]);
      setStaticKey(k => k + 1);
    } catch (e) {
      setPhase('error');
      setError(`创建新会话失败: ${String(e)}`);
    }
  }, [sessionId]);

  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    // Commit user message immediately to <Static>
    const userMsg: DisplayMessage = {
      id: uid(), role: 'user', content: text.trim(), timestamp: Date.now(),
    };
    setCommittedMessages(prev => [...prev, userMsg]);

    // Start streaming assistant message in dynamic area
    const assistantId = uid();
    const assistantMsg: DisplayMessage = {
      id: assistantId, role: 'assistant', content: '', blocks: [], timestamp: Date.now(),
    };
    streamingRef.current = assistantMsg;
    setStreamingMessage(assistantMsg);

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

    // ── Progressive commit: completed blocks go to <Static> immediately ──
    // This keeps the dynamic area small even for long responses.
    let committedBlockCount = 0;

    /** Commit blocks[0..endIdx] to <Static> as a partial assistant message */
    const commitBlocksToStatic = (endIdx: number) => {
      if (endIdx <= committedBlockCount) return;
      const toCommit = blocks.slice(committedBlockCount, endIdx).map(b => ({ ...b }));
      if (toCommit.length === 0) return;

      const textContent = toCommit
        .filter(b => b.type === 'text')
        .map(b => (b as { type: 'text'; text: string }).text)
        .join('\n\n');

      const partialMsg: DisplayMessage = {
        id: uid(),
        role: 'assistant',
        content: textContent,
        blocks: toCommit,
        timestamp: Date.now(),
      };
      setCommittedMessages(prev => [...prev, partialMsg]);
      committedBlockCount = endIdx;
    };

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

    // Update streamingRef + streamingMessage — only show blocks AFTER committed
    const syncToReact = () => {
      // Streaming message only contains blocks that haven't been committed yet
      const liveBlocks = blocks.slice(committedBlockCount).map(b => ({ ...b }));
      const textContent = liveBlocks
        .filter(b => b.type === 'text')
        .map(b => (b as { type: 'text'; text: string }).text)
        .join('\n\n');
      const reasoning = accReasoning;
      if (streamingRef.current) {
        const updated: DisplayMessage = {
          ...streamingRef.current,
          blocks: liveBlocks,
          content: textContent,
          reasoning: reasoning || undefined,
        };
        streamingRef.current = updated;
        setStreamingMessage(updated);
      }
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

            // ── Progressive commit: this tool is done, commit everything up to it ──
            const commitEnd = (idx !== undefined ? idx + 1 : blocks.length);
            commitBlocksToStatic(commitEnd);
            immediateSync();
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration || 0);
            if (ev.usage?.prompt_tokens) updateContext(ev.usage.prompt_tokens, model);
            if (ev.usage?.prompt_cache_hit_tokens) setCacheHit(ev.usage.prompt_cache_hit_tokens);
            if (ev.usage) {
              const est = estimateCost(ev.usage, model);
              setCostTotal(prev => prev + est.effective);
              setCostSaved(prev => prev + est.saved);
            }
            break;

          case 'done':
            immediateSync();
            if (ev.usage?.prompt_tokens) updateContext(ev.usage.prompt_tokens, model);
            if (ev.usage?.prompt_cache_hit_tokens) setCacheHit(ev.usage.prompt_cache_hit_tokens);
            if (ev.usage) {
              const est = estimateCost(ev.usage, model);
              setCostTotal(prev => prev + est.effective);
              setCostSaved(prev => prev + est.saved);
            }
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
      // ── Commit remaining streaming blocks to <Static> ──
      if (blocks.length > committedBlockCount) {
        commitBlocksToStatic(blocks.length);
      }
      streamingRef.current = null;
      setStreamingMessage(null);

      abortRef.current = null;
      setPhase('ready');
    }
  }, [sessionId, model, updateContext]);

  return {
    committedMessages, streamingMessage, staticKey,
    phase, sessionId, error, model, gitBranch,
    contextPct, contextTokens, cacheHit, costTotal, costSaved,
    iteration, spinnerVerb, connected,
    lastActivityRef,
    setModel, sendMessage, cancel, restoreSession, newSession, clearMessages, abortRef,
  };
}
