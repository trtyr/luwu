import { describe, test, expect, mock, beforeEach, afterEach } from 'bun:test';

const originalFetch = globalThis.fetch;

function mockRes(opts: { ok?: boolean; status?: number; json?: () => Promise<any>; text?: () => Promise<string>; body?: ReadableStream<Uint8Array> }): Response {
  return { ok: opts.ok ?? true, status: opts.status ?? 200, json: opts.json ?? (async () => ({})), text: opts.text ?? (async () => ''), body: opts.body ?? null } as any;
}

beforeEach(() => { globalThis.fetch = mock(() => Promise.resolve(mockRes({}))) as any; });
afterEach(() => { globalThis.fetch = originalFetch; });

describe('checkHealth', () => {
  test('ok=true → true', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: true }))) as any;
    const { checkHealth } = await import('../src/services/api.ts');
    expect(await checkHealth()).toBe(true);
  });
  test('ok=false → false', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: false, status: 503 }))) as any;
    const { checkHealth } = await import('../src/services/api.ts');
    expect(await checkHealth()).toBe(false);
  });
});

describe('getModels', () => {
  test('returns array', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ json: async () => ({ data: [{ id: 'm1' }] }) }))) as any;
    const { getModels } = await import('../src/services/api.ts');
    const r = await getModels();
    expect(r.length).toBe(1);
    expect(r[0].id).toBe('m1');
  });
  test('error → empty', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: false }))) as any;
    const { getModels } = await import('../src/services/api.ts');
    expect(await getModels()).toEqual([]);
  });
});

describe('createSession', () => {
  test('returns id', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ json: async () => ({ id: 's1' }) }))) as any;
    const { createSession } = await import('../src/services/api.ts');
    expect(await createSession()).toBe('s1');
  });
  test('throws on error', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: false, status: 500, text: async () => 'err' }))) as any;
    const { createSession } = await import('../src/services/api.ts');
    expect(createSession()).rejects.toThrow();
  });
});

describe('getStats', () => {
  test('returns parsed', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ json: async () => ({ sessions: { total: 5, running: 1 }, workers: 2 }) }))) as any;
    const { getStats } = await import('../src/services/api.ts');
    const s = await getStats();
    expect(s.sessions.total).toBe(5);
    expect(s.workers).toBe(2);
  });
});

describe('getSkills', () => {
  test('returns array', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ json: async () => [{ name: 'x' }] }))) as any;
    const { getSkills } = await import('../src/services/api.ts');
    expect((await getSkills()).length).toBe(1);
  });
});

describe('listSessions', () => {
  test('returns array', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ json: async () => ({ sessions: [{ id: 's1', model: 'm', message_count: 3, is_running: false }] }) }))) as any;
    const { listSessions } = await import('../src/services/api.ts');
    expect((await listSessions()).length).toBe(1);
  });
});

describe('deleteSession', () => {
  test('ok → true', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: true }))) as any;
    const { deleteSession } = await import('../src/services/api.ts');
    expect(await deleteSession('s1')).toBe(true);
  });
});

describe('cancelTurn', () => {
  test('calls POST', async () => {
    let url = '';
    globalThis.fetch = ((u: string) => { url = u; return Promise.resolve(mockRes({ ok: true })); }) as any;
    const { cancelTurn } = await import('../src/services/api.ts');
    await cancelTurn('session-1');
    expect(url).toContain('/cancel');
    expect(url).toContain('session-1');
  });
});

describe('streamChat', () => {
  test('parses SSE', async () => {
    const sseData = 'data: {"type":"text_delta","delta":"hi"}\n\ndata: [DONE]\n\n';
    const encoder = new TextEncoder();
    const stream = new ReadableStream({ start(c) { c.enqueue(encoder.encode(sseData)); c.close(); } });
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: true, body: stream }))) as any;
    const { streamChat } = await import('../src/services/api.ts');
    const events: any[] = [];
    await streamChat('s1', 'hi', (ev) => events.push(ev));
    expect(events.length).toBe(1);
    expect(events[0].delta).toBe('hi');
  });

  test('error → throws', async () => {
    globalThis.fetch = mock(() => Promise.resolve(mockRes({ ok: false, status: 500, text: async () => 'err' }))) as any;
    const { streamChat } = await import('../src/services/api.ts');
    expect(streamChat('s1', 'hi', () => {})).rejects.toThrow();
  });
});
