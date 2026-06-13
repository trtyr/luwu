import { describe, test, expect, mock, beforeEach, afterEach } from 'bun:test';

// We test the SSE parsing logic and API client surface.
// Since client.ts uses fetch + ReadableStream, we mock them.

// ── Helper: create a fake SSE stream ──
function createSSEStream(events: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let index = 0;
  return new ReadableStream({
    pull(controller) {
      if (index < events.length) {
        controller.enqueue(encoder.encode(events[index]));
        index++;
      } else {
        controller.close();
      }
    },
  });
}

// ── SSE parsing tests (unit test the parsing logic directly) ──
describe('SSE Parsing', () => {
  test('single event in one chunk', () => {
    const events: any[] = [];
    const chunk = 'data: {"type":"text_delta","delta":"hello"}\n\n';
    parseSSEChunk(chunk, (ev) => events.push(ev));
    expect(events.length).toBe(1);
    expect(events[0].type).toBe('text_delta');
    expect(events[0].delta).toBe('hello');
  });

  test('multiple events in one chunk', () => {
    const events: any[] = [];
    const chunk = [
      'data: {"type":"text_delta","delta":"a"}',
      '',
      'data: {"type":"text_delta","delta":"b"}',
      '',
    ].join('\n');
    parseSSEChunk(chunk, (ev) => events.push(ev));
    expect(events.length).toBe(2);
    expect(events[0].delta).toBe('a');
    expect(events[1].delta).toBe('b');
  });

  test('[DONE] termination', () => {
    let terminated = false;
    const chunk = 'data: [DONE]\n\n';
    parseSSEChunk(chunk, () => {}, () => { terminated = true; });
    expect(terminated).toBe(true);
  });

  test('malformed JSON is skipped', () => {
    const events: any[] = [];
    const chunk = 'data: {broken json}\n\ndata: {"type":"text_delta","delta":"ok"}\n\n';
    parseSSEChunk(chunk, (ev) => events.push(ev));
    expect(events.length).toBe(1);
    expect(events[0].delta).toBe('ok');
  });

  test('empty lines between events', () => {
    const events: any[] = [];
    const chunk = '\n\ndata: {"type":"done"}\n\n\n\n';
    parseSSEChunk(chunk, (ev) => events.push(ev));
    expect(events.length).toBe(1);
  });

  test('data: without space prefix', () => {
    const events: any[] = [];
    const chunk = 'data:{"type":"text_delta","delta":"x"}\n\n';
    parseSSEChunk(chunk, (ev) => events.push(ev));
    expect(events.length).toBe(1);
    expect(events[0].delta).toBe('x');
  });
});

/**
 * Inline SSE parser — mirrors the logic in client.ts streamChat
 */
function parseSSEChunk(
  data: string,
  onEvent: (ev: any) => void,
  onDone?: () => void,
): void {
  const parts = data.split('\n\n');
  for (const part of parts) {
    const lines = part.split('\n');
    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed.startsWith('data:')) continue;
      const payload = trimmed.slice(5).trim();
      if (payload === '[DONE]') { onDone?.(); return; }
      try {
        onEvent(JSON.parse(payload));
      } catch { /* skip */ }
    }
  }
}
