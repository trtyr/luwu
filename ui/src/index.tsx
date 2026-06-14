#!/usr/bin/env bun
// luwu TUI — entry point
// Anti-flicker: Ink clearTerminal bypass (doc 31)
//
// Root cause of flicker: Ink's onRender fallback — when outputHeight >=
// stdout.rows, it uses ESC[2J (clearTerminal) instead of eraseLines.
// This full-screen clear causes visible flicker on every frame.
//
// Fix: Override stdout.rows to MAX_SAFE_INTEGER so Ink NEVER hits the
// clearTerminal path. Ink always uses eraseLines (ESC[2K per line),
// which is flicker-free. Terminal scrollback is preserved because we
// stay in main-screen mode — overflow content scrolls into history
// naturally, and eraseLines cursor-up clamps at terminal top (harmless).
//
// Alt-screen (ESC[?1049h) was tested but removed because it kills
// terminal scrollback — users can't scroll up to see history.

import React from 'react';
import { render } from 'ink';
import { App } from './App';

const isTTY = process.stdout.isTTY ?? false;
const _realRows = process.stdout.rows ?? 24;

// ── Bypass Ink's clearTerminal ──
// Ink's onRender: if (outputHeight >= stdout.rows) → ESC[2J (FLICKER!)
// Override rows to MAX_SAFE_INTEGER so Ink always uses eraseLines (safe).
// Safe because yoga layout only reads stdout.columns (width), never
// stdout.rows (height) — and eraseLines cursor-up clamps harmlessly at
// terminal top when previousLineCount > visible rows.
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
            value: _realRows,
            writable: true,
            configurable: true,
        });
    } catch {}
}

// Catch all exit paths: normal exit, crash, kill, SIGINT, SIGTERM
process.on('exit', cleanup);

// exitOnCtrlC: false — we handle Ctrl+C ourselves in App.tsx useInput
// (streaming→cancel | has text→clear | empty→exit)
const instance = render(React.createElement(App), { exitOnCtrlC: false });

// When Ink unmounts normally (via useApp().exit()), also clean up
instance.waitUntilExit().then(cleanup);
