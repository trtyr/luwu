#!/usr/bin/env bun
// luwu TUI — entry point
import React from 'react';
import { render } from 'ink';
import { App } from './App';

// exitOnCtrlC: false — we handle Ctrl+C ourselves in App.tsx useInput
// (streaming→cancel | has text→clear | empty→exit)
render(React.createElement(App), { exitOnCtrlC: false });
