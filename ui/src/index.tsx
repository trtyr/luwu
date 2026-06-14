#!/usr/bin/env bun
// luwu TUI — entry point
// Anti-flicker: alt-screen + Ink clearTerminal bypass (doc 31)
//
// Two root causes of flicker (per Claude Code anti-flicker doc):
// 1. Main-screen mode: content enters scrollback, cursor can't reach old lines
//    → Ink does ESC[2J full-screen clear → FLICKER
// 2. Ink's onRender fallback: when outputHeight >= stdout.rows, uses clearTerminal
//    → same full-screen clear path → FLICKER
//
// Fix:
// 1. Enter alt-screen (ESC[?1049h): no scrollback, cursor reaches all lines
// 2. Override stdout.rows to MAX_SAFE_INTEGER: Ink never hits clearTerminal,
//    always uses eraseLines (ESC[2K per line) which is flicker-free

import React from 'react';
import { render } from 'ink';
import { App } from './App';

const isTTY = process.stdout.isTTY ?? false;
const _realRows = process.stdout.rows ?? 24;

// ── 1. Enter alt-screen ──
// Alt-screen has no scrollback, so content never needs ESC[2J clear.
// This single change eliminates ~90% of terminal flicker (doc 31 §4).
if (isTTY) {
    process.stdout.write('\x1b[?1049h');
}

// ── 2. Bypass Ink's clearTerminal ──
// Ink's onRender: if (outputHeight >= stdout.rows) → ESC[2J (FLICKER!)
// Override rows to MAX_SAFE_INTEGER so Ink always uses eraseLines (safe).
// Safe because yoga only reads stdout.columns for width, never stdout.rows.
if (isTTY) {
    Object.defineProperty(process.stdout, 'rows', {
        value: Number.MAX_SAFE_INTEGER,
        writable: true,
        configurable: true,
    });
}

// ── Cleanup: exit alt-screen + restore rows ──
let _cleaned = false;
function exitAltScreen() {
    if (_cleaned || !isTTY) return;
    _cleaned = true;
    try {
        Object.defineProperty(process.stdout, 'rows', {
            value: _realRows,
            writable: true,
            configurable: true,
        });
    } catch {}
    try { process.stdout.write('\x1b[?1049l'); } catch {}
}

// Catch all exit paths: normal exit, crash, kill, SIGINT, SIGTERM
process.on('exit', exitAltScreen);

// exitOnCtrlC: false — we handle Ctrl+C ourselves in App.tsx useInput
// (streaming→cancel | has text→clear | empty→exit)
const instance = render(React.createElement(App), { exitOnCtrlC: false });

// When Ink unmounts normally (via useApp().exit()), also clean up
instance.waitUntilExit().then(exitAltScreen);
