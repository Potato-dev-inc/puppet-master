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

describe('bridge SSE focus event', () => {
  afterEach(() => {
    MockEventSource.instances = [];
    vi.unstubAllGlobals();
  });

  it('delivers a focus event with the phone view', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    const view = { on: true, endsAtMs: 1_000, minutes: 45, heldTotal: 3, waiting: 1, last: null };
    expect(MockEventSource.instances[0].listeners.has('focus')).toBe(true);
    MockEventSource.instances[0].emit('focus', JSON.stringify(view));
    expect(events).toEqual([{ type: 'focus', view }]);
    handle.unsubscribe();
  });
});
