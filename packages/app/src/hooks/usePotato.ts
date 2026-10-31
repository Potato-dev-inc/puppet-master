// Potato Bot's state on the desktop. Read once through the Tauri transport, then read again whenever the app says
// `potato://changed` (a message, a typing row, a mood, a crash) or `settings://changed` (the Labs switch).
// `state` is null when there is no desktop backend (a browser, the tests' harness): nothing shows then.
import { useCallback, useEffect, useRef, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from '../lib/cockpit-api';
import { tauriPotatoTransport, type PotatoMessage, type PotatoState, type PotatoTransport } from '../lib/potato';

export interface PotatoStateHook {
  state: PotatoState | null;
  /** Goes up on every change: a thread reads again when it moves. */
  version: number;
  /** Read again at once (after a command, in case the event is late). */
  refresh: () => void;
}

export function usePotatoState(transport: PotatoTransport = tauriPotatoTransport): PotatoStateHook {
  const [state, setState] = useState<PotatoState | null>(null);
  const [version, setVersion] = useState(0);
  const [tick, setTick] = useState(0);
  // An answer that arrives after a newer one was asked for is dropped.
  const asked = useRef(0);

  useEffect(() => {
    // No desktop shell: there is nothing to ask.
    if (transport === tauriPotatoTransport && !isTauri()) return;
    let live = true;
    const unlisteners: UnlistenFn[] = [];
    const read = () => {
      const mine = ++asked.current;
      transport
        .state()
        .then((next) => {
          if (!live || mine !== asked.current) return;
          setState(next);
          setVersion((n) => n + 1);
        })
        .catch(() => {
          if (live && mine === asked.current) setState(null);
        });
    };
    const keep = (p: Promise<UnlistenFn> | undefined) =>
      p?.then((fn) => (live ? unlisteners.push(fn) : fn())).catch(() => {});
    read();
    keep(transport.onChanged?.(() => read()));
    if (transport === tauriPotatoTransport) keep(listen('settings://changed', () => read()));
    return () => {
      live = false;
      unlisteners.forEach((fn) => fn());
    };
  }, [transport, tick]);

  const refresh = useCallback(() => setTick((n) => n + 1), []);
  return { state, version, refresh };
}

export interface PotatoThread {
  messages: PotatoMessage[];
  /** False until the first read of this bot's thread is back. */
  loaded: boolean;
}

/** One bot's app thread. Read again whenever `version` moves, and marked read while it is open. */
export function usePotatoThread(transport: PotatoTransport, botId: string | null, version: number, unread: number): PotatoThread {
  const [thread, setThread] = useState<{ botId: string | null; messages: PotatoMessage[] }>({ botId: null, messages: [] });
  const asked = useRef(0);

  useEffect(() => {
    if (!botId) return;
    const mine = ++asked.current;
    let live = true;
    transport
      .messages(botId, 200)
      .then((messages) => {
        if (live && mine === asked.current) setThread({ botId, messages });
      })
      .catch(() => {
        if (live && mine === asked.current) setThread({ botId, messages: [] });
      });
    return () => {
      live = false;
    };
  }, [transport, botId, version]);

  // Opening the thread reads it: an unread reply that arrives while it is open is read at once.
  useEffect(() => {
    if (botId && unread > 0) void transport.markRead(botId).catch(() => {});
  }, [transport, botId, unread]);

  const mine = thread.botId === botId;
  return { messages: mine ? thread.messages : [], loaded: mine };
}

const OPEN_KEY = 'pm.potato.open';

/** The bot whose thread was open last time (spec section 4). A broken or blocked store reads as none. */
export function readOpenBot(): string | null {
  try {
    return window.localStorage.getItem(OPEN_KEY) || null;
  } catch {
    return null;
  }
}

export function writeOpenBot(id: string | null): void {
  try {
    if (id) window.localStorage.setItem(OPEN_KEY, id);
    else window.localStorage.removeItem(OPEN_KEY);
  } catch {
    // The remembered thread is a convenience only.
  }
}
