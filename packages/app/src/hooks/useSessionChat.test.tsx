// @vitest-environment jsdom
// The chat hook: what an empty chat is waiting for (from the open answer, then from status events), and
// the open / close / open a dev build's StrictMode does at once.
import { act, StrictMode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const handlers = new Map<string, (e: { payload: unknown }) => void>();
const listen = vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
  handlers.set(event, cb);
  return () => handlers.delete(event);
});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...(a as Parameters<typeof listen>)) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { useSessionChat, type SessionChatHook } from './useSessionChat';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const WAITING = 'Looking in C:\\x\\projects\\p for a file written after 05:03:05.';

let host: HTMLDivElement;
let root: Root;
let latest: SessionChatHook;

function Probe({ paneId }: { paneId: string }) {
  latest = useSessionChat(paneId, true);
  return null;
}

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('useSessionChat waiting line', () => {
  it('takes what the chat waits for from the open answer', async () => {
    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [], waitingFor: WAITING } : undefined,
    );
    act(() => root.render(<Probe paneId="p1" />));
    await flush();
    expect(latest.state).toBe('ready');
    expect(latest.waitingFor).toBe(WAITING);
  });

  it('follows status events of its own pane only', async () => {
    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [], waitingFor: WAITING } : undefined,
    );
    act(() => root.render(<Probe paneId="p1" />));
    await flush();
    act(() => handlers.get('session-chat-status')!({ payload: { paneId: 'other', waitingFor: 'not mine' } }));
    expect(latest.waitingFor).toBe(WAITING);
    act(() => handlers.get('session-chat-status')!({ payload: { paneId: 'p1', waitingFor: 'Waiting for a.jsonl to be written.' } }));
    expect(latest.waitingFor).toBe('Waiting for a.jsonl to be written.');
    act(() => handlers.get('session-chat-status')!({ payload: { paneId: 'p1', waitingFor: null } }));
    expect(latest.waitingFor).toBeNull();
  });

  it('has no waiting line when the open answer carries none (an older backend)', async () => {
    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [] } : undefined,
    );
    act(() => root.render(<Probe paneId="p1" />));
    await flush();
    expect(latest.waitingFor).toBeNull();
  });
});

describe('useSessionChat under StrictMode', () => {
  it('ends with the reader open: the last call for the pane is an open, after the close', async () => {
    const calls: string[] = [];
    invoke.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      return cmd === 'session_chat_open' ? { available: true, reason: null, items: [], waitingFor: WAITING } : undefined;
    });
    act(() => root.render(<StrictMode><Probe paneId="p9" /></StrictMode>));
    await flush();
    expect(calls).toEqual(['session_chat_open', 'session_chat_close', 'session_chat_open']);
    expect(latest.state).toBe('ready');
    expect(latest.waitingFor).toBe(WAITING);
  });
});

describe('useSessionChat surfaces', () => {
  it('two views of one pane name themselves apart, and each closes only its own hold', async () => {
    const calls: Array<{ cmd: string; surface: unknown }> = [];
    invoke.mockImplementation(async (cmd: string, args?: { surface?: unknown }) => {
      calls.push({ cmd, surface: args?.surface });
      return cmd === 'session_chat_open' ? { available: true, reason: null, items: [] } : undefined;
    });
    function Two({ second }: { second: boolean }) {
      useSessionChat('p1', true);
      return second ? <Second /> : null;
    }
    function Second() {
      useSessionChat('p1', true);
      return null;
    }
    act(() => root.render(<Two second />));
    await flush();
    const opens = calls.filter((c) => c.cmd === 'session_chat_open');
    expect(opens).toHaveLength(2);
    expect(typeof opens[0]!.surface).toBe('string');
    expect(opens[0]!.surface).not.toBe(opens[1]!.surface);

    // One view goes away: it closes under its own name, the other keeps its hold.
    act(() => root.render(<Two second={false} />));
    await flush();
    const closes = calls.filter((c) => c.cmd === 'session_chat_close');
    expect(closes).toHaveLength(1);
    const surfaces = opens.map((c) => c.surface);
    expect(surfaces).toContain(closes[0]!.surface);

    // The other view closes under its own name when it goes.
    act(() => root.render(<div />));
    await flush();
    const all = calls.filter((c) => c.cmd === 'session_chat_close').map((c) => c.surface);
    expect(all).toHaveLength(2);
    expect(new Set(all)).toEqual(new Set(surfaces));
  });
});

describe('useSessionChat source', () => {
  it('tells a chat read from the screen from one read from a file, and treats a missing source as a file', async () => {
    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [], waitingFor: null, source: 'screen' } : undefined,
    );
    act(() => root.render(<Probe paneId="p1" />));
    await flush();
    expect(latest.source).toBe('screen');

    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [] } : undefined,
    );
    act(() => root.render(<Probe paneId="p2" />));
    await flush();
    expect(latest.source).toBe('file');

    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: false, reason: 'no', items: [], source: null } : undefined,
    );
    act(() => root.render(<Probe paneId="p3" />));
    await flush();
    expect(latest.state).toBe('unavailable');
    expect(latest.source).toBeNull();
  });

  it('puts a reply that is sent again under the same id in place of the old copy, keeping the order', async () => {
    const reply = (text: string) => ({ kind: 'assistant', id: 'screen:1:reply', at: null, text, model: null, finalAnswer: false });
    const user = { kind: 'user', id: 'screen:1:user', at: null, text: 'say hi' };
    invoke.mockImplementation(async (cmd: string) =>
      cmd === 'session_chat_open' ? { available: true, reason: null, items: [user, reply('hi')], source: 'screen' } : undefined,
    );
    act(() => root.render(<Probe paneId="p1" />));
    await flush();
    act(() => handlers.get('session-chat-items')!({ payload: { paneId: 'p1', items: [reply('hi there')], reset: false } }));
    act(() => handlers.get('session-chat-items')!({ payload: { paneId: 'p1', items: [reply('hi there, friend')], reset: false } }));
    expect(latest.turns).toHaveLength(1);
    expect(latest.turns[0].user?.text).toBe('say hi');
    expect(latest.turns[0].steps).toHaveLength(1);
    const step = latest.turns[0].steps[0];
    expect(step.kind === 'text' && step.item.text).toBe('hi there, friend');
    // An item that went away is a reset with what is left.
    act(() => handlers.get('session-chat-items')!({ payload: { paneId: 'p1', items: [user], reset: true } }));
    expect(latest.turns[0].steps).toHaveLength(0);
  });
});
