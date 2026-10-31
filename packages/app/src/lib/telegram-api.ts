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

// ---- Tag Your Agent: the bot answering when it is tagged in an approved group ----------------------------
// The same rules as above: only the desktop window can call these, nothing here returns the token, and a group
// is approved only by the person on this PC.

/** Who may tag the bot in a group: only the connected person, or anyone (with a per-person hourly cap). */
export type TagWho = 'only_me' | 'anyone';

/** A group the person approved. `agent` and `folder` are optional: `null` means any agent / a neutral empty folder. */
export type TagGroup = {
  chatId: number;
  title: string;
  who: TagWho;
  agent: string | null;
  folder: string | null;
  style: string;
};

/** A group that tagged the bot and waits for a yes. Only its id and title are kept. */
export type TagPending = { chatId: number; title: string; seenAtMs: number };

/** One question the bot answered (or could not) in a group. */
export type TagAnswer = {
  atMs: number;
  groupTitle: string;
  asker: string;
  question: string;
  answer: string | null;
  agent: string;
  ok: boolean;
};

export type TagAgent = { id: string; label: string };

/** What the Tag Your Agent block shows (`src-tauri/src/channels/setup_tag.rs`). */
export type TagBotView = {
  paired: boolean;
  botUsername: string | null;
  enabled: boolean;
  dailyCap: number;
  perPersonCap: number;
  groups: TagGroup[];
  pending: TagPending[];
  agents: TagAgent[];
  /** Newest first. */
  recent: TagAnswer[];
};

/** The fields of an approved group the person can change. */
export type TagGroupUpdate = { who: TagWho; agent: string | null; folder: string | null; style: string };

async function callTag(command: string, args?: Record<string, unknown>): Promise<TagBotView> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<TagBotView>(command, args);
}

export const telegramTagBot = () => callTag('telegram_tag_bot');
export const telegramTagSetEnabled = (on: boolean) => callTag('telegram_tag_set_enabled', { on });
export const telegramTagSetCaps = (dailyCap: number, perPersonCap: number) =>
  callTag('telegram_tag_set_caps', { dailyCap, perPersonCap });
export const telegramTagApprove = (chatId: number) => callTag('telegram_tag_approve', { chatId });
export const telegramTagIgnore = (chatId: number) => callTag('telegram_tag_ignore', { chatId });
export const telegramTagUpdateGroup = (chatId: number, update: TagGroupUpdate) =>
  callTag('telegram_tag_update_group', { chatId, update });
export const telegramTagRemove = (chatId: number) => callTag('telegram_tag_remove', { chatId });
