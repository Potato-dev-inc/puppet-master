import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeBridgeClient } from './bridge';

describe('makeBridgeClient session routes', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches MCP registry tools', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [{ name: 'read_session_context' }],
    });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient('http://127.0.0.1:17321');
    await expect(bridge.listMcpTools()).resolves.toEqual([{ name: 'read_session_context' }]);
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:17321/mcp/tools', expect.objectContaining({
      method: 'GET',
    }));
  });

  it('updates session context through the bridge', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ current_goal: 'Finish migration' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient('http://127.0.0.1:17321');
    await expect(bridge.updateSessionContext({ current_goal: 'Finish migration' })).resolves.toEqual({
      current_goal: 'Finish migration',
    });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:17321/session/context', expect.objectContaining({
      method: 'PATCH',
      body: JSON.stringify({ current_goal: 'Finish migration' }),
    }));
  });

  it('posts delegation preview requests', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, prompt: 'Task intent: Ship' }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient('http://127.0.0.1:17321');
    await expect(bridge.delegateTask({
      intent: 'Ship',
      acceptance_criteria: ['Tests pass'],
    })).resolves.toEqual({ ok: true, prompt: 'Task intent: Ship' });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:17321/delegate-task', expect.objectContaining({
      method: 'POST',
    }));
  });
});
