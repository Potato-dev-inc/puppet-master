// @vitest-environment jsdom
// The state hook reads on mount and again on every `potato://changed`; the thread hook reads the open bot,
// marks it read, and the remembered thread survives a blocked store.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { bot, msg } from '../components/cockpit/potato/test-helpers';
import { fakePotatoTransport } from '../lib/potato';
import { readOpenBot, usePotatoState, usePotatoThread, writeOpenBot } from './usePotato';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

function StateProbe({ transport, out }: { transport: ReturnType<typeof fakePotatoTransport>; out: { last?: ReturnType<typeof usePotatoState> } }) {
  out.last = usePotatoState(transport);
  return null;
}

describe('usePotatoState', () => {
  it('reads the state, then again when the app says something changed', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    const out: { last?: ReturnType<typeof usePotatoState> } = {};
    await act(async () => root.render(<StateProbe transport={transport} out={out} />));
    await act(async () => {});
    expect(out.last!.state?.bots.map((b) => b.id)).toEqual(['fish']);
    expect(out.last!.state?.enabled).toBe(true);
    const before = out.last!.version;

    await act(async () => transport.setEnabled(false));
    await act(async () => {});
    expect(out.last!.state?.enabled).toBe(false);
    expect(out.last!.version).toBeGreaterThan(before);
  });

  it('keeps the last good state, and goes to none when the first read fails', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    vi.spyOn(transport, 'state').mockRejectedValue(new Error('POTATO_NOT_READY: Potato Bot is not ready yet.'));
    const out: { last?: ReturnType<typeof usePotatoState> } = {};
    await act(async () => root.render(<StateProbe transport={transport} out={out} />));
    await act(async () => {});
    expect(out.last!.state).toBeNull();
  });

  it('stops listening when it leaves', async () => {
    const transport = fakePotatoTransport({ bots: [bot()] });
    const out: { last?: ReturnType<typeof usePotatoState> } = {};
    await act(async () => root.render(<StateProbe transport={transport} out={out} />));
    await act(async () => {});
    const spy = vi.spyOn(transport, 'state');
    act(() => root.render(<div />));
    await act(async () => transport.setEnabled(false));
    expect(spy).not.toHaveBeenCalled();
  });
});

function ThreadProbe({ transport, id, version, unread, out }: { transport: ReturnType<typeof fakePotatoTransport>; id: string | null; version: number; unread: number; out: { last?: ReturnType<typeof usePotatoThread> } }) {
  out.last = usePotatoThread(transport, id, version, unread);
  return null;
}

describe('usePotatoThread', () => {
  it('reads the open bot, again when the version moves, and nothing for no bot', async () => {
    const transport = fakePotatoTransport({ bots: [bot()], messages: { fish: [msg({ text: 'hi' })] } });
    const out: { last?: ReturnType<typeof usePotatoThread> } = {};
    await act(async () => root.render(<ThreadProbe transport={transport} id="fish" version={1} unread={0} out={out} />));
    await act(async () => {});
    expect(out.last!.messages.map((m) => m.text)).toEqual(['hi']);
    expect(out.last!.loaded).toBe(true);

    transport.addMessage('fish', msg({ text: 'there' }));
    await act(async () => root.render(<ThreadProbe transport={transport} id="fish" version={2} unread={0} out={out} />));
    await act(async () => {});
    expect(out.last!.messages.map((m) => m.text)).toEqual(['hi', 'there']);

    await act(async () => root.render(<ThreadProbe transport={transport} id={null} version={2} unread={0} out={out} />));
    expect(out.last!.messages).toEqual([]);
    expect(out.last!.loaded).toBe(false);
  });

  it('marks the thread read while it has unread replies, and not otherwise', async () => {
    const transport = fakePotatoTransport({ bots: [bot({ unread: 2 })] });
    const out: { last?: ReturnType<typeof usePotatoThread> } = {};
    await act(async () => root.render(<ThreadProbe transport={transport} id="fish" version={1} unread={0} out={out} />));
    expect(transport.calls.some((c) => c.method === 'markRead')).toBe(false);
    await act(async () => root.render(<ThreadProbe transport={transport} id="fish" version={1} unread={2} out={out} />));
    expect(transport.calls.filter((c) => c.method === 'markRead').map((c) => c.args)).toEqual([['fish']]);
  });
});

describe('the remembered thread (pm.potato.open)', () => {
  it('writes and reads the bot id, and clears it', () => {
    expect(readOpenBot()).toBeNull();
    writeOpenBot('fish');
    expect(window.localStorage.getItem('pm.potato.open')).toBe('fish');
    expect(readOpenBot()).toBe('fish');
    writeOpenBot(null);
    expect(readOpenBot()).toBeNull();
  });

  it('reads as none and writes nothing when the store is blocked', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readOpenBot()).toBeNull();
    expect(() => writeOpenBot('fish')).not.toThrow();
  });
});
