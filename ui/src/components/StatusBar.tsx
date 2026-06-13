// Bottom status bar — model, clock, cwd, git branch, context stats
import React from 'react';
import { Box, Text } from 'ink';

export interface StatusBarProps {
  model: string;
  cwd: string;
  gitBranch: string | null;
  contextPct: number;     // 0-100
  sessionCount: number;
  thinking: boolean;
}

function clockStr(): string {
  const d = new Date();
  const h = String(d.getHours()).padStart(2, '0');
  const m = String(d.getMinutes()).padStart(2, '0');
  return `${h}:${m}`;
}

// shorten a long path: /Users/foo/Documents/Code/Rust/luwu → ~/Doc/Code/Rust/luwu
function shortPath(p: string): string {
  const home = process.env.HOME || '';
  let out = p;
  if (home && out.startsWith(home)) out = '~' + out.slice(home.length);
  const parts = out.split('/');
  if (parts.length > 4) {
    return parts[0] + '/' + parts.slice(1, 2).map(s => s.slice(0, 3)).join('/') + '/…/' + parts.slice(-2).join('/');
  }
  return out;
}

export function StatusBar(props: StatusBarProps) {
  const [time, setTime] = React.useState(clockStr());

  React.useEffect(() => {
    const id = setInterval(() => setTime(clockStr()), 30000);
    return () => clearInterval(id);
  }, []);

  const ctxColor = props.contextPct > 80 ? 'red' : props.contextPct > 50 ? 'yellow' : 'green';

  return (
    <Box flexDirection="column">
      {/* separator line */}
      <Text dimColor>{'━'.repeat(50)}</Text>

      {/* main status row */}
      <Box justifyContent="space-between">
        <Box gap={1}>
          {/* model */}
          <Text bold color="magenta">● {props.model}</Text>

          {/* git branch */}
          {props.gitBranch && (
            <Text color="blue">  {props.gitBranch === 'master' || props.gitBranch === 'main' ? '⎇ ' + props.gitBranch : '⎇ ' + props.gitBranch}</Text>
          )}

          {/* cwd */}
          <Text dimColor>  {shortPath(props.cwd)}</Text>
        </Box>

        <Box gap={1}>
          {/* context */}
          <Text color={ctxColor}>CTX {props.contextPct}%</Text>

          {/* sessions */}
          {props.sessionCount > 0 && (
            <Text dimColor>○ {props.sessionCount}</Text>
          )}

          {/* thinking indicator */}
          {props.thinking && (
            <Text color="yellow">thinking…</Text>
          )}

          {/* clock */}
          <Text dimColor>{time}</Text>
        </Box>
      </Box>
    </Box>
  );
}
