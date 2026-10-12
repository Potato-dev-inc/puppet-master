import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BridgeEvent } from './bridge-types';
import { subscribeBridgeEventsViaPoll } from './bridge-sse';

const terminalBlock = (paneId: string, text: string) =>
  `event: terminal\ndata: ${JSON.stringify({ pane_id: paneId, data: [...new TextEncoder().encode(text)] })}\n\n`;

describe('subscribeBridgeEventsViaPoll', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('starts from now, then delivers the payloads after each cursor and reports gaps', async () => {
    const answers = [
      { cursor: 10, data: '', gap: false },
      { cursor: 12, data: terminalBlock('p1', 'hi') + 'event: pane-status\ndata: {"pane_id":"p1","status":"running"}\n\n', gap: false },
      { cursor: 40, data: terminalBlock('p1', 'later'), gap: true },
    ];
    const urls: string[] = [];
    let drained: () => void = () => {};
    const done = new Promise<void>((resolve) => { drained = resolve; });
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      urls.push(url);
      const next = answers.shift();
      if (!next) {
        drained();
        return new Promise(() => {});
      }
      return { ok: true, json: async () => next };
    }));

    const events: BridgeEvent[] = [];
    const stop = subscribeBridgeEventsViaPoll('https://x.trycloudflare.com/bridge', (e) => events.push(e));
    await done;
    stop();

    expect(urls[0]).toContain('/events/poll?wait_ms=');
    expect(urls[1]).toContain('cursor=10');
    expect(urls[2]).toContain('cursor=12');
    expect(events.map((e) => e.type)).toEqual(['terminal', 'pane-status', 'resync', 'terminal']);
  });
});
