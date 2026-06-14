// services/api.ts — luwu server API client
// (migrated from client.ts, same API surface)
import type { StreamEvent, ModelInfo, StatsResponse, TaskItem } from '../core/types.js';

export const BASE_URL = process.env.LUWU_URL || 'http://127.0.0.1:51740';

export async function checkHealth(): Promise<boolean> {
  const res = await fetch(`${BASE_URL}/health`);
  return res.ok;
}

export async function getModels(): Promise<ModelInfo[]> {
  const res = await fetch(`${BASE_URL}/v1/models`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.data ?? [];
}

export async function createSession(): Promise<string> {
  const res = await fetch(`${BASE_URL}/v1/sessions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({}),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Failed to create session (${res.status}): ${text}`);
  }
  const data = await res.json();
  return data.id;
}

export async function getStats(): Promise<StatsResponse> {
  const res = await fetch(`${BASE_URL}/v1/stats`);
  if (!res.ok) throw new Error(`Stats request failed (${res.status})`);
  return res.json();
}

export async function getSkills(): Promise<Array<{ name: string; description?: string }>> {
  const res = await fetch(`${BASE_URL}/v1/skills`);
  if (!res.ok) return [];
  return res.json();
}

export async function listSessions(): Promise<Array<{
  id: string; model: string; message_count: number; is_running: boolean;
}>> {
  const res = await fetch(`${BASE_URL}/v1/sessions`);
  if (!res.ok) return [];
  const data = await res.json();
  return data.sessions ?? [];
}

export async function deleteSession(id: string): Promise<boolean> {
  const res = await fetch(`${BASE_URL}/v1/sessions/${id}`, { method: 'DELETE' });
  return res.ok;
}

export async function streamChat(
  sessionId: string,
  message: string,
  onEvent: (event: StreamEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`${BASE_URL}/v1/sessions/${sessionId}/chat`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    },
    body: JSON.stringify({ message, stream: true }),
    signal,
  });

  if (!res.ok) {
    const text = await res.text().catch(() => 'unknown error');
    throw new Error(`Server error ${res.status}: ${text}`);
  }

  const body = res.body;
  if (!body) throw new Error('No response body');

  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split('\n\n');
      buffer = parts.pop() || '';

      for (const part of parts) {
        const lines = part.split('\n');
        for (const line of lines) {
          const trimmed = line.trim();
          if (!trimmed.startsWith('data:')) continue;

          const payload = trimmed.slice(5).trim();
          if (payload === '[DONE]') return;

          try {
            const event = JSON.parse(payload) as StreamEvent;
            onEvent(event);
          } catch {
            // skip unparseable lines
          }
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

export async function cancelTurn(sessionId: string): Promise<void> {
  await fetch(`${BASE_URL}/v1/sessions/${sessionId}/cancel`, { method: 'POST' });
}

export async function getTasks(sessionId: string): Promise<TaskItem[]> {
  const res = await fetch(`${BASE_URL}/v1/sessions/${sessionId}/tasks`);
  if (!res.ok) return [];
  const data = await res.json();
  return (data.tasks || []) as TaskItem[];
}
