import { describe, test, expect } from 'bun:test';
import type {
  DisplayMessage, ToolCallInfo, StreamEvent, Phase, ModelInfo, StatsResponse, Role,
} from '../src/core/types.ts';

describe('DisplayMessage', () => {
  test('can construct user message', () => {
    const msg: DisplayMessage = {
      id: 'm1', role: 'user', content: 'hello', timestamp: Date.now(),
    };
    expect(msg.role).toBe('user');
    expect(msg.content).toBe('hello');
  });

  test('can construct assistant message with tools', () => {
    const msg: DisplayMessage = {
      id: 'm2', role: 'assistant', content: 'result', timestamp: Date.now(),
      tools: [{ name: 'bash', args: '{}', result: 'ok', status: 'done' }],
    };
    expect(msg.role).toBe('assistant');
    expect(msg.tools?.length).toBe(1);
    expect(msg.tools?.[0].name).toBe('bash');
  });

  test('can construct system message', () => {
    const msg: DisplayMessage = {
      id: 'm3', role: 'system', content: 'system msg', timestamp: Date.now(),
    };
    expect(msg.role).toBe('system');
  });
});

describe('ToolCallInfo', () => {
  test('all status values are valid', () => {
    const statuses: ToolCallInfo['status'][] = ['running', 'done', 'error'];
    expect(statuses.length).toBe(3);
  });

  test('can construct with optional result', () => {
    const tool: ToolCallInfo = { name: 'read', args: '', status: 'running' };
    expect(tool.result).toBeUndefined();
  });
});

describe('StreamEvent', () => {
  test('text_delta uses delta field', () => {
    const ev: StreamEvent = { type: 'text_delta', delta: 'hello' };
    expect(ev.delta).toBe('hello');
  });

  test('done has usage', () => {
    const ev: StreamEvent = {
      type: 'done',
      assistant_text: 'hello',
      usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
    };
    expect(ev.usage?.total_tokens).toBe(15);
  });

  test('error has message', () => {
    const ev: StreamEvent = { type: 'error', message: 'something broke' };
    expect(ev.message).toBe('something broke');
  });

  test('iteration_end has iteration number', () => {
    const ev: StreamEvent = { type: 'iteration_end', iteration: 3 };
    expect(ev.iteration).toBe(3);
  });
});

describe('Phase', () => {
  test('all phase values are valid', () => {
    const phases: Phase[] = ['connecting', 'ready', 'thinking', 'streaming', 'error'];
    expect(phases.length).toBe(5);
  });
});

describe('Role', () => {
  test('all role values are valid', () => {
    const roles: Role[] = ['user', 'assistant', 'system'];
    expect(roles.length).toBe(3);
  });
});

describe('ModelInfo', () => {
  test('can construct with id', () => {
    const m: ModelInfo = { id: 'glm-4.7' };
    expect(m.id).toBe('glm-4.7');
  });

  test('name is optional', () => {
    const m: ModelInfo = { id: 'glm-4.7', name: 'GLM 4.7' };
    expect(m.name).toBe('GLM 4.7');
  });
});

describe('StatsResponse', () => {
  test('can construct full response', () => {
    const s: StatsResponse = {
      sessions: { total: 100, running: 2 },
      workers: 3,
    };
    expect(s.sessions.total).toBe(100);
    expect(s.workers).toBe(3);
  });
});
