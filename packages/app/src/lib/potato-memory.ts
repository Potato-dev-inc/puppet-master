// Memory of a Potato Bot (the bot panel's Advanced tab): the notes in the bot's own vault, read-only, with Delete. Desktop
// only: the Tauri commands `potato_memory_*` in src-tauri/src/commands/potato_panel.rs. The paths come from the list
// and the Rust side checks them again. `fakeMemoryTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';

export interface MemoryNote {
  /** Relative to the bot's vault, with `/`: `journal/2026/10/2026-10-28.md`. */
  path: string;
  bytes: number;
  modifiedMs: number;
}

export interface MemoryTransport {
  list(botId: string): Promise<MemoryNote[]>;
  read(botId: string, path: string): Promise<string>;
  /** Moves the note to Recently deleted; resolves with the trash id (for Undo). */
  remove(botId: string, path: string): Promise<string>;
}

export const tauriMemoryTransport: MemoryTransport = {
  list: (botId) => invoke<MemoryNote[]>('potato_memory_list', { botId }),
  read: (botId, path) => invoke<string>('potato_memory_read', { botId, path }),
  remove: (botId, path) => invoke<string>('potato_memory_delete', { botId, path }),
};

/** The title of a note: its file name without `.md`. */
export function noteTitle(path: string): string {
  const name = path.split('/').pop() ?? path;
  return name.replace(/\.md$/, '');
}

export interface FakeMemorySeed {
  notes?: { path: string; body?: string; modifiedMs?: number }[];
  /** When set, every call fails with this sentence. */
  fail?: string;
}

export interface FakeMemoryTransport extends MemoryTransport {
  readonly calls: { method: keyof MemoryTransport; args: unknown[] }[];
}

export function fakeMemoryTransport(seed: FakeMemorySeed = {}): FakeMemoryTransport {
  const notes = (seed.notes ?? []).map((n) => ({ path: n.path, body: n.body ?? '', modifiedMs: n.modifiedMs ?? 1_700_000_000_000 }));
  const calls: FakeMemoryTransport['calls'] = [];
  const run = <T>(method: keyof MemoryTransport, args: unknown[], body: () => T): Promise<T> => {
    calls.push({ method, args });
    if (seed.fail) return Promise.reject(new Error(seed.fail));
    try {
      return Promise.resolve(body());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const find = (path: string) => {
    const note = notes.find((n) => n.path === path);
    if (!note) throw new Error("That note is not in this bot's memory.");
    return note;
  };
  return {
    calls,
    list: (botId) =>
      run('list', [botId], () =>
        [...notes]
          .sort((a, b) => b.modifiedMs - a.modifiedMs || a.path.localeCompare(b.path))
          .map((n) => ({ path: n.path, bytes: n.body.length, modifiedMs: n.modifiedMs })),
      ),
    read: (botId, path) => run('read', [botId, path], () => find(path).body),
    remove: (botId, path) =>
      run('remove', [botId, path], () => {
        find(path);
        notes.splice(
          notes.findIndex((n) => n.path === path),
          1,
        );
        return `trash-${path}`;
      }),
  };
}
