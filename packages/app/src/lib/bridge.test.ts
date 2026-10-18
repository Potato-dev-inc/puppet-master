import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeBridgeClient } from './bridge';

describe('makeBridgeClient session routes', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('reads the new pane id out of the spawn envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ ok: true, snapshot: { pane_id: 'p-1' }, suggested_wait: {} }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient('http://127.0.0.1:17321');
    await expect(bridge.spawnPane({ agent_type: 'powershell' })).resolves.toEqual({ pane_id: 'p-1' });
  });

  it('treats closing a pane that is already gone (typed 404) as done, and still reports other failures', async () => {
    const gone = vi.fn().mockResolvedValue({
      ok: false,
      status: 404,
      text: async () => JSON.stringify({ code: 'PANE_NOT_FOUND', error: 'unknown pane: p-9' }),
    });
    vi.stubGlobal('fetch', gone);
    const bridge = makeBridgeClient('http://127.0.0.1:17321');
    await expect(bridge.killPane('p-9')).resolves.toBeUndefined();
    expect(gone).toHaveBeenCalledWith('http://127.0.0.1:17321/panes/p-9', expect.objectContaining({ method: 'DELETE' }));

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 403, text: async () => '{"code":"AUTHORIZATION_DENIED"}' }));
    await expect(bridge.killPane('p-1')).rejects.toThrow(/403/);
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

  it('shows the proxy\'s BRIDGE_UNREACHABLE message instead of raw JSON', async () => {
    const message = "The Puppet Master app's bridge is not running on port 17999 (read from a port file).";
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: false,
      status: 502,
      text: async () => JSON.stringify({ code: 'BRIDGE_UNREACHABLE', message }),
    }));
    const bridge = makeBridgeClient('http://127.0.0.1:1');
    await expect(bridge.listPanes()).rejects.toThrow(`bridge GET /panes -> 502: ${message}`);
  });
});
