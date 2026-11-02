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
  /** What the person chose for keys and scrolling with this computer. Absent from an older app build: use the defaults. */
  prefs?: PeerPrefs;
}

/** Keys and scrolling, per computer (Rust: desk/link/prefs.rs). */
export interface PeerPrefs {
  /** Windows Alt <-> Mac Command. */
  alt_cmd: boolean;
  /** Windows key <-> Mac Option. */
  win_option: boolean;
  /** Flip the wheel when I use that computer. */
  natural: boolean;
  /** Wheel speed when I use that computer; 1 = as is. */
  speed: number;
  /** Flip the wheel when that computer uses this one. */
  reverse_incoming: boolean;
  /** How far that computer's pointer moves per step of my mouse; 1 = as is. */
  pointer_speed: number;
}

export const DEFAULT_PEER_PREFS: PeerPrefs = {
  alt_cmd: true,
  win_option: true,
  natural: false,
  speed: 1,
  reverse_incoming: false,
  pointer_speed: 1,
};

/** The pointer speeds the slider takes; the app accepts exactly this range (Rust: desk/link/pointer_speed.rs). */
export const POINTER_SPEED_MIN = 0.25;
export const POINTER_SPEED_MAX = 3;

/** The speeds the page offers; the app accepts anything from 0.25 to 4. */
export const SCROLL_SPEEDS = [0.5, 0.75, 1, 1.5, 2, 3];

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

/** One screen in the shared layout space (this computer's box starts at 0, 0). */
export interface LayoutScreen {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
}

/** One computer in the layout: the box around its screens and the screens in it. */
export interface LayoutComputer {
  /** Empty for this computer. */
  node: string;
  name: string;
  me: boolean;
  online: boolean;
  /** False until it has told us its screens; the screens drawn are a stand-in. */
  known: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
  screens: LayoutScreen[];
}

/** Every computer's screens in one space. */
export interface LayoutView {
  computers: LayoutComputer[];
  /** Edges closer than this count as touching. */
  touch_gap: number;
}

export const SIDES: { side: LinkSide; label: string }[] = [
  { side: 'left', label: 'On the left' },
  { side: 'right', label: 'On the right' },
  { side: 'top', label: 'Above' },
  { side: 'bottom', label: 'Below' },
];

export const linkView = () => invoke<LinkView>('desk_link_view');
export const linkSetSide = (node: string, side: LinkSide) => invoke<LinkView>('desk_link_set_side', { node, side });
export const linkLayout = () => invoke<LayoutView>('desk_link_layout');
export const linkSetPlace = (node: string, x: number, y: number) => invoke<LayoutView>('desk_link_set_place', { node, x, y });
export const linkPause = (paused: boolean) => invoke<LinkView>('desk_link_pause', { paused });
export const linkPanic = () => invoke<LinkView>('desk_link_panic');
export const linkSetKeymap = (node: string, altCmd: boolean, winOption: boolean) =>
  invoke<LinkView>('desk_link_set_keymap', { node, altCmd, winOption });
export const linkSetScroll = (node: string, natural: boolean, speed: number, reverseIncoming: boolean) =>
  invoke<LinkView>('desk_link_set_scroll', { node, natural, speed, reverseIncoming });
export const linkSetPointerSpeed = (node: string, speed: number) => invoke<LinkView>('desk_link_set_pointer_speed', { node, speed });

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

function isLayout(v: unknown): v is LayoutView {
  return typeof v === 'object' && v !== null && Array.isArray((v as LayoutView).computers);
}

function isView(v: unknown): v is LinkView {
  return typeof v === 'object' && v !== null && Array.isArray((v as LinkView).devices) && typeof (v as LinkView).sharing === 'boolean';
}

export interface LinkApi {
  view: LinkView | null;
  /** Every computer's screens in one space, or null until the engine answers. */
  layout: LayoutView | null;
  /** The last command that failed. Cleared by the next one. */
  error: string | null;
  setSide: (node: string, side: LinkSide) => Promise<void>;
  setKeymap: (node: string, altCmd: boolean, winOption: boolean) => Promise<void>;
  setScroll: (node: string, natural: boolean, speed: number, reverseIncoming: boolean) => Promise<void>;
  setPointerSpeed: (node: string, speed: number) => Promise<void>;
  /** Puts a computer's box at (x, y) in the layout. Resolves false (and sets `error`) when it was refused. */
  setPlace: (node: string, x: number, y: number) => Promise<boolean>;
  pause: (paused: boolean) => Promise<void>;
  /** Pointer home and everything let go, on both computers. */
  panic: () => Promise<void>;
}

/** Follows the link state pushed by the desktop app. Reads nothing until `enabled`. */
export function useLink(enabled: boolean): LinkApi {
  const [view, setView] = useState<LinkView | null>(null);
  const [layout, setLayout] = useState<LayoutView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const apply = useCallback((next: unknown) => {
    if (alive.current && isView(next)) setView(next);
  }, []);

  const applyLayout = useCallback((next: unknown) => {
    if (alive.current && isLayout(next)) setLayout(next);
  }, []);

  useEffect(() => {
    alive.current = true;
    if (!enabled) return () => void (alive.current = false);
    const offs: UnlistenFn[] = [];
    let disposed = false;
    const keep = (u: UnlistenFn) => (disposed ? u() : offs.push(u));
    listen<LinkView>('desk://link', (e) => apply(e.payload))
      .then(keep)
      .catch(() => {});
    listen<LayoutView>('desk://layout', (e) => applyLayout(e.payload))
      .then(keep)
      .catch(() => {});
    Promise.resolve(linkView())
      .then(apply)
      .catch(() => {});
    Promise.resolve(linkLayout())
      .then(applyLayout)
      .catch(() => {});
    return () => {
      disposed = true;
      alive.current = false;
      offs.forEach((u) => u());
    };
  }, [enabled, apply, applyLayout]);

  const call = useCallback(
    async (f: () => Promise<unknown>): Promise<boolean> => {
      setError(null);
      try {
        const next = await f();
        apply(next);
        applyLayout(next);
        return true;
      } catch (e) {
        if (alive.current) setError(pairErrorText(e));
        return false;
      }
    },
    [apply, applyLayout],
  );

  return {
    view,
    layout,
    error,
    setSide: async (node, side) => {
      await call(() => linkSetSide(node, side));
    },
    setPlace: (node, x, y) => call(() => linkSetPlace(node, x, y)),
    setKeymap: async (node, altCmd, winOption) => {
      await call(() => linkSetKeymap(node, altCmd, winOption));
    },
    setScroll: async (node, natural, speed, reverseIncoming) => {
      await call(() => linkSetScroll(node, natural, speed, reverseIncoming));
    },
    setPointerSpeed: async (node, speed) => {
      await call(() => linkSetPointerSpeed(node, speed));
    },
    pause: async (paused) => {
      await call(() => linkPause(paused));
    },
    panic: async () => {
      await call(linkPanic);
    },
  };
}
