// Potato Bot on the desktop (spec section 4). The names mirror the Rust serde shapes in
// src-tauri/src/potato/{service,store}.rs and src-tauri/src/commands/potato.rs, in camelCase. Desktop only:
// the page talks to Tauri commands and to the `potato://changed` event; the phone and the bridge never see this.
// Nothing here draws. `fakePotatoTransport` is the in-memory twin for tests (the pattern of `fakeCompanionTransport`).
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from './cockpit-api';

/** The two voices of a bot. */
export type TalkPersona = 'fish' | 'potato';

/** What the page shows for a bot's brain (`BrainState` in service.rs). */
export type BrainState =
  | 'off'
  | 'idle'
  | 'starting'
  | 'ready'
  | 'restarting'
  | 'stopped'
  | 'needs_key'
  | 'needs_node'
  | 'missing_program'
  | 'daily_limit'
  | 'agents';

export type BrainKind = 'deepseek' | 'go' | 'agents';

export interface MoodNow {
  mood: string;
  intensity: number;
  energy: number;
  hunger: number;
}

export interface PotatoBotView {
  id: string;
  name: string;
  talkPersona: TalkPersona;
  brainState: BrainState;
  /** The last error code the brain reported and that nothing has cleared since. */
  lastError: string | null;
  typing: boolean;
  mood: MoodNow | null;
  provider: string | null;
  unread: number;
  lastLine: string | null;
  lastAt: number | null;
  usedTodayNano: number;
  limitCents: number;
  openCards: number;
}

export interface PotatoSettings {
  v: number;
  brain: BrainKind;
  defaultPersona: TalkPersona;
  dailyLimitCents: number;
}

export interface PotatoState {
  /** The Labs switch `potato_bot_enabled`. Off: no row, no palette entry, no page. */
  enabled: boolean;
  settings: PotatoSettings;
  key: { set: boolean; last4: string | null };
  node: { ready: boolean; found: string | null };
  bots: PotatoBotView[];
}

export type FromKind = 'you' | 'bot' | 'person' | 'note';
export type MessageKind = 'text' | 'helper_card' | 'helper_result' | 'note';

export interface PotatoMessage {
  id: string;
  at: number;
  from: { kind: FromKind; name?: string };
  kind: MessageKind;
  text: string;
  mood?: string;
  requestId?: string;
  /** A helper card: the folder name the bot asked for (a name only). */
  project?: string;
  /** A helper result row: the pane the helper worked in, for its Open button. */
  paneId?: string;
}

export interface PotatoBot {
  id: string;
  talkPersona: TalkPersona;
  createdMs: number;
}

export interface SendOutcome {
  id: string;
  state: BrainState;
  delivered: boolean;
}

/** What Start answers: the project the helper works in (a name). */
export interface HelperStarted {
  folder: string;
}

export interface PotatoTransport {
  state(): Promise<PotatoState>;
  messages(botId: string, limit?: number): Promise<PotatoMessage[]>;
  send(botId: string, text: string): Promise<SendOutcome>;
  addBot(botId: string): Promise<PotatoBot>;
  setPersona(botId: string, pack: TalkPersona): Promise<PotatoBot>;
  startBot(botId: string): Promise<void>;
  stopBot(botId: string): Promise<void>;
  markRead(botId: string): Promise<void>;
  /** "Change folder": the names of the recent projects a helper may work in (names, never paths). */
  helperFolders(botId: string): Promise<string[]>;
  /** Start on a helper card. `folder` is a project name from `helperFolders`; without it the card's own folder is used. */
  helperStart(botId: string, requestId: string, folder?: string): Promise<HelperStarted>;
  /** Not now on a helper card. */
  helperDecline(botId: string, requestId: string): Promise<void>;
  /** "Install Node.js": the agent installer's `node` tool, in a window the person watches. */
  installNode(): Promise<void>;
  /** Settings: the saved settings (`<app data>/potato/settings.json`). The key is never in them. */
  settingsGet(): Promise<PotatoSettings>;
  settingsSet(settings: PotatoSettings): Promise<PotatoSettings>;
  /** Settings > Check key: one tiny turn. Answers "Works." or rejects with one plain sentence. */
  checkKey(): Promise<string>;
  /** `potato://changed`: something about this bot changed (`null`: the Labs switch moved). */
  onChanged?(cb: (botId: string | null) => void): Promise<UnlistenFn>;
}

/** `CODE: A plain sentence.` from a Tauri command, shown as the sentence only. */
export function plainError(error: unknown, fallback: string): string {
  const raw = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  const text = raw.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '').trim();
  return text || fallback;
}

export const tauriPotatoTransport: PotatoTransport = {
  state: () => invoke<PotatoState>('potato_state'),
  messages: (botId, limit) => invoke<PotatoMessage[]>('potato_messages', { botId, limit: limit ?? null }),
  send: (botId, text) => invoke<SendOutcome>('potato_send', { botId, text }),
  addBot: (botId) => invoke<PotatoBot>('potato_bot_add', { botId }),
  setPersona: (botId, pack) => invoke<PotatoBot>('potato_set_persona', { botId, pack }),
  startBot: (botId) => invoke<void>('potato_bot_start', { botId }),
  stopBot: (botId) => invoke<void>('potato_bot_stop', { botId }),
  markRead: (botId) => invoke<void>('potato_mark_read', { botId }),
  helperFolders: (botId) => invoke<string[]>('potato_helper_folders', { botId }),
  helperStart: (botId, requestId, folder) => invoke<HelperStarted>('potato_helper_start', { botId, requestId, folder: folder ?? null }),
  helperDecline: (botId, requestId) => invoke<void>('potato_helper_decline', { botId, requestId }),
  installNode: async () => {
    await invoke('potato_install_node');
  },
  settingsGet: () => invoke<PotatoSettings>('potato_settings_get'),
  settingsSet: (settings) => invoke<PotatoSettings>('potato_settings_set', { settings }),
  checkKey: () => invoke<string>('potato_check_key'),
  onChanged: (cb) => {
    if (!isTauri()) return Promise.resolve((() => {}) as UnlistenFn);
    return listen<{ botId?: string } | null>('potato://changed', (e) => cb(e.payload?.botId ?? null));
  },
};

// ---- copy ---------------------------------------------------------------------------------------

export const PERSONA_LABEL: Record<TalkPersona, string> = { fish: 'Fish', potato: 'Potato' };

const BRAIN_LABEL: Record<BrainKind, string> = { deepseek: 'DeepSeek', go: 'OpenCode Go', agents: 'Through my agents' };

export function brainLabel(kind: BrainKind): string {
  return BRAIN_LABEL[kind] ?? BRAIN_LABEL.deepseek;
}

const STATE_WORD: Record<BrainState, string> = {
  off: 'Off',
  idle: 'Idle',
  starting: 'Starting',
  ready: 'Running',
  restarting: 'Restarting',
  stopped: 'Stopped',
  needs_key: 'Needs a key',
  needs_node: 'Needs Node.js',
  missing_program: 'Not installed',
  daily_limit: 'Limit reached',
  agents: 'Through my agents',
};

export function brainStateWord(state: BrainState): string {
  return STATE_WORD[state] ?? '';
}

/** Cents as dollars: `$0.50`. */
export function dollars(cents: number): string {
  return `$${(Math.max(0, cents) / 100).toFixed(2)}`;
}

/** `Genki`: the mood word with a capital letter. */
export function moodWord(mood: string): string {
  return mood ? mood.charAt(0).toUpperCase() + mood.slice(1) : '';
}

/** `Genki · energy 72`: text only (spec section 4). */
export function moodLine(mood: MoodNow): string {
  return `${moodWord(mood.mood)} · energy ${Math.round(mood.energy)}`;
}

export const COPY = {
  emptyList: 'No bots yet. Make one to start chatting.',
  composer: (name: string) => `Message ${name}`,
  typing: (name: string) => `${name} is typing…`,
  limit: (name: string, cents: number) => `${name} has used today's limit (${dollars(cents)}). Raise it in Settings or wait until tomorrow.`,
  stopped: (name: string) => `${name}'s brain stopped.`,
  noKey: (name: string) => `Add your DeepSeek key in Settings to talk with ${name}.`,
  needsNode: 'Potato Bot needs Node.js 20 or newer.',
  agentsOnly: 'Mood and personas need the DeepSeek or OpenCode Go brain.',
  missingProgram: "Potato Bot's brain is missing from this install. Reinstall Puppet Master to get it back.",
};

/** The sentence for the last error code the brain reported, or null when it is not worth a card. */
export function errorSentence(name: string, code: string | null): string | null {
  switch (code) {
    case 'PROVIDER_AUTH':
      return `${name} could not use the model key. Check it in Settings.`;
    case 'PROVIDER_LIMIT':
      return 'The model is out of balance or too busy. Try again later.';
    case 'PROVIDER_DOWN':
      return 'The model is not answering right now. Try again in a minute.';
    case 'NETWORK':
      return `${name} could not reach the internet.`;
    case 'PERSONA_TOO_BIG':
      return `${name}'s persona text is too long.`;
    case 'SURFACE_AUTH':
    case 'INTERNAL':
      return `Something went wrong inside ${name}'s brain.`;
    default:
      return null;
  }
}

/** Today: the clock. Another day: the short date. */
export function formatWhen(at: number, now: number): string {
  const when = new Date(at);
  const today = new Date(now);
  if (when.toDateString() === today.toDateString()) {
    return when.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
  }
  return when.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

// ---- the fake -----------------------------------------------------------------------------------

export interface FakePotatoSeed {
  state?: Partial<PotatoState>;
  bots?: PotatoBotView[];
  messages?: Record<string, PotatoMessage[]>;
  /** What `send` answers. Default: delivered. */
  outcome?: Partial<SendOutcome>;
  /** The recent project names "Change folder" offers. */
  folders?: string[];
  /** What Check key answers: a sentence, or throws one. */
  keyCheck?: () => string;
}

export interface FakePotatoTransport extends PotatoTransport {
  /** Every call so far, in order: the method name and its arguments. */
  readonly calls: { method: keyof PotatoTransport; args: unknown[] }[];
  /** Fires the `potato://changed` listeners, as the app does after the brain moved. */
  emit(botId?: string | null): void;
  /** Changes a bot or the state the way the app would, then fires the listeners. */
  patchBot(botId: string, patch: Partial<PotatoBotView>): void;
  setEnabled(on: boolean): void;
  addMessage(botId: string, message: PotatoMessage): void;
}

let nextId = 1;

export function fakePotatoTransport(seed: FakePotatoSeed = {}): FakePotatoTransport {
  const state: PotatoState = {
    enabled: true,
    settings: { v: 1, brain: 'deepseek', defaultPersona: 'fish', dailyLimitCents: 50 },
    key: { set: true, last4: null },
    node: { ready: true, found: null },
    bots: (seed.bots ?? []).map((b) => ({ ...b })),
    ...seed.state,
  };
  const threads = new Map<string, PotatoMessage[]>(Object.entries(seed.messages ?? {}).map(([id, list]) => [id, list.map((m) => ({ ...m }))]));
  const listeners = new Set<(botId: string | null) => void>();
  const calls: FakePotatoTransport['calls'] = [];
  const log = <T>(method: keyof PotatoTransport, args: unknown[], run: () => T): Promise<T> => {
    calls.push({ method, args });
    try {
      return Promise.resolve(run());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const find = (id: string): PotatoBotView => {
    const bot = state.bots.find((b) => b.id === id);
    if (!bot) throw new Error('No bot has that name.');
    return bot;
  };
  const emit = (botId: string | null = null) => listeners.forEach((cb) => cb(botId));
  const thread = (id: string): PotatoMessage[] => {
    let list = threads.get(id);
    if (!list) threads.set(id, (list = []));
    return list;
  };
  const view = (): PotatoState => ({ ...state, bots: state.bots.map((b) => ({ ...b })) });
  const message = (from: PotatoMessage['from'], kind: MessageKind, text: string): PotatoMessage => ({ id: `m-fake-${nextId++}`, at: Date.now(), from, kind, text });

  return {
    calls,
    emit,
    patchBot(botId, patch) {
      Object.assign(find(botId), patch);
      emit(botId);
    },
    setEnabled(on) {
      state.enabled = on;
      emit(null);
    },
    addMessage(botId, m) {
      thread(botId).push({ ...m });
      emit(botId);
    },
    state: () => log('state', [], view),
    messages: (botId, limit) => log('messages', [botId, limit], () => thread(botId).slice(-(limit ?? 200)).map((m) => ({ ...m }))),
    send: (botId, text) =>
      log('send', [botId, text], () => {
        const bot = find(botId);
        const saved = message({ kind: 'you' }, 'text', text.trim());
        thread(botId).push(saved);
        bot.lastLine = saved.text;
        bot.lastAt = saved.at;
        emit(botId);
        return { id: saved.id, state: bot.brainState, delivered: true, ...seed.outcome };
      }),
    addBot: (botId) =>
      log('addBot', [botId], () => {
        if (!state.bots.some((b) => b.id === botId)) {
          state.bots.push({
            id: botId, name: moodWord(botId), talkPersona: state.settings.defaultPersona, brainState: 'idle', lastError: null, typing: false, mood: null, provider: null,
            unread: 0, lastLine: null, lastAt: null, usedTodayNano: 0, limitCents: state.settings.dailyLimitCents, openCards: 0,
          });
        }
        emit(botId);
        return { id: botId, talkPersona: find(botId).talkPersona, createdMs: Date.now() };
      }),
    setPersona: (botId, pack) =>
      log('setPersona', [botId, pack], () => {
        const bot = find(botId);
        if (bot.talkPersona !== pack) {
          bot.talkPersona = pack;
          thread(botId).push(message({ kind: 'note' }, 'note', `Now talking as ${PERSONA_LABEL[pack]}.`));
        }
        emit(botId);
        return { id: botId, talkPersona: pack, createdMs: 0 };
      }),
    startBot: (botId) =>
      log('startBot', [botId], () => {
        find(botId).brainState = 'ready';
        emit(botId);
      }),
    stopBot: (botId) =>
      log('stopBot', [botId], () => {
        find(botId).brainState = 'idle';
        emit(botId);
      }),
    markRead: (botId) =>
      log('markRead', [botId], () => {
        find(botId).unread = 0;
      }),
    helperFolders: (botId) => log('helperFolders', [botId], () => [...(seed.folders ?? ['demo-a', 'demo-b'])]),
    helperStart: (botId, requestId, folder) =>
      log('helperStart', [botId, requestId, folder], () => {
        const card = thread(botId).find((m) => m.kind === 'helper_card' && m.requestId === requestId);
        const chosen = folder ?? card?.project;
        if (!chosen) throw new Error('Choose a folder for the helper first.');
        thread(botId).push({ ...message({ kind: 'note' }, 'helper_result', `Started a helper in ${chosen}.`), requestId });
        emit(botId);
        return { folder: chosen };
      }),
    helperDecline: (botId, requestId) =>
      log('helperDecline', [botId, requestId], () => {
        thread(botId).push({ ...message({ kind: 'note' }, 'helper_result', 'Not now.'), requestId });
        emit(botId);
      }),
    installNode: () => log('installNode', [], () => undefined),
    settingsGet: () => log('settingsGet', [], () => ({ ...state.settings })),
    settingsSet: (settings) =>
      log('settingsSet', [settings], () => {
        state.settings = { ...settings, dailyLimitCents: Math.min(2000, Math.max(5, Math.round(settings.dailyLimitCents))) };
        emit(null);
        return { ...state.settings };
      }),
    checkKey: () => log('checkKey', [], () => (seed.keyCheck ? seed.keyCheck() : 'Works.')),
    onChanged: (cb) => {
      listeners.add(cb);
      return Promise.resolve((() => listeners.delete(cb)) as UnlistenFn);
    },
  };
}
