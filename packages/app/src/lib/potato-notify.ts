// "Notify me" of a Potato Bot (the bot panel's Advanced tab): when it replies, or never. (`needs_you` is stored by the engine
// but not offered: nothing sends that push yet.) The choice is
// stored per bot (src-tauri/src/potato/notify.rs). Desktop only: the Tauri commands `potato_notify_*`.
// `fakeNotifyTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';

export type NotifyChoice = 'reply' | 'needs_you' | 'never';

export const NOTIFY_CHOICES: readonly { id: NotifyChoice; label: string }[] = [
  { id: 'reply', label: 'When it replies' },
  { id: 'never', label: 'Never' },
];

export interface NotifyTransport {
  get(botId: string): Promise<NotifyChoice>;
  set(botId: string, value: NotifyChoice): Promise<NotifyChoice>;
}

export const tauriNotifyTransport: NotifyTransport = {
  get: (botId) => invoke<NotifyChoice>('potato_notify_get', { botId }),
  set: (botId, value) => invoke<NotifyChoice>('potato_notify_set', { botId, value }),
};

export interface FakeNotifySeed {
  /** The saved choice per bot. A bot with no entry says `reply`, the default. */
  choices?: Record<string, NotifyChoice>;
  /** When set, every call fails with this sentence. */
  fail?: string;
}

export interface FakeNotifyTransport extends NotifyTransport {
  readonly calls: { method: keyof NotifyTransport; args: unknown[] }[];
}

export function fakeNotifyTransport(seed: FakeNotifySeed = {}): FakeNotifyTransport {
  const choices = new Map<string, NotifyChoice>(Object.entries(seed.choices ?? {}));
  const calls: FakeNotifyTransport['calls'] = [];
  const run = <T>(method: keyof NotifyTransport, args: unknown[], body: () => T): Promise<T> => {
    calls.push({ method, args });
    if (seed.fail) return Promise.reject(new Error(seed.fail));
    return Promise.resolve(body());
  };
  return {
    calls,
    get: (botId) => run('get', [botId], () => choices.get(botId) ?? 'reply'),
    set: (botId, value) =>
      run('set', [botId, value], () => {
        choices.set(botId, value);
        return value;
      }),
  };
}
