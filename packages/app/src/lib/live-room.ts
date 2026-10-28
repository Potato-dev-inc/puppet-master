// Live Room: TypeScript mirrors of the Rust types in `live_room/` and typed wrappers for the
// `live_room_*` commands (`commands/live_room.rs`) and the `live://state` event. Desktop window only.
// The room's secrets (read token, content key) stay in the link's #fragment. The UI only ever sees the masked link.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** What a viewer may see about a pane. `gone` = the pane closed. */
export type LiveState = 'working' | 'idle' | 'waiting' | 'error' | 'gone';

export interface LiveTask {
  text: string;
  status: string | null;
}

/** One cleaned chat item, as viewers see it. Tagged by `t`. */
export type LiveItem =
  | { t: 'user'; id: number; pane: string; text: string }
  | { t: 'agent'; id: number; pane: string; text: string }
  | { t: 'tool'; id: number; pane: string; name: string; summary: string; files: number; writes: boolean }
  | { t: 'result'; id: number; pane: string; ok: boolean; lines: number }
  | { t: 'end'; id: number; pane: string; outcome: string; ms: number | null }
  | { t: 'plan'; id: number; pane: string; tasks: LiveTask[]; replace: boolean }
  | { t: 'note'; id: number; pane: string; text: string };

export interface LivePane {
  key: string;
  label: string;
  title: string;
  state: LiveState;
  line: string | null;
}

export interface LiveStatus {
  live: boolean;
  paused: boolean;
  viewers: number;
  endsAtMs: number;
  paneIds: string[];
  hidden: number;
  /** Why the room stopped, as a fixed sentence key from the app; null while it runs. */
  stopped: string | null;
}

/** What the Go live sheet shows: the exact first snapshot viewers would get. */
export interface PreviewView {
  panes: LivePane[];
  items: LiveItem[];
  hidden: number;
}

export interface StartRequest {
  paneIds: string[];
  title: string;
  minutes: number;
  maxViewers: number;
}

export interface StartedRoom {
  endsAtMs: number;
}

export interface LiveRoomConfig {
  /** Relay address. Empty = use the team relay, if one is set. */
  relay: string;
  minutes: number;
  maxViewers: number;
}

/** Event name the app emits with a `LiveStatus` payload whenever the room changes. */
export const LIVE_STATE_EVENT = 'live://state';

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Tauri rejects with the Rust error text; the UI wants a plain Error with that text as the message. */
function messageOf(err: unknown): string {
  if (err instanceof Error) return err.message;
  return typeof err === 'string' ? err : String(err);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  try {
    return args === undefined ? await invoke<T>(command) : await invoke<T>(command, args);
  } catch (err) {
    throw new Error(messageOf(err));
  }
}

export const liveRoomApi = {
  preview: (paneIds: string[]) => call<PreviewView>('live_room_preview', { paneIds }),
  start: (req: StartRequest) => call<StartedRoom>('live_room_start', { req }),
  stop: () => call<void>('live_room_stop'),
  pause: () => call<void>('live_room_pause'),
  resume: () => call<void>('live_room_resume'),
  status: () => call<LiveStatus>('live_room_status'),
  /** Returns the full link. The caller copies it; the app never logs it. */
  copyLink: () => call<string>('live_room_copy_link'),
  configGet: () => call<LiveRoomConfig>('live_room_config_get'),
  configSet: (cfg: LiveRoomConfig) => call<void>('live_room_config_set', { cfg }),
};

export type LiveRoomApi = typeof liveRoomApi;

/** Subscribes to `live://state`. Outside the desktop shell it does nothing and returns a no-op. */
export async function onLiveState(cb: (status: LiveStatus) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<LiveStatus>(LIVE_STATE_EVENT, (e) => cb(e.payload));
}

/**
 * Shows a link without its secret: keeps the scheme, host and path, hides the #fragment.
 * Example: `https://relay.example.test/r/#••••`. Input that is not a URL becomes `••••`.
 */
export function maskLink(link: string): string {
  const hash = link.indexOf('#');
  const head = hash >= 0 ? link.slice(0, hash) : link;
  try {
    const u = new URL(head);
    return `${u.protocol}//${u.host}${u.pathname}#••••`;
  } catch {
    return '••••';
  }
}

/**
 * Whole minutes left until `endsAtMs`, rounded up so the bar never shows 0 while time is still running.
 * Never negative.
 */
export function minutesLeft(endsAtMs: number, now: number): number {
  const ms = endsAtMs - now;
  if (ms <= 0) return 0;
  return Math.ceil(ms / 60_000);
}

/** The length choice as plain words: 30 → `30 min`, 60 → `1 h`, 90 → `1 h 30 min`. */
export function durationLabel(min: number): string {
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const rest = min % 60;
  return rest === 0 ? `${h} h` : `${h} h ${rest} min`;
}
