// My computers (Labs `desk_enabled`, off by default): the Labs switch hook, then the engine wire types and calls.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The Labs switch (`desk_enabled`). Off unless the person turns it on. */
export function deskEnabled(settings: { desk_enabled?: boolean } | null | undefined): boolean {
  return settings?.desk_enabled === true;
}

/** Whether the Labs switch is on, kept current when Settings change. */
export function useDeskEnabled(): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!inTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const read = () =>
      import('./settings')
        .then(({ loadSettings }) => loadSettings())
        .then((s) => !disposed && setOn(deskEnabled(s as { desk_enabled?: boolean })))
        .catch(() => {});
    void read();
    listen('settings://changed', () => void read())
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return on;
}

// ---- Engine wire types (phase DL-3). Commands live in src-tauri/src/desk/commands.rs; the phone and agents have no route to them.
// No mock data here: outside the desktop app these calls reject.

/** `off` = Screen hops not turned on. `unsupported` = this OS has no host yet. */
export type DeskState = 'off' | 'on' | 'starting' | 'paused' | 'labs_off' | 'error' | 'unsupported';

export interface DeskStatus {
  state: DeskState;
  hops_enabled: boolean;
  paused: boolean;
  hooks_installed: boolean;
  /** Times the watchdog put a dropped keyboard hook back. */
  hook_reinstalls: number;
  /** Stays until the next command that works; polling does not clear it. */
  error: string | null;
}

export interface DeskScreen {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** dpi / 96. */
  scale: number;
  primary: boolean;
  /** 1-based reading-order number ("go to screen N"). */
  index: number;
}

export interface DeskPointer {
  x: number;
  y: number;
  screen_id: string | null;
  screen_index: number | null;
}

export interface DeskHotkeyConfig {
  next: string;
  prev: string;
  /** Must contain `{n}`. */
  screen_n: string;
  find_pointer: string;
  pause: string;
}

export interface DeskBinding {
  /** `next`, `prev`, `screen:3`, `find`, `pause`. */
  action: string;
  keys: string;
}

export interface DeskHotkeys {
  config: DeskHotkeyConfig;
  defaults: DeskHotkeyConfig;
  bindings: DeskBinding[];
}

export interface DeskFind {
  /** Goes up on every "find my pointer"; draw the ring when it changes. */
  seq: number;
  x: number;
  y: number;
  screen_index: number | null;
  /** Say this to a screen reader; the ring is only visual. */
  announce: string;
}

export type DeskHopMode = 'relative' | 'memory' | 'center';

export interface DeskSnapshot {
  seq: number;
  status: DeskStatus;
  screens: DeskScreen[];
  pointer: DeskPointer | null;
  hop_mode: DeskHopMode;
  hotkeys: DeskHotkeys;
  find: DeskFind | null;
}

export type DeskCommand =
  | { type: 'refresh' }
  | { type: 'hop_next' }
  | { type: 'hop_prev' }
  | { type: 'hop_to'; screen: number }
  | { type: 'find_pointer' }
  | { type: 'set_hotkeys'; hotkeys: DeskHotkeyConfig }
  | { type: 'reset_hotkeys' }
  | { type: 'set_hops_enabled'; enabled: boolean }
  | { type: 'pause'; paused: boolean }
  | { type: 'set_hop_mode'; mode: DeskHopMode };

/** The error text every desk command returns while Labs `desk_enabled` is off. */
export const DESK_FEATURE_OFF_PREFIX = 'FEATURE_OFF';

export function isDeskFeatureOff(error: unknown): boolean {
  return String(error).includes(DESK_FEATURE_OFF_PREFIX);
}

export function deskSnapshot(): Promise<DeskSnapshot> {
  return invoke<DeskSnapshot>('desk_snapshot');
}

export function deskCommand(cmd: DeskCommand): Promise<DeskSnapshot> {
  return invoke<DeskSnapshot>('desk_command', { cmd });
}

/** Every changed snapshot, pushed by the engine. */
export function onDeskSnapshot(handler: (snapshot: DeskSnapshot) => void): Promise<UnlistenFn> {
  return listen<DeskSnapshot>('desk://snapshot', (event) => handler(event.payload));
}

// ---- The page's data hook. Starts from the engine's own snapshot, then follows every pushed change.

/** How long the first snapshot may take before the page says it could not start. */
export const DESK_START_LIMIT_MS = 3000;

export interface DeskView {
  /** null until the first snapshot arrives. */
  snapshot: DeskSnapshot | null;
  /** Plain words, set when the first read failed or took too long. Cleared by `reload`. */
  loadError: string | null;
  /** The last command that failed. Cleared by the next command. */
  actionError: string | null;
  reload: () => void;
  /** Sends a command. Resolves true when it worked. */
  run: (cmd: DeskCommand) => Promise<boolean>;
}

export function useDesk(): DeskView {
  const [snapshot, setSnapshot] = useState<DeskSnapshot | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [slow, setSlow] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const newest = useRef(-1);

  const apply = useCallback((next: DeskSnapshot) => {
    if (next.seq < newest.current) return; // an older snapshot never replaces a newer one
    newest.current = next.seq;
    setSnapshot(next);
    setReadError(null);
  }, []);

  useEffect(() => {
    let disposed = false;
    let off: UnlistenFn | undefined;
    setReadError(null);
    setSlow(false);
    const timer = setTimeout(() => !disposed && setSlow(true), DESK_START_LIMIT_MS);
    onDeskSnapshot((s) => !disposed && apply(s))
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    deskSnapshot()
      .then((s) => !disposed && apply(s))
      .catch((e: unknown) => !disposed && setReadError(String(e)));
    return () => {
      disposed = true;
      clearTimeout(timer);
      off?.();
    };
  }, [attempt, apply]);

  const run = useCallback(
    async (cmd: DeskCommand) => {
      setActionError(null);
      try {
        apply(await deskCommand(cmd));
        return true;
      } catch (e) {
        setActionError(String(e));
        return false;
      }
    },
    [apply],
  );

  const reload = useCallback(() => setAttempt((n) => n + 1), []);

  const loadError =
    readError ?? (slow && snapshot === null ? 'Took longer than 3 seconds to start. Try again.' : null);
  return { snapshot, loadError, actionError, reload, run };
}
