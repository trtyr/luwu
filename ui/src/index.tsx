#!/usr/bin/env bun
// luwu TUI — entry point
//
// Rendering: standard Ink with <Static> (Claude Code inline mode).
// <Static> commits messages to scrollback (written once, never re-rendered).
// Dynamic area stays small → eraseLines cursor-up within terminal height → no flicker.
//
// Bracketed paste: ESC[?2004h enables DEC mode 2004 so terminal wraps paste
// content in ESC[200~ ... ESC[201~ markers for precise detection (doc 32 §2.1).

import React from 'react';
import { render } from 'ink';
import { App } from './App';

const isTTY = process.stdin.isTTY;

// ── Bracketed paste mode (DEC 2004) ──
function enableBracketedPaste() {
  if (isTTY) process.stdout.write('\x1B[?2004h');
}
function disableBracketedPaste() {
  if (isTTY) process.stdout.write('\x1B[?2004l');
}

// ── Cleanup on exit ──
let _cleaned = false;
function cleanup() {
  if (_cleaned) return;
  _cleaned = true;
  process.stdout.write('\x1B[?25h'); // show cursor
  disableBracketedPaste();
}

process.on('exit', cleanup);
process.on('SIGTERM', () => { cleanup(); process.exit(0); });

// Enable bracketed paste before rendering
enableBracketedPaste();

const instance = render(React.createElement(App), {
  exitOnCtrlC: false,
});

instance.waitUntilExit().then(cleanup);
