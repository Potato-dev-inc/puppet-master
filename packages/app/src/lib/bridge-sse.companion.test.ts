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

describe('bridge SSE companion event', () => {
  afterEach(() => {
    MockEventSource.instances = [];
    vi.unstubAllGlobals();
  });

  it('delivers the companion list', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    const companions = [{ id: 'mochi', name: 'Mochi', state: 'idle' }];
    MockEventSource.instances[0].emit('companion', JSON.stringify({ v: 1, companions }));
    expect(events).toEqual([{ type: 'companion', companions }]);
    handle.unsubscribe();
  });

  it('treats a payload without a list as an empty list', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    MockEventSource.instances[0].emit('companion', JSON.stringify({ v: 1 }));
    expect(events).toEqual([{ type: 'companion', companions: [] }]);
    handle.unsubscribe();
  });
});
