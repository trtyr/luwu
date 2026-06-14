// core/toolUtils.ts — per-tool semantic result summaries
// Source: Claude Code doc 26-tool-output-rendering.md §5
// Each tool decides its own one-line summary format (not raw truncated text).

/** Parse tool args JSON string → display string for the tool_use line */
export function parseToolArgs(name: string, args: string): string {
  if (!args) return '';
  try {
    const p = JSON.parse(args);

    // File-based tools: show path
    if (p.path) return p.path;
    if (p.file_path) return p.file_path;

    // Bash: show command
    if (p.command) return p.command;

    // Search tools: pattern + path
    if (p.pattern) {
      const scope = p.path ? `, path: ${p.path}` : '';
      return `pattern: "${p.pattern}"${scope}`;
    }

    // Grep (ffgrep): pattern + optional path
    if (p.query) {
      const scope = p.path ? `, path: ${p.path}` : '';
      return `"${p.query}"${scope}`;
    }

    // Write: file path is in path
    if (p.target) return p.target;

    // Edit: old_text/new_text → show nothing (too long)
    if (p.old_text || p.new_text) return '';

    // URL-based tools
    if (p.url) return p.url;

    // Generic: show first string value
    const vals = Object.values(p).filter(v => typeof v === 'string') as string[];
    if (vals.length > 0) return vals[0].slice(0, 60);

    return '';
  } catch {
    // Not JSON — return as-is, truncated
    return args.length > 60 ? args.slice(0, 57) + '...' : args;
  }
}

/**
 * Summarize tool result → human-readable one-liner.
 * Matches Claude Code doc 26 §5 patterns:
 *   Read → "Read N lines"
 *   Grep → "Found N files" / "Found N matches"
 *   Bash → first stdout line or "(No output)"
 *   Write → "Wrote N lines"
 *   Edit → "Edited N lines"
 */
export function summarizeToolResult(name: string, result: string | undefined): string | null {
  if (!result) return null;
  const trimmed = result.trim();
  if (!trimmed) return null;

  const lower = name.toLowerCase();

  // ── Read ──
  if (lower === 'read') {
    const lines = trimmed.split('\n').length;
    return `Read ${lines} ${lines === 1 ? 'line' : 'lines'}`;
  }

  // ── Grep / ffgrep ──
  if (lower === 'grep' || lower === 'ffgrep' || lower === 'memory' || lower === 'memory_search') {
    // Count file-like lines (contain : or look like file paths)
    const lines = trimmed.split('\n');
    const fileCount = lines.filter(l =>
      l.includes('/') || l.includes(':') || l.includes('.ts') || l.includes('.rs') || l.includes('.py')
    ).length;
    if (fileCount > 0 && fileCount < lines.length) {
      return `Found ${fileCount} ${fileCount === 1 ? 'result' : 'results'}`;
    }
    return `Found ${lines.length} ${lines.length === 1 ? 'match' : 'matches'}`;
  }

  // ── Find / fffind ──
  if (lower === 'find' || lower === 'fffind' || lower === 'glob') {
    const files = trimmed.split('\n').filter(l => l.trim());
    return `Found ${files.length} ${files.length === 1 ? 'file' : 'files'}`;
  }

  // ── Bash ──
  if (lower === 'bash') {
    // Try to detect structured output
    const lines = trimmed.split('\n');

    // Check for common patterns: exit codes, pass/fail
    if (/^\d+ (pass|fail|PASS|FAIL)/.test(trimmed) || /tests?[:\s]/i.test(trimmed)) {
      return lines[0].slice(0, 80);
    }

    // Check for empty output
    if (/^\(no output\)|^done$|^$/i.test(trimmed)) {
      return '(No output)';
    }

    // First non-empty line, truncated
    const firstLine = lines.find(l => l.trim()) || '(No output)';
    return firstLine.length > 80 ? firstLine.slice(0, 77) + '...' : firstLine;
  }

  // ── Write ──
  if (lower === 'write') {
    return trimmed.includes('error') || trimmed.includes('Error')
      ? trimmed.slice(0, 80)
      : 'File written';
  }

  // ── Edit ──
  if (lower === 'edit') {
    if (/error|Error|not found/i.test(trimmed)) return trimmed.slice(0, 80);
    return 'File edited';
  }

  // ── Web fetch ──
  if (lower === 'web_fetch' || lower === 'webfetch') {
    const chars = trimmed.length;
    return `Fetched ${chars.toLocaleString()} chars`;
  }

  // ── Default: first line truncated ──
  const firstLine = trimmed.split('\n')[0];
  return firstLine.length > 80 ? firstLine.slice(0, 77) + '...' : firstLine;
}

/** User-facing tool name — normalize server names to Claude Code style */
export function toolDisplayName(name: string): string {
  const map: Record<string, string> = {
    'fffind': 'Glob',
    'ffgrep': 'Grep',
    'memory': 'Memory',
    'memory_search': 'Memory',
    'todo': 'Todo',
    'web_fetch': 'WebFetch',
    'webfetch': 'WebFetch',
  };
  return map[name.toLowerCase()] || name.charAt(0).toUpperCase() + name.slice(1);
}
