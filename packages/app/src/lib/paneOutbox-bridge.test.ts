// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from './bridge-types';
import type { OutboxMessage } from './paneOutbox';
import { bridgeOutboxTransport, bridgeRefusalText, OUTBOX_POLL_MS, publishBridgeOutbox } from './paneOutbox-bridge';

const message = (over: Partial<OutboxMessage> = {}): OutboxMessage => ({
  id: 'm1',
  text: 'fix it',
  createdAt: 1,
  state: 'queued',
  waitingFor: 'The agent is working. Sends when the turn ends.',
  error: null,
  ...over,
});

let visibility: DocumentVisibilityState = 'visible';

beforeEach(() => {
  vi.useFakeTimers();
  visibility = 'visible';
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility });
});

afterEach(() => {
  vi.useRealTimers();
});

describe('bridgeOutboxTransport send and cancel', () => {
  it('sends through the bridge and returns the message the PC made', async () => {
    const sendPaneMessage = vi.fn(async () => message());
    const transport = bridgeOutboxTransport({ sendPaneMessage } as unknown as BridgeClient);
    await expect(transport.send('p1', 'fix it')).resolves.toEqual(message());
    expect(sendPaneMessage).toHaveBeenCalledWith('p1', 'fix it');
  });

  it('gives the PC refusal as one plain sentence, not the raw bridge error', async () => {
    const sendPaneMessage = vi.fn(async () => {
      throw new Error('bridge POST /panes/p1/messages -> 400: {"error":"The message is empty.","code":"MESSAGE_REFUSED"}');
    });
    const transport = bridgeOutboxTransport({ sendPaneMessage } as unknown as BridgeClient);
    await expect(transport.send('p1', '')).rejects.toThrow('The message is empty.');
  });

  it('says the PC cannot be reached when the request never got there', async () => {
    const sendPaneMessage = vi.fn(async () => {
      throw new TypeError('Failed to fetch');
    });
    const transport = bridgeOutboxTransport({ sendPaneMessage } as unknown as BridgeClient);
    await expect(transport.send('p1', 'hi')).rejects.toThrow('Could not reach your PC.');
  });

  it('says an old PC cannot take messages when the route is missing', async () => {
    const transport = bridgeOutboxTransport({} as BridgeClient);
    await expect(transport.send('p1', 'hi')).rejects.toThrow('This PC is too old to take messages from the phone.');
    await expect(transport.cancel('p1', 'm1')).rejects.toThrow('This PC is too old to take messages from the phone.');
  });

  it('cancels one message through the bridge', async () => {
    const cancelPaneMessage = vi.fn(async () => message({ state: 'cancelled' }));
    const transport = bridgeOutboxTransport({ cancelPaneMessage } as unknown as BridgeClient);
    await expect(transport.cancel('p1', 'm1')).resolves.toEqual(message({ state: 'cancelled' }));
    expect(cancelPaneMessage).toHaveBeenCalledWith('p1', 'm1');
  });
});

describe('bridgeOutboxTransport watch', () => {
  it('gets the pane list from the PC when a pane-outbox event comes in, for that pane only', () => {
    const transport = bridgeOutboxTransport({} as BridgeClient);
    const seen: string[][] = [];
    const stop = transport.watch('p1', (list) => seen.push(list.map((m) => m.id)));
    publishBridgeOutbox('p2', [message({ id: 'other' })]);
    publishBridgeOutbox('p1', [message({ id: 'm1' })]);
    expect(seen).toEqual([['m1']]);
    stop();
    publishBridgeOutbox('p1', [message({ id: 'm2' })]);
    expect(seen).toEqual([['m1']]);
  });

  it('reads the list every few seconds while the page is visible, and not while it is hidden', async () => {
    const listPaneMessages = vi.fn(async () => [message()]);
    const transport = bridgeOutboxTransport({ listPaneMessages } as unknown as BridgeClient);
    const cb = vi.fn();
    const stop = transport.watch('p1', cb);

    await vi.advanceTimersByTimeAsync(OUTBOX_POLL_MS);
    expect(listPaneMessages).toHaveBeenCalledTimes(1);
    expect(cb).toHaveBeenLastCalledWith([message()]);

    visibility = 'hidden';
    await vi.advanceTimersByTimeAsync(OUTBOX_POLL_MS * 3);
    expect(listPaneMessages).toHaveBeenCalledTimes(1);

    stop();
    visibility = 'visible';
    await vi.advanceTimersByTimeAsync(OUTBOX_POLL_MS * 3);
    expect(listPaneMessages).toHaveBeenCalledTimes(1);
  });

  it('a failed read is skipped and the next one still runs', async () => {
    const listPaneMessages = vi.fn().mockRejectedValueOnce(new TypeError('Failed to fetch')).mockResolvedValue([message()]);
    const transport = bridgeOutboxTransport({ listPaneMessages } as unknown as BridgeClient);
    const cb = vi.fn();
    const stop = transport.watch('p1', cb);
    await vi.advanceTimersByTimeAsync(OUTBOX_POLL_MS * 2);
    expect(listPaneMessages).toHaveBeenCalledTimes(2);
    expect(cb).toHaveBeenCalledTimes(1);
    stop();
  });
});

describe('bridgeRefusalText', () => {
  it('reads the error sentence out of a bridge refusal', () => {
    expect(bridgeRefusalText(new Error('bridge DELETE /x -> 409: {"error":"Too late to cancel.","code":"X"}'), 'fallback')).toBe('Too late to cancel.');
  });

  it('keeps a plain body and falls back when the body has no sentence', () => {
    expect(bridgeRefusalText(new Error('bridge GET /x -> 502: The PC is asleep.'), 'fallback')).toBe('The PC is asleep.');
    expect(bridgeRefusalText(new Error('bridge POST /x -> 500: {}'), 'fallback')).toBe('fallback');
  });
});
