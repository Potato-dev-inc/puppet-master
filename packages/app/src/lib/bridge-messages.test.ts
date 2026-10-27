import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeBridgeClient } from './bridge';
import type { OutboxMessage } from './paneOutbox';

const BASE = 'http://127.0.0.1:17321';

const message = (over: Partial<OutboxMessage> = {}): OutboxMessage => ({
  id: 'm1',
  text: 'fix the test',
  createdAt: 1,
  state: 'queued',
  waitingFor: 'The agent is working. Sends when the turn ends.',
  error: null,
  ...over,
});

describe('makeBridgeClient pane message routes', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('POSTs the text to the pane and returns the new message', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ message: message() }) });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient(BASE);
    await expect(bridge.sendPaneMessage!('p 1', 'fix the test')).resolves.toEqual(message());
    expect(fetchMock).toHaveBeenCalledWith(`${BASE}/panes/p%201/messages`, expect.objectContaining({ method: 'POST' }));
    const init = fetchMock.mock.calls[0]![1] as { body: string };
    expect(JSON.parse(init.body)).toEqual({ text: 'fix the test' });
  });

  it('DELETEs one message of the pane and returns it', async () => {
    const cancelled = message({ state: 'cancelled' });
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ message: cancelled }) });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient(BASE);
    await expect(bridge.cancelPaneMessage!('p1', 'm/1')).resolves.toEqual(cancelled);
    expect(fetchMock).toHaveBeenCalledWith(`${BASE}/panes/p1/messages/m%2F1`, expect.objectContaining({ method: 'DELETE' }));
  });

  it('GETs the list of the pane, and an empty list when the PC sends none', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ messages: [message()] }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ messages: null }) });
    vi.stubGlobal('fetch', fetchMock);

    const bridge = makeBridgeClient(BASE);
    await expect(bridge.listPaneMessages!('p1')).resolves.toEqual([message()]);
    expect(fetchMock).toHaveBeenLastCalledWith(`${BASE}/panes/p1/messages`, expect.objectContaining({ method: 'GET' }));
    await expect(bridge.listPaneMessages!('p1')).resolves.toEqual([]);
  });

  it('rejects with the bridge error when the PC refuses the send', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: false,
      status: 403,
      text: async () => '{"code":"AUTHORIZATION_DENIED"}',
    }));
    const bridge = makeBridgeClient(BASE);
    await expect(bridge.sendPaneMessage!('p1', 'hi')).rejects.toThrow(/403/);
  });
});
