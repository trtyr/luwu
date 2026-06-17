// hooks/useChatSession.ts — chat state + SSE stream processing
//
// ARCHITECTURE: Two-tier message state for flicker-free rendering.
//   committedMessages → <Static> (written to terminal once, enters scrollback)
//   streamingMessage  → dynamic area (re-rendered every frame, always small)
//
// PROGRESSIVE COMMIT (two triggers):
//   1. tool_completed → commitBlocksToStatic(commitEnd) moves tool blocks
//      to <Static> immediately
//   2. text_delta where last text block exceeds MAX_DYNAMIC_LINES →
//      commitTextOverflow() splits: early lines → <Static>, recent N lines
//      stay in dynamic area. This is the CRITICAL fix for Ink's
//      clearTerminal nuclear option — when the dynamic area exceeds
//      terminal rows, Ink erases the whole screen and rewrites from the
//      top, causing flicker + scroll-jump. Long text without tool calls
//      (e.g. a 60-line explanation) previously caused this.
//   3. done event → final commitBlocksToStatic + setStreamingMessage(null)
//      + setPhase('ready') all batched in the same microtask, so React
//      renders "new static + empty dynamic" atomically (no intermediate
//      "dynamic shrinks but no new static" flash).

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
  /// Cumulative effective cost (USD) across the session — what the user
  /// actually paid, with cache hits at the discounted rate. Reset on
  /// newSession/restoreSession/clearMessages.
  costTotal: number;
  /// Cumulative cost saved by prefix caching (USD). raw - effective.
  /// Reset alongside costTotal.
  costSaved: number;
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
    // Reset all per-turn cost/cache counters so a /clear doesn't keep
    // showing the previous conversation's cost in the status bar.
    setCacheHit(0);
    setCostTotal(0);
    setCostSaved(0);
  }, []);

  const restoreSession = useCallback((id: string) => {
    if (abortRef.current) {
      abortRef.current.abort();
      if (sessionId) cancelTurn(sessionId).catch(() => {});
    }
    setSessionId(id);
    setContextPct(0); setContextTokens(0); setIteration(0); setSpinnerVerb(undefined);
    setCacheHit(0); setCostTotal(0); setCostSaved(0);
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
    setCacheHit(0); setCostTotal(0); setCostSaved(0);
    setPhase('connecting');
    streamingRef.current = null;
    setStreamingMessage(null);
    // FIX 3: Do NOT pre-clear messages + setStaticKey before createSession
    // succeeds. The old code cleared messages, showed a placeholder
    // "正在创建新会话…", and bumped staticKey — then if createSession
    // failed, the user lost their old messages AND saw a double
    // <Static> remount. Now we keep the old messages visible until the
    // new session is confirmed, then do a SINGLE setStaticKey remount
    // with the new system message.
    try {
      const newId = await createSession();
      setSessionId(newId);
      setPhase('ready');
      setCommittedMessages([sysMsg(`新会话 ${newId.slice(0, 8)}… 已创建 · 开始对话吧`)]);
      setStaticKey(k => k + 1);  // SINGLE remount, only on success
    } catch (e) {
      setPhase('error');
      setError(`创建新会话失败: ${String(e)}`);
    }
  }, [sessionId]);

  const sendMessage = useCallback(async (text: string) => {
    if (!sessionId || !text.trim()) return;

    // Commit user message immediately to <Static>
    const userMsg: DisplayMessage = {
      id: uid(),
      role: 'user',
      content: text,
      timestamp: Date.now(),
    };
    setCommittedMessages(prev => [...prev, userMsg]);

    // Fresh state for this turn
    const blocks: AssistantBlock[] = [];
    let currentText = '';
    let accReasoning = '';
    const toolIndexMap = new Map<string, number>();
    let committedBlockCount = 0;
    let receivedDone = false;  // FIX 2: track whether 'done' event fired

    // Create streaming assistant message
    const assistantMsg: DisplayMessage = {
      id: uid(),
      role: 'assistant',
      content: '',
      blocks: [],
      timestamp: Date.now(),
    };
    streamingRef.current = assistantMsg;
    setStreamingMessage(assistantMsg);

    const controller = new AbortController();
    abortRef.current = controller;

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

    // FIX 1: Progressive text overflow commit.
    // ────────────────────────────────────────────────────────────────────
    // When a text block grows beyond MAX_DYNAMIC_LINES (or has too many
    // characters for one line), split it: the early part is committed to
    // <Static> (enters scrollback permanently), the recent MAX_DYNAMIC_LINES
    // stay in the dynamic area for continued streaming.
    //
    // This is the CRITICAL fix for Ink's clearTerminal nuclear option:
    // when the dynamic area exceeds terminal rows, Ink erases the whole
    // screen and rewrites from the top, causing flicker + scroll-jump.
    // Long AI explanations (e.g. 60 lines of Markdown) without any tool
    // calls previously caused this. Now the dynamic area stays bounded
    // at MAX_DYNAMIC_LINES regardless of total response length.
    // ────────────────────────────────────────────────────────────────────
    const MAX_DYNAMIC_LINES = 15;
    const MAX_DYNAMIC_CHARS = 2000;
    const commitTextOverflow = () => {
      // Find the last text block in the uncommitted region
      let lastTextIdx = -1;
      for (let i = blocks.length - 1; i >= committedBlockCount; i--) {
        if (blocks[i].type === 'text') {
          lastTextIdx = i;
          break;
        }
      }
      if (lastTextIdx < 0) return;

      const lastText = blocks[lastTextIdx] as { type: 'text'; text: string };
      const lines = lastText.text.split('\n');

      if (lines.length <= MAX_DYNAMIC_LINES && lastText.text.length <= MAX_DYNAMIC_CHARS) {
        return; // not overflow yet
      }

      // Split: keep last MAX_DYNAMIC_LINES lines in dynamic area,
      // commit everything before that to <Static>.
      const splitAt = Math.max(0, lines.length - MAX_DYNAMIC_LINES);
      const earlyLines = lines.slice(0, splitAt);
      const recentLines = lines.slice(splitAt);

      if (earlyLines.length === 0) return; // nothing to commit

      const earlyText = earlyLines.join('\n');
      const recentText = recentLines.join('\n');

      // Replace the last text block with the recent part
      blocks[lastTextIdx] = { type: 'text', text: recentText };

      // Insert a new block for the early part BEFORE the recent part
      const earlyBlock: AssistantBlock = { type: 'text', text: earlyText };
      blocks.splice(lastTextIdx, 0, earlyBlock);

      // Commit blocks[committedBlockCount..lastTextIdx+1] to <Static>.
      // After this, committedBlockCount = lastTextIdx + 1, and the
      // uncommitted region starts at lastTextIdx + 1 (the recent text).
      commitBlocksToStatic(lastTextIdx + 1);
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
      // FIX 1: After appending, check if the text block is too long.
      // If so, split it and commit the early part to <Static>.
      commitTextOverflow();
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
            syncToReact();
            break;

          case 'tool_call': {
            // Flush any accumulated text before starting a new tool
            flushText();
            currentText = '';
            const tc = ev.tool_call;
            const toolBlock: AssistantBlock = {
              type: 'tool',
              id: tc.id,
              name: tc.name,
              arguments: tc.arguments,
              status: 'running',
            };
            blocks.push(toolBlock);
            toolIndexMap.set(tc.name + ':' + tc.id, blocks.length - 1);
            syncToReact();
            break;
          }

          case 'tool_completed': {
            // Update the matching tool block with the result
            const tcId = ev.tool_call_id;
            const idx = toolIndexMap.get(tcId || '');
            if (idx !== undefined && blocks[idx] && blocks[idx].type === 'tool') {
              const toolBlock = blocks[idx] as AssistantBlock & { type: 'tool' };
              // FIX 3 (review): Detect tool errors from ev.error flag
              // and result keyword matching — set status='error' for
              // red coloring instead of always 'done' green.
              const resultStr = ev.result || '';
              const isError = ev.error === true
                || /\b(error|panicked|no such file|not found|failed|permission denied)\b/i.test(resultStr);
              toolBlock.result = resultStr;
              toolBlock.status = isError ? 'error' : 'done';
            }
            // Commit blocks[0..idx+1] to <Static> as a partial assistant
            // message — the tool call + its result enter scrollback
            // immediately, so the dynamic area stays small.
            const commitEnd = (idx !== undefined ? idx : blocks.length - 1) + 1;
            if (commitEnd > committedBlockCount) {
              commitBlocksToStatic(commitEnd);
            }
            syncToReact();
            break;
          }

          case 'iteration_end':
            setIteration(ev.iteration);
            // Update cache hit from this iteration's usage
            if (ev.usage?.prompt_cache_hit_tokens) {
              setCacheHit(ev.usage.prompt_cache_hit_tokens);
            }
            // Accumulate cost from this iteration's usage
            if (ev.usage) {
              const est = estimateCost(ev.usage, model);
              setCostTotal(prev => prev + est.effective);
              setCostSaved(prev => prev + est.saved);
            }
            break;

          case 'done':
            // FIX 2: Batch all final state updates in the done handler.
            // ──────────────────────────────────────────────────────
            // Previously the cleanup (commitBlocksToStatic + setStreaming-
            // Message(null) + setPhase('ready')) was in the `finally`
            // block, which runs in a SEPARATE microtask from this
            // event handler. That caused 2 separate renders:
            //   1. done event → immediateSync() (dynamic area still has
            //      streamingMessage, dynamic area still high)
            //   2. finally   → clear streaming + commit (dynamic area
            //      suddenly drops, <Static> remounts)
            // Between these, the user saw a flicker / scroll-jump.
            //
            // By doing all cleanup HERE, React 18 batching puts every
            // setState into ONE render: the user sees the committed
            // message in scrollback AND an empty dynamic area
            // simultaneously. No flicker, no jump.
            // ──────────────────────────────────────────────────────
            receivedDone = true;
            // Clear the throttledSync timer first so a late text_delta
            // (network reordering) can't fire syncToReact 60ms after we
            // clear streamingMessage and resurrect the same content.
            if (syncTimer) { clearTimeout(syncTimer); syncTimer = null; }
            // Flush any remaining text into the last block
            flushText();
            currentText = '';
            // Update context/cache from final usage
            if (ev.usage?.prompt_tokens) updateContext(ev.usage.prompt_tokens, model);
            if (ev.usage?.prompt_cache_hit_tokens) setCacheHit(ev.usage.prompt_cache_hit_tokens);
            // NOTE: costTotal/costSaved are NOT accumulated here. The
            // backend's `done` event carries `total_usage` (cumulative
            // across all iterations), while `iteration_end` carries
            // `last_usage` (per-iteration). The TUI accumulates from
            // iteration_end only — summing per-iteration costs gives
            // the same total as the backend's cumulative done.usage.
            // Accumulating from both would double-count (e.g. for a
            // 3-iteration turn: 3*iteration_end + 1*done = 4x the real
            // cost). Context/cache fields are fine to overwrite since
            // they're "latest value", not "cumulative".
            // ──────────────────────────────────────────────────────
            // ATOMIC FINAL CLEANUP — all in one React render:
            //   1. Commit remaining blocks to <Static> (enters scrollback)
            //   2. Clear streamingMessage (dynamic area becomes empty)
            //   3. Set phase to ready (spinner disappears)
            //   4. Clear abortRef
            // ──────────────────────────────────────────────────────
            if (blocks.length > committedBlockCount) {
              commitBlocksToStatic(blocks.length);
            }
            streamingRef.current = null;
            setStreamingMessage(null);
            abortRef.current = null;
            setPhase('ready');
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
      if (syncTimer) { clearTimeout(syncTimer); syncTimer = null; }
      if ((e as Error).name !== 'AbortError') {
        currentText += `\n⚠ ${String(e)}`;
        flushText();
        syncToReact();
      } else {
        syncToReact();
      }
    } finally {
      // FIX 2: Safety net for error/cancel paths where 'done' was
      // NEVER received. The normal path handles cleanup in the
      // 'done' handler for batched atomic state updates. This branch
      // only runs when the SSE stream ended abnormally (network error,
      // AbortError from /cancel command, etc.) and we still need to
      // commit whatever blocks we have and clear the dynamic area.
      if (syncTimer) { clearTimeout(syncTimer); syncTimer = null; }
      if (!receivedDone) {
        if (blocks.length > committedBlockCount) {
          commitBlocksToStatic(blocks.length);
        }
        streamingRef.current = null;
        setStreamingMessage(null);
        abortRef.current = null;
        setPhase('ready');
      }
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
