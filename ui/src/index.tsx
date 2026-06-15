#!/usr/bin/env bun
// luwu TUI — entry point
//
// Anti-flicker architecture (Claude Code doc 31):
//
// 1. stdout.rows = MAX_SAFE_INTEGER → Ink never hits clearTerminal (ESC[2J)
// 2. Custom diff log-update (diff-log.ts) → replaces Ink's eraseLines(N)+full-rewrite
//    with line-level diff: only outputs changed lines, cursor-up is always tiny
// 3. No <Static> needed — all messages in one render, old messages naturally
//    enter terminal scrollback, diff keeps re-renders minimal
//
// The diff log-update is the key: standard Ink does eraseLines(N) + write(ALL)
// every frame. When N > terminal rows, cursor-up clamps at top → FLICKER.
// Our diff finds common prefix and only rewrites changed lines near the bottom.

import React from 'react';
import { render } from 'ink';
import { throttle } from 'es-toolkit/compat';
import { App } from './App';
import { createDiffLog } from './diff-log';

const isTTY = process.stdout.isTTY ?? false;
const realRows = process.stdout.rows ?? 24;

// Expose real terminal dimensions (stdout.rows is overridden below)
(globalThis as any).__realRows = realRows;

// ── Bypass Ink's clearTerminal path ──
// Ink's onRender: if (outputHeight >= stdout.rows) → ESC[2J (FLICKER!)
// Override rows so this NEVER fires. Our diff log-update handles rendering.
if (isTTY) {
  Object.defineProperty(process.stdout, 'rows', {
    value: Number.MAX_SAFE_INTEGER,
    writable: true,
    configurable: true,
  });
}

// ── Cleanup: restore real rows on exit ──
let _cleaned = false;
function cleanup() {
  if (_cleaned || !isTTY) return;
  _cleaned = true;
  try {
    Object.defineProperty(process.stdout, 'rows', {
      value: realRows,
      writable: true,
      configurable: true,
    });
  } catch {}
  // Show cursor on exit
  process.stdout.write('\x1B[?25h');
}

process.on('exit', cleanup);
process.on('SIGTERM', () => { cleanup(); process.exit(0); });

// ── Create Ink instance ──
const instance = render(React.createElement(App), { exitOnCtrlC: false });

// ── Monkey-patch: replace Ink's log with our diff-based renderer ──
// This is the core anti-flicker mechanism.
const diffLog = createDiffLog(process.stdout);
(instance as any).log = diffLog;
(instance as any).throttledLog = throttle(diffLog, 32, {
  leading: true,
  trailing: true,
});

// Expose diff log reset for clear/restore/newSession operations
(globalThis as any).__diffLogReset = () => diffLog.clear();

// Cleanup when Ink unmounts
instance.waitUntilExit().then(cleanup);
