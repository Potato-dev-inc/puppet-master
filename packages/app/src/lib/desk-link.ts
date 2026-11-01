// The link between my computers (Labs `desk_enabled`): wire types, plain words, calls and the data hook.
// Commands live in src-tauri/src/desk/link/commands.rs; agents, MCP and the phone have no route to them.
// The switches ("Mouse and keyboard", "Clipboard") are the pairing commands in desk-pair.ts. No sample data here.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { pairErrorText } from './desk-pair';

/** `off`: the person has not turned "Mouse and keyboard" on for this computer, so nothing is tried. */
export type LinkState = 'online' | 'connecting' | 'offline' | 'off';
/** Where a computer sits relative to this one. */
export type LinkSide = 'left' | 'right' | 'top' | 'bottom';

export interface LinkDevice {
  node: string;
  name: string;
  state: LinkState;
  side: LinkSide;
  /** The pointer is on that computer right now. */
  mouse_here: boolean;
  /** That computer is using this one right now. */
  controls_me: boolean;
  input_on: boolean;
  clipboard_on: boolean;
}

export interface LinkView {
  seq: number;
  devices: LinkDevice[];
  /** The pointer left this computer for that one. */
  mouse_on: string | null;
  /** That computer is using this one. */
  controlled_by: string | null;
  paused: boolean;
  sharing: boolean;
  /** "Mouse sharing is on" while sharing; the tray says it too. */
  tray: string | null;
  /** "Can't control windows that run as admin." */
  admin_note: string;
  error: string | null;
  /** Why the last copy did not go across. */
  clipboard_note: string | null;
  supported: boolean;
}

export const SIDES: { side: LinkSide; label: string }[] = [
  { side: 'left', label: 'On the left' },
  { side: 'right', label: 'On the right' },
  { side: 'top', label: 'Above' },
  { side: 'bottom', label: 'Below' },
];

export const linkView = () => invoke<LinkView>('desk_link_view');
export const linkSetSide = (node: string, side: LinkSide) => invoke<LinkView>('desk_link_set_side', { node, side });
export const linkPause = (paused: boolean) => invoke<LinkView>('desk_link_pause', { paused });
export const linkPanic = () => invoke<LinkView>('desk_link_panic');

/** Plain words for a computer's link state. `null` when nothing is being tried (the switch is off). */
export function linkStateText(state: LinkState | undefined): string | null {
  switch (state) {
    case 'online':
      return 'Connected';
    case 'connecting':
      return 'Connecting…';
    case 'offline':
      return 'Offline';
    default:
      return null;
  }
}

/** The one line about the mouse, or `null` when nothing is shared. Pure, so the page and its tests read the same. */
export function linkStatusText(view: LinkView | null): string | null {
  if (!view || !view.supported) return null;
  if (view.error) return `Mouse sharing hit a problem: ${view.error}`;
  if (view.controlled_by) return `${view.controlled_by} is using this computer`;
  if (view.mouse_on) return `Mouse is on ${view.mouse_on}`;
  if (!view.sharing) return null;
  if (view.paused) return 'Mouse sharing is paused';
  const online = view.devices.filter((d) => d.input_on && d.state === 'online').map((d) => d.name);
  return online.length ? `Mouse sharing is on · ${online.join(', ')} connected` : (view.tray ?? 'Mouse sharing is on');
}

function isView(v: unknown): v is LinkView {
  return typeof v === 'object' && v !== null && Array.isArray((v as LinkView).devices) && typeof (v as LinkView).sharing === 'boolean';
}

export interface LinkApi {
  view: LinkView | null;
  /** The last command that failed. Cleared by the next one. */
  error: string | null;
  setSide: (node: string, side: LinkSide) => Promise<void>;
  pause: (paused: boolean) => Promise<void>;
  /** Pointer home and everything let go, on both computers. */
  panic: () => Promise<void>;
}

/** Follows the link state pushed by the desktop app. Reads nothing until `enabled`. */
export function useLink(enabled: boolean): LinkApi {
  const [view, setView] = useState<LinkView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const apply = useCallback((next: unknown) => {
    if (alive.current && isView(next)) setView(next);
  }, []);

  useEffect(() => {
    alive.current = true;
    if (!enabled) return () => void (alive.current = false);
    let off: UnlistenFn | undefined;
    let disposed = false;
    listen<LinkView>('desk://link', (e) => apply(e.payload))
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    Promise.resolve(linkView())
      .then(apply)
      .catch(() => {});
    return () => {
      disposed = true;
      alive.current = false;
      off?.();
    };
  }, [enabled, apply]);

  const call = useCallback(
    async (f: () => Promise<unknown>) => {
      setError(null);
      try {
        apply(await f());
      } catch (e) {
        if (alive.current) setError(pairErrorText(e));
      }
    },
    [apply],
  );

  return {
    view,
    error,
    setSide: (node, side) => call(() => linkSetSide(node, side)),
    pause: (paused) => call(() => linkPause(paused)),
    panic: () => call(linkPanic),
  };
}
