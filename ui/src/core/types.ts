// core/types.ts — Shared types (zero dependencies, no React/Ink imports)

export type Role = 'user' | 'assistant' | 'system';

export type Phase = 'connecting' | 'ready' | 'thinking' | 'streaming' | 'error';

export interface ToolCallInfo {
  name: string;
  args: string;
  result?: string;
  status: 'running' | 'done' | 'error';
}

// Assistant message blocks — interleaved text + tool calls in chronological order
// (Claude Code doc 15: assistant content is an array of blocks, not separate fields)
export type AssistantBlock =
  | { type: 'text'; text: string }
  | { type: 'tool'; tool: ToolCallInfo };

export interface DisplayMessage {
  id: string;
  role: Role;
  content: string;          // used for user/system messages; for assistant, derived from blocks
  blocks?: AssistantBlock[]; // assistant messages: ordered text + tool blocks
  timestamp: number;
  reasoning?: string;
}

export interface StreamEvent {
  type: string;
  delta?: string;
  content?: string;
  name?: string;
  tool_name?: string;
  arguments?: unknown;
  args?: Record<string, unknown>;
  result?: string;
  output?: string;
  message?: string;
  iteration?: number;
  assistant_text?: string;
  usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
}

export interface ModelInfo {
  id: string;
  name?: string;
}

export interface StatsResponse {
  sessions: { total: number; running: number };
  workers: number;
}

export interface CommandDef {
  name: string;
  description: string;
  aliases?: string[];
}

export interface SuggestionItem {
  id: string;
  displayText: string;
  description: string;
}

export interface TaskItem {
  id: number;
  subject: string;
  description?: string;
  status: 'pending' | 'in_progress' | 'completed';
  blocked_by?: number[];
  owner?: string;
}
