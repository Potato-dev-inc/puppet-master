// The Computer panel's side of Potato Bot (the sandbox screen in a bot's chat). The names mirror the Rust serde shapes in
// src-tauri/src/potato/sandbox/screen.rs and src-tauri/src/commands/potato_screen.rs, in camelCase. Desktop only: the
// panel talks to Tauri commands and to the `potato://screen` event. `fakeScreenTransport` is the in-memory twin for tests.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauri } from './cockpit-api';

/** `view` shows the screen; `control` lets the person use the mouse and keyboard (only after Take over). */
export type ScreenMode = 'view' | 'control';

/** Who has the screen: nobody, a bot, the person, or a bot that asked the person to step in. */
export type ScreenPhase = 'free' | 'bot' | 'you' | 'needs_you';

export interface ScreenState {
  /** Both Labs switches (`potato_bot_enabled` and `potato_sandbox_enabled`) are on. Off: the panel is not drawn at all. */
  enabled: boolean;
  phase: ScreenPhase;
  /** The bot that has it, or the one that asked. */
  bot: string | null;
  cardId: string | null;
  /** The bot's reason on a "Needs you" card (already cleaned). */
  reason: string | null;
  cardExpiresMs: number | null;
}

export interface ScreenLink {
  /** The iframe address. The screen password is in its fragment: pass it to the iframe and to nothing else. */
  src: string;
  mode: ScreenMode;
  /** When the link runs out (epoch milliseconds). */
  expiresMs: number;
}

export interface ScreenTransport {
  state(): Promise<ScreenState>;
  link(botId: string, mode: ScreenMode): Promise<ScreenLink>;
  /** "Take over": the person takes the screen. */
  takeOver(): Promise<ScreenState>;
  /** "Hand back": the person is done. */
  handBack(): Promise<ScreenState>;
  /** "Skip" on a "Needs you" card. */
  skip(cardId: string): Promise<ScreenState>;
  /** The person is still working in the control view: keeps the lease for 15 more minutes. */
  keep(): Promise<void>;
  /** `potato://screen`: read the state again. */
  onChanged?(cb: () => void): Promise<UnlistenFn>;
}

export const tauriScreenTransport: ScreenTransport = {
  state: () => invoke<ScreenState>('potato_screen_state'),
  link: (botId, mode) => invoke<ScreenLink>('potato_screen_link', { botId, mode }),
  takeOver: () => invoke<ScreenState>('potato_screen_take_over'),
  handBack: () => invoke<ScreenState>('potato_screen_hand_back'),
  skip: (cardId) => invoke<ScreenState>('potato_screen_skip', { cardId }),
  keep: async () => {
    await invoke('potato_screen_keep');
  },
  onChanged: (cb) => {
    if (!isTauri()) return Promise.resolve((() => {}) as UnlistenFn);
    return listen('potato://screen', () => cb());
  },
};

// ---- what the viewer page tells its parent ------------------------------------------------------

/** The fixed message type of the viewer page (packages/potato-sandbox/viewer/README.md). */
export const SCREEN_STATUS_TYPE = 'pm-sbx-screen-status';

export type ViewerStatus = 'loading' | 'connecting' | 'connected' | 'disconnected' | 'failed';
export type ViewerReason = 'load' | 'timeout' | 'security' | 'closed' | 'error';

const STATUSES: readonly ViewerStatus[] = ['loading', 'connecting', 'connected', 'disconnected', 'failed'];
const REASONS: readonly ViewerReason[] = ['load', 'timeout', 'security', 'closed', 'error'];

export interface ViewerMessage {
  status: ViewerStatus;
  reason: ViewerReason | null;
}

/** The status a viewer message carries, or null when it is not one (wrong type, an unknown word, not an object). */
export function parseViewerMessage(data: unknown): ViewerMessage | null {
  if (typeof data !== 'object' || data === null) return null;
  const { type, status, reason } = data as { type?: unknown; status?: unknown; reason?: unknown };
  if (type !== SCREEN_STATUS_TYPE) return null;
  if (typeof status !== 'string' || !STATUSES.includes(status as ViewerStatus)) return null;
  const known = typeof reason === 'string' && REASONS.includes(reason as ViewerReason) ? (reason as ViewerReason) : null;
  return { status: status as ViewerStatus, reason: known };
}

/** The origin a link's address belongs to (the only origin its messages are believed from), or null. */
export function originOf(src: string, base: string = typeof location === 'undefined' ? 'http://localhost' : location.href): string | null {
  try {
    const url = new URL(src, base);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.origin : null;
  } catch {
    return null;
  }
}

// ---- copy ---------------------------------------------------------------------------------------

/** The header words. `botId` is the bot whose chat the panel is in. */
export function headerWords(state: ScreenState, botId: string, botName: string): string {
  const mine = state.bot === botId;
  switch (state.phase) {
    case 'you':
      return 'You have control';
    case 'needs_you':
      return mine ? 'Needs you' : 'Another bot needs you';
    case 'bot':
      return mine ? `${botName} is using it` : 'Another bot is using it';
    default:
      return 'Not in use';
  }
}

/** The line under the screen while it is not showing. */
export function viewerNote(status: ViewerStatus, reason: ViewerReason | null): string | null {
  switch (status) {
    case 'loading':
    case 'connecting':
      return 'Connecting…';
    case 'connected':
      return null;
    case 'disconnected':
      return 'The connection closed.';
    default:
      switch (reason) {
        case 'timeout':
          return 'It took too long to connect.';
        case 'security':
          return 'The screen did not accept the link.';
        case 'load':
          return 'The screen viewer could not load.';
        default:
          return 'Could not connect to the screen.';
      }
  }
}

// ---- the fake -----------------------------------------------------------------------------------

export interface FakeScreenTransport extends ScreenTransport {
  readonly calls: { method: keyof ScreenTransport; args: unknown[] }[];
  /** Changes the state the way the app would, then fires the listeners. */
  set(patch: Partial<ScreenState>): void;
  emit(): void;
  /** The next call of `method` rejects with this text (once). */
  failNext(method: keyof ScreenTransport, message: string): void;
}

export const FAKE_ORIGIN = 'http://127.0.0.1:41999';

export function fakeScreenTransport(seed: Partial<ScreenState> = {}): FakeScreenTransport {
  let state: ScreenState = { enabled: true, phase: 'free', bot: null, cardId: null, reason: null, cardExpiresMs: null, ...seed };
  const listeners = new Set<() => void>();
  const calls: FakeScreenTransport['calls'] = [];
  const failures = new Map<string, string>();
  let links = 0;
  const emit = () => listeners.forEach((cb) => cb());
  const log = <T>(method: keyof ScreenTransport, args: unknown[], run: () => T): Promise<T> => {
    calls.push({ method, args });
    const failure = failures.get(method);
    if (failure !== undefined) {
      failures.delete(method);
      return Promise.reject(new Error(failure));
    }
    try {
      return Promise.resolve(run());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const move = (patch: Partial<ScreenState>) => {
    state = { ...state, ...patch };
    return { ...state };
  };
  return {
    calls,
    set(patch) {
      move(patch);
      emit();
    },
    emit,
    failNext: (method, message) => void failures.set(method, message),
    state: () => log('state', [], () => ({ ...state })),
    link: (botId, mode) =>
      log('link', [botId, mode], () => ({
        src: `${FAKE_ORIGIN}/bridge/potato/sandbox/screen/FAKE${++links}/embed.html${mode === 'control' ? '?control=1' : ''}#vnc=TESTPASS`,
        mode,
        expiresMs: Date.now() + 3_600_000,
      })),
    takeOver: () => log('takeOver', [], () => move({ phase: 'you', bot: null, cardId: null, reason: null, cardExpiresMs: null })),
    handBack: () => log('handBack', [], () => move({ phase: 'free', bot: null })),
    skip: (cardId) => log('skip', [cardId], () => move({ phase: 'bot', cardId: null, reason: null, cardExpiresMs: null })),
    keep: () => log('keep', [], () => undefined),
    onChanged: (cb) => {
      listeners.add(cb);
      return Promise.resolve((() => void listeners.delete(cb)) as UnlistenFn);
    },
  };
}
