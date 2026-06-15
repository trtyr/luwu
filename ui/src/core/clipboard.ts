// core/clipboard.ts — Clipboard write utility
//
// Based on Claude Code doc 32 §1.3: three-layer clipboard write strategy.
// Layer 1: native pbcopy/xclip (fire-and-forget, highest confidence)
// Layer 2: OSC 52 sequence to stdout (best-effort, works over SSH)
// Layer 3: tmux load-buffer -w (propagates to outer terminal)
//
// Clipboard read is NOT implemented — we only write (for /copy command etc).

const BEL = '\x07';

export type ClipboardPath = 'native' | 'tmux-buffer' | 'osc52';

/** Determine the most reliable clipboard write path for current environment */
export function getClipboardPath(): ClipboardPath {
  if (process.platform === 'darwin' && !process.env['SSH_CONNECTION']) return 'native';
  if (process.env['TMUX']) return 'tmux-buffer';
  return 'osc52';
}

/**
 * Write text to system clipboard via three-layer strategy.
 * All layers fire — the most reliable one for the environment wins.
 */
export function setClipboard(text: string): void {
  // Layer 1: native (fire-and-forget)
  copyNative(text);

  // Layer 2: OSC 52 (write sequence to stdout — terminal processes it)
  const b64 = Buffer.from(text, 'utf8').toString('base64');
  const osc52 = `\x1b]52;c;${b64}${BEL}`;

  // Layer 3: tmux buffer (propagate to outer terminal via OSC 52)
  if (process.env['TMUX']) {
    tmuxLoadBuffer(text);
    // tmux with -w will emit its own OSC 52, so we don't double-write
  } else {
    process.stdout.write(osc52);
  }
}

/** Get a user-facing toast message for clipboard operations */
export function getCopiedToast(text: string): string {
  const path = getClipboardPath();
  const n = text.length;
  switch (path) {
    case 'native':
      return `copied ${n} chars to clipboard`;
    case 'tmux-buffer':
      return `copied ${n} chars to tmux buffer · paste with prefix + ]`;
    case 'osc52':
      return `sent ${n} chars via OSC 52 · check terminal clipboard settings if paste fails`;
  }
}

// ── Internal ──

function copyNative(text: string): void {
  try {
    if (process.platform === 'darwin') {
      const proc = Bun.spawn(['pbcopy'], { stdin: 'pipe' });
      proc.stdin?.write(text);
      proc.stdin?.end();
    } else if (process.platform === 'linux') {
      // Try xclip first (most common on X11), wl-copy on Wayland
      const proc = Bun.spawn(['xclip', '-selection', 'clipboard'], { stdin: 'pipe' });
      proc.stdin?.write(text);
      proc.stdin?.end();
    }
  } catch {
    // Silently fail — OSC 52 may still work
  }
}

function tmuxLoadBuffer(text: string): void {
  try {
    const args = process.env['LC_TERMINAL'] === 'iTerm2'
      ? ['load-buffer', '-']           // iTerm2: no -w (crash bug #22432)
      : ['load-buffer', '-w', '-'];    // Others: -w propagates to outer terminal
    const proc = Bun.spawn(['tmux', ...args], { stdin: 'pipe' });
    proc.stdin?.write(text);
    proc.stdin?.end();
  } catch {
    // Silently fail
  }
}
