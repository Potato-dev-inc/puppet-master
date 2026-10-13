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

describe('bridge SSE loop-signal event', () => {
  afterEach(() => {
    MockEventSource.instances = [];
    vi.unstubAllGlobals();
  });

  it('delivers a loop-signal with its signal', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    MockEventSource.instances[0].emit(
      'loop-signal',
      JSON.stringify({ pane_id: 'p1', signal: { kind: 'repeated_error', summary: 'the same error 4 times' } }),
    );
    expect(events).toEqual([
      { type: 'loop-signal', pane_id: 'p1', signal: { kind: 'repeated_error', summary: 'the same error 4 times' } },
    ]);
    handle.unsubscribe();
  });

  it('delivers a cleared loop-signal with signal null', () => {
    vi.stubGlobal('EventSource', MockEventSource);
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsHandle(BASE, (e) => events.push(e));
    MockEventSource.instances[0].emit('loop-signal', JSON.stringify({ pane_id: 'p1', signal: null }));
    expect(events).toEqual([{ type: 'loop-signal', pane_id: 'p1', signal: null }]);
    handle.unsubscribe();
  });
});
