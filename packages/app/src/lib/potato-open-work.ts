// Open work of a Potato Bot (the bot panel's Advanced tab): the list of things the bot promised to do, and the helper
// jobs still running for it (read-only: they end by themselves). Desktop only: the Tauri commands `potato_open_work_*`
// in src-tauri/src/commands/potato_panel.rs. `fakeOpenWorkTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';

export interface OpenWorkItem {
  id: string;
  text: string;
  done: boolean;
}

export interface OpenWorkHelper {
  requestId: string;
  /** The project's name (never a path). */
  project: string;
  startedMs: number;
}

export interface OpenWorkView {
  items: OpenWorkItem[];
  helpers: OpenWorkHelper[];
}

export interface OpenWorkTransport {
  list(botId: string): Promise<OpenWorkView>;
  add(botId: string, text: string): Promise<OpenWorkItem>;
  setDone(botId: string, id: string, done: boolean): Promise<OpenWorkItem>;
  remove(botId: string, id: string): Promise<void>;
}

export const tauriOpenWorkTransport: OpenWorkTransport = {
  list: (botId) => invoke<OpenWorkView>('potato_open_work_list', { botId }),
  add: (botId, text) => invoke<OpenWorkItem>('potato_open_work_add', { botId, text }),
  setDone: (botId, id, done) => invoke<OpenWorkItem>('potato_open_work_set_done', { botId, id, done }),
  remove: (botId, id) => invoke<void>('potato_open_work_remove', { botId, id }),
};

export interface FakeOpenWorkSeed {
  items?: OpenWorkItem[];
  helpers?: OpenWorkHelper[];
  /** When set, every call fails with this sentence (as the Rust side would say it). */
  fail?: string;
}

export interface FakeOpenWorkTransport extends OpenWorkTransport {
  /** Every call so far: the method and its arguments. */
  readonly calls: { method: keyof OpenWorkTransport; args: unknown[] }[];
}

let nextId = 1;

export function fakeOpenWorkTransport(seed: FakeOpenWorkSeed = {}): FakeOpenWorkTransport {
  const items = (seed.items ?? []).map((i) => ({ ...i }));
  const helpers = (seed.helpers ?? []).map((h) => ({ ...h }));
  const calls: FakeOpenWorkTransport['calls'] = [];
  const run = <T>(method: keyof OpenWorkTransport, args: unknown[], body: () => T): Promise<T> => {
    calls.push({ method, args });
    if (seed.fail) return Promise.reject(new Error(seed.fail));
    try {
      return Promise.resolve(body());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const find = (id: string) => {
    const item = items.find((i) => i.id === id);
    if (!item) throw new Error('That item is not on the list any more.');
    return item;
  };
  return {
    calls,
    list: (botId) => run('list', [botId], () => ({ items: items.map((i) => ({ ...i })), helpers: helpers.map((h) => ({ ...h })) })),
    add: (botId, text) =>
      run('add', [botId, text], () => {
        const clean = text.split(/\s+/).filter(Boolean).join(' ').slice(0, 200);
        if (!clean) throw new Error('Write what the bot should do first.');
        const item = { id: `ow-fake${String(nextId++).padStart(4, '0')}`, text: clean, done: false };
        items.push(item);
        return { ...item };
      }),
    setDone: (botId, id, done) =>
      run('setDone', [botId, id, done], () => {
        const item = find(id);
        item.done = done;
        return { ...item };
      }),
    remove: (botId, id) =>
      run('remove', [botId, id], () => {
        find(id);
        items.splice(
          items.findIndex((i) => i.id === id),
          1,
        );
      }),
  };
}
