#!/usr/bin/env bun
// luwu TUI — entry point
//
// Rendering strategy: standard Ink with <Static> (Claude Code inline mode).
//
// <Static> writes completed messages to terminal ONCE — they enter scrollback
// and are never touched again. The dynamic area (streaming message + spinner +
// input + statusbar) is small (~10-15 lines), so Ink's eraseLines(N) cursor-up
// is always well within terminal height → NO FLICKER.
//
// No hacks: no stdout.rows override, no custom diff-log, no alt-screen.
// Terminal native scrollback works naturally for browsing history.

import React from 'react';
import { render } from 'ink';
import { App } from './App';

// Cleanup: show cursor on exit
let _cleaned = false;
function cleanup() {
  if (_cleaned) return;
  _cleaned = true;
  process.stdout.write('\x1B[?25h');
}

process.on('exit', cleanup);
process.on('SIGTERM', () => { cleanup(); process.exit(0); });

const instance = render(React.createElement(App), {
  exitOnCtrlC: false,
});

instance.waitUntilExit().then(cleanup);
