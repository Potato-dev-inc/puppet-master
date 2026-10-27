import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BridgeEvent } from './bridge-types';
import { subscribeBridgeEventsHandle } from './bridge-sse';

class MockEventSource {
  static instances: MockEventSource[] = [];
  listeners = new Map<string, (ev: { data: string }) => void>();
  onerror: (() => void) | null = null;
  onopen: (() => void) | null = null;
  constructor(public url: string) {
    MockEventSource.instances.push(this);
  }
  addEventListener(name: string, cb: (ev: { data: string }) => void) {
    this.listeners.set(name, cb);
  }
  emit(name: string, data: string) {
    this.listeners.get(name)?.({ data });
  }
  close() {}
}

const BASE = 'http://127.0.0.1:1234';

describe('bridge SSE pane-outbox event', () => {
  afterEach(() => {
    MockEventSource.instances = [];
    vi.unstubAllGlobals();
  });

  it('delivers the pane message list with the pane id', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    const messages = [{ id: 'm1', text: 'hi', createdAt: 5, state: 'queued', waitingFor: 'Waiting.', error: null }];
    MockEventSource.instances[0].emit('pane-outbox', JSON.stringify({ paneId: 'p1', messages }));
    expect(events).toEqual([{ type: 'pane-outbox', paneId: 'p1', messages }]);
    handle.unsubscribe();
  });

  it('treats a payload without a list as an empty list', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    MockEventSource.instances[0].emit('pane-outbox', JSON.stringify({ paneId: 'p2' }));
    expect(events).toEqual([{ type: 'pane-outbox', paneId: 'p2', messages: [] }]);
    handle.unsubscribe();
  });
});
