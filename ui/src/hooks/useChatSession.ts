// hooks/useChatSession.ts — chat state + SSE stream processing
//
// ARCHITECTURE: Two-tier message state for flicker-free rendering.
//   committedMessages → <Static> (written to terminal once, enters scrollback)
//   streamingMessage  → dynamic area (re-rendered every frame, always small)
//
// PROGRESSIVE COMMIT (two triggers):
//   1. tool_completed → commitBlocksToStatic(commitEnd) moves tool blocks
//      to <Static> immediately
//   2. text_delta where last text block exceeds MAX_DYNAMIC_LINES or
//      MAX_DYNAMIC_CHARS → commitTextOverflow() splits: early part goes
//      to <Static> (enters scrollback), recent part stays in dynamic area.
//      This is the CRITICAL fix for Ink's clearTerminal nuclear option —
//      when the dynamic area exceeds terminal rows, Ink erases the whole
//      screen and rewrites from the top, causing flicker + scroll-jump.
//   3. done event → final commitBlocksToStatic + setStreamingMessage(null)
//      + setPhase('ready') all batched in the same microtask, so React
//      renders "new static + empty dynamic" atomically (no intermediate
//      "dynamic shrinks but no new static" flash).
//
// DUPLICATION BUG FIX (commitTextOverflow + currentText trimming):
//   `currentText` is the SSE delta accumulator. After commitTextOverflow
//   splits a text block, `currentText` MUST be trimmed to the recent part.
//   Otherwise the next flushText() will write the FULL currentText
//   (including the already-committed early part) back into the recent
//   block, and the NEXT commitTextOverflow() will re-commit the SAME
//   early part to <Static> — causing the same content to appear 2-3
//   times in scrollback. This was the root cause of the "AI reply shown
//   multiple times" bug.
//
// CHAR OFFSET BUG FIX (commitTextOverflow line→char conversion):
//   The splitAt for line-based splitting must be a CHARACTER OFFSET for
//   text.slice(), not a line index. For a 20-line text, lines.length - 15 = 5
//   is "5 lines from the start", but text.slice(0, 5) only takes the first
//   5 CHARACTERS. Fix: sum lines[i].length + 1 for each line skipped to
//   get the actual char offset where the "recent" part begins.

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

    // FIX 3 (review): Set thinking state IMMEDIATELY after committing
    // the user message so the UI shows "thinking" spinner before the
    // first text_delta arrives. Previously these were missing, leaving
    // the user staring at a "ready" state (no spinner) from message
    // send until the first byte of AI output — could be several seconds
    // of confusion ("did it receive my message?").
    setPhase('thinking');
    setIteration(0);
    setSpinnerVerb(undefined);

    const controller = new AbortController();
    abortRef.current = controller;

    /** Commit blocks[committedBlockCount..endIdx] to <Static> as a partial
     *  assistant message. The committed text enters terminal scrollback. */
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

    // FIX 1 + DUPLICATION FIX + CHAR OFFSET FIX: Progressive text overflow.
    // ────────────────────────────────────────────────────────────────────
    // When a text block grows beyond MAX_DYNAMIC_LINES (or has more than
    // MAX_DYNAMIC_CHARS in a single line), split it: the early part is
    // committed to <Static> (enters scrollback permanently), the recent
    // part stays in the dynamic area for continued streaming.
    //
    // CRITICAL: splitAt must be a CHARACTER OFFSET (for text.slice()),
    // NOT a line index. For line-based splitting, we sum lines[i].length + 1
    // for each skipped line to get the actual char position where the
    // "recent" part begins. The old code used `lines.length - MAX` as the
    // offset, which only worked if every line was 1 character long.
    //
    // DUPLICATION BUG FIX: After split, `currentText` MUST be trimmed
    // to the recent part. The SSE delta accumulator `currentText` still
    // holds the full text, so the next flushText() will overwrite the
    // recent block with the full text (including the just-committed
    // early part). Without trimming, every new text_delta would
    // resurrect the committed content in the recent block, and the
    // next split would re-commit it to <Static> — causing the AI reply
    // to appear 2-3 times in scrollback.
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

      // Decide where to split (or skip if not overflow).
      // Two overflow triggers:
      //   (a) many lines → split by line count, keep last MAX_DYNAMIC_LINES
      //   (b) single long line → split at last \n before char budget
      let splitAt: number;
      if (lines.length > MAX_DYNAMIC_LINES) {
        // ── LINE-BASED SPLIT (must be char offset, not line index!) ──
        // For "lines.length - MAX_DYNAMIC_LINES" we want to keep the last
        // MAX lines. We need the CHAR OFFSET where those lines start.
        // Sum each skipped line's length + 1 (for the \n separator).
        // Example: "a\nbb\nccc\ndddd" split by \n = ["a","bb","ccc","dddd"]
        //   lines.length=4, keep last 2 lines → skip 2 lines → 1+1+2+1=5
        //   text.slice(0, 5) = "a\nbb\n" ← correct early part
        //   text.slice(5) = "ccc\ndddd" ← correct recent part
        const skipLines = lines.length - MAX_DYNAMIC_LINES;
        let charOffset = 0;
        for (let i = 0; i < skipLines; i++) {
          charOffset += lines[i].length + 1; // +1 for the \n separator
        }
        splitAt = charOffset;
      } else if (lastText.text.length > MAX_DYNAMIC_CHARS) {
        // Single-line overflow: find the last \n before the char budget
        // so we don't cut a word in half. Fallback to char-budget if no \n.
        const charBudget = lastText.text.length - MAX_DYNAMIC_CHARS;
        let lastNewline = -1;
        for (let i = 0; i < charBudget && i < lastText.text.length; i++) {
          if (lastText.text[i] === '\n') lastNewline = i;
        }
        splitAt = lastNewline >= 0 ? lastNewline + 1 : charBudget;
      } else {
        return; // not overflow yet
      }

      if (splitAt <= 0 || splitAt >= lastText.text.length) return;

      const earlyText = lastText.text.slice(0, splitAt);
      const recentText = lastText.text.slice(splitAt);

      if (earlyText.length === 0 || recentText.length === 0) return;

      // Replace the last text block with the recent part
      blocks[lastTextIdx] = { type: 'text', text: recentText };

      // Insert a new block for the early part BEFORE the recent part
      blocks.splice(lastTextIdx, 0, { type: 'text', text: earlyText } as AssistantBlock);

      // Commit blocks[committedBlockCount..lastTextIdx+1] to <Static>.
      // After this, committedBlockCount = lastTextIdx + 1, and the
      // uncommitted region starts at lastTextIdx + 1 (the recent text).
      commitBlocksToStatic(lastTextIdx + 1);

      // ── CRITICAL: trim currentText to the recent part ──
      // Without this, the next flushText() will put the FULL currentText
      // (including the already-committed early part) back into the
      // recent block, and the next commitTextOverflow() will re-commit
      // the SAME early part to <Static> — causing the same content to
      // appear 2-3 times in scrollback.
      currentText = recentText;
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
            // FIX 4 (review): Use throttledSync (not syncToReact) — reasoning
            // can be high-frequency (few KB per response), and direct
            // syncToReact would cause dozens of React re-renders per
            // second. 60ms throttle is invisible to the user but saves
            // significant work.
            accReasoning += ev.delta || '';
            throttledSync();
            break;

          case 'tool_call': {
            // Flush any accumulated text before starting a new tool
            flushText();
            currentText = '';
            // StreamEvent carries tool identity as flat fields (name/tool_name
            // + arguments/args) — NOT a nested tool_call object. The
            // AssistantBlock tool variant wraps everything in `tool: ToolCallInfo`.
            const name: string = ev.name || ev.tool_name || 'unknown';
            const rawArgs = ev.arguments ?? ev.args;
            const args: string = typeof rawArgs === 'string'
              ? rawArgs
              : JSON.stringify(rawArgs ?? {});
            const toolInfo: ToolCallInfo = { name, args, status: 'running' };
            blocks.push({ type: 'tool', tool: toolInfo });
            // Key by tool name (the identifier the backend uses to pair
            // tool_call with tool_completed). If the same tool name
            // appears multiple times in one turn, the LAST one wins —
            // acceptable since the backend invokes each tool at most once
            // per iteration.
            toolIndexMap.set(name, blocks.length - 1);
            syncToReact();
            break;
          }

          case 'tool_completed': {
            // StreamEvent carries tool identity as flat fields (name/tool_name)
            // + result (result/output). The matching tool block in `blocks`
            // wraps the ToolCallInfo inside `.tool`, so we access
            // blocks[idx].tool.result / .tool.status.
            const name: string = ev.name || ev.tool_name || '';
            const idx = toolIndexMap.get(name);
            if (idx !== undefined && blocks[idx] && blocks[idx].type === 'tool') {
              const resultStr: string = ev.result || ev.output || '';
              // FIX 3 (review): Detect tool errors via keyword matching on
              // the result string. The StreamEvent type doesn't have an
              // `error` field, so we rely on substring detection. This
              // sets status='error' for red coloring instead of always
              // defaulting to 'done' green.
              const isError = /\b(error|panicked|no such file|not found|failed|permission denied)\b/i.test(resultStr);
              blocks[idx].tool.result = resultStr;
              blocks[idx].tool.status = isError ? 'error' : 'done';
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
