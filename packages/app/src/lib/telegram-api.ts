// Typed wrappers for the Telegram commands (`commands/channels.rs`) and the `telegram://changed`
// event. Only the desktop window can call them. The bot token goes IN to `telegramSetToken` and is
// never sent back: the status only has the bot's `@name`.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** What the chat may do. `control` is never offered: the Rust side refuses it too. */
export type TelegramCap = 'watch' | 'answer' | 'type';

/** The status the Settings page shows (`src-tauri/src/channels/status.rs`). */
export type TelegramStatus = {
  storeAvailable: boolean;
  storeLabel: string;
  tokenSet: boolean;
  botUsername: string | null;
  enabled: boolean;
  state: 'off' | 'connecting' | 'on' | 'error';
  error: null | 'token_rejected' | 'conflict' | 'webhook_set' | 'blocked' | 'network';
  paired: null | { username: string | null; firstName: string | null; pairedAtMs: number };
  pairing: null | {
    code: string;
    expiresAtMs: number;
    waitingFor: null | { username: string | null; firstName: string | null };
  };
  caps: TelegramCap[];
  notifyRuns: boolean;
  shortMode: boolean;
  lastPollAtMs: number | null;
  strangersLastHour: number;
};

/** The event carries no data: call `telegramStatus()` again to read the new state. */
export const TELEGRAM_CHANGED_EVENT = 'telegram://changed';

const noop: UnlistenFn = () => {};

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call(command: string, args?: Record<string, unknown>): Promise<TelegramStatus> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<TelegramStatus>(command, args);
}

/** The current status. Safe to call often: it does not talk to Telegram. */
export const telegramStatus = () => call('telegram_status');

/** "Check": Telegram looks at the token, and the store keeps it. Returns the new status. */
export const telegramSetToken = (token: string) => call('telegram_set_token', { token });

/** "Make a code": starts pairing mode. */
export const telegramPairStart = () => call('telegram_pair_start');

/** "Yes, that's me" (`true`) or "Not me" (`false`). */
export const telegramPairDecide = (accept: boolean) => call('telegram_pair_decide', { accept });

/** Turns the chat on or off on the PC. */
export const telegramSetEnabled = (on: boolean) => call('telegram_set_enabled', { on });

/** What the chat may do and how it reports. Rust refuses `control`; the type refuses it too. */
export const telegramSetOptions = (options: { caps: TelegramCap[]; notifyRuns: boolean; shortMode: boolean }) =>
  call('telegram_set_options', { caps: options.caps, notifyRuns: options.notifyRuns, shortMode: options.shortMode });

/** "Disconnect": forgets the token and the chat. */
export const telegramForget = () => call('telegram_forget');

/** Calls `cb` each time the Telegram status changed on the PC. Does nothing outside the desktop app. */
export async function onTelegramChanged(cb: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return noop;
  return listen(TELEGRAM_CHANGED_EVENT, () => cb());
}
