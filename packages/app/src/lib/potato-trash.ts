// Recently deleted (UX-2): what Potato Bot deleted in the last 7 days (a memory note, a routine or a whole bot), and
// "Bring back" / "Undo". Desktop only: the Tauri commands `potato_trash_*` in src-tauri/src/commands/potato_trash.rs.
// `fakeTrashTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from './cockpit-api';

export type TrashKind = 'note' | 'routine' | 'bot';

export interface TrashItem {
  id: string;
  kind: TrashKind;
  /** The bot the item belonged to. */
  bot: string;
  /** The note's name, the routine's title or the bot's name. */
  title: string;
  /** When it was deleted (epoch ms). It is purged 7 days later. */
  deletedAt: number;
}

export interface TrashTransport {
  list(): Promise<TrashItem[]>;
  restore(id: string): Promise<void>;
}

/** How long "Undo" stays on screen after a delete. */
export const UNDO_MS = 10_000;

/** Days the item has left before it is purged (never below 1 for an item still listed). */
export const TRASH_DAYS = 7;

const DAY_MS = 24 * 60 * 60 * 1000;

export function daysLeft(item: TrashItem, now: number): number {
  return Math.max(1, Math.ceil((item.deletedAt + TRASH_DAYS * DAY_MS - now) / DAY_MS));
}

const NOT_ON_DESKTOP = 'Recently deleted needs the Puppet Master desktop app.';

export const tauriTrashTransport: TrashTransport = {
  list: () => (isTauri() ? invoke<TrashItem[]>('potato_trash_list') : Promise.resolve([])),
  restore: (id) => (isTauri() ? invoke<void>('potato_trash_restore', { id }) : Promise.reject(new Error(NOT_ON_DESKTOP))),
};

export interface FakeTrashTransport extends TrashTransport {
  readonly calls: { method: keyof TrashTransport; args: unknown[] }[];
  /** The items the fake holds, after the calls so far. */
  readonly items: TrashItem[];
}

/** An in-memory `TrashTransport`. `fail` makes every call reject with that message. A restore drops the item. */
export function fakeTrashTransport(seed: TrashItem[] = [], fail?: string): FakeTrashTransport {
  const items = [...seed];
  const calls: FakeTrashTransport['calls'] = [];
  const run = <T,>(method: keyof TrashTransport, args: unknown[], work: () => T): Promise<T> => {
    calls.push({ method, args });
    if (fail) return Promise.reject(new Error(fail));
    return Promise.resolve().then(work);
  };
  return {
    calls,
    items,
    list: () => run('list', [], () => items.map((i) => ({ ...i }))),
    restore: (id) =>
      run('restore', [id], () => {
        const at = items.findIndex((i) => i.id === id);
        if (at < 0) throw new Error('That item is no longer in Recently deleted.');
        items.splice(at, 1);
      }),
  };
}
