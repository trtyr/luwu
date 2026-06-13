// luwu TUI shared types

export type Role = 'user' | 'assistant' | 'system';

export interface ToolCallInfo {
  name: string;
  args: string;
  result?: string;
  status: 'running' | 'done' | 'error';
}

export interface DisplayMessage {
  id: string;
  role: Role;
  content: string;
  tools?: ToolCallInfo[];
  timestamp: number;
}

export interface StreamEvent {
  type: string;
  content?: string;
  name?: string;
  arguments?: string;
  args?: Record<string, unknown>;
  result?: string;
  message?: string;
  iteration?: number;
}

export interface ModelInfo {
  id: string;
  name?: string;
}

export interface StatsResponse {
  sessions: { total: number; running: number };
  workers: number;
}

export type Phase = 'connecting' | 'ready' | 'thinking' | 'streaming' | 'error';
