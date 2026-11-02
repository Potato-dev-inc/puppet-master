// Pair my computers (Labs `desk_enabled`): wire types, calls and the data hook. Commands live in
// src-tauri/src/pairing_commands.rs; agents, MCP and the phone have no route to them. No sample data here.
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface PairHosting {
  /** "4F7Q-9K2M" */
  code: string;
  /** What the other computer can paste when it cannot find this one nearby. */
  ticket: string;
  /** Milliseconds since 1970. */
  expires_at: number;
}

/** A "wants to pair" question waiting for this computer's person. */
export interface PairPending {
  id: number;
  /** `host`: this computer showed the code. `joiner`: this computer typed it. */
  role: 'host' | 'joiner';
  name: string;
  code: string;
  expires_at: number;
}

export interface PairDevice {
  node: string;
  name: string;
  added_at: number;
  /** `null`: not known yet (the desk link says it later). */
  online: boolean | null;
  /** Capability ids that are turned on. */
  allowed: string[];
}

export interface PairView {
  hosting: PairHosting | null;
  pending: PairPending | null;
  /** Progress words while this computer is pairing with another. */
  joining: string | null;
  /** The last result in plain words. */
  message: string | null;
  /** Milliseconds since 1970 while pairing is paused after too many wrong tries. */
  locked_until: number | null;
  /** The saved list failed its signature check and was not used. */
  tampered: boolean;
  devices: PairDevice[];
}

export interface PairNearby {
  node: string;
  name: string;
}

/** What the person can turn on per computer. Everything starts off. */
export const DESK_CAPABILITIES: { id: string; label: string }[] = [
  { id: 'desk/input', label: 'Mouse and keyboard' },
  { id: 'desk/clipboard', label: 'Clipboard' },
  { id: 'files', label: 'Files' },
];

export const pairView = () => invoke<PairView>('pair_view');
export const pairHostStart = () => invoke<PairView>('pair_host_start');
export const pairHostCancel = () => invoke<PairView>('pair_host_cancel');
export const pairJoin = (target: string, code: string) => invoke<PairView>('pair_join', { target, code });
export const pairAnswer = (id: number, allow: boolean) => invoke<PairView>('pair_answer', { id, allow });
export const pairNearby = () => invoke<PairNearby[]>('pair_nearby');
export const trustSetPermission = (node: string, capability: string, on: boolean) =>
  invoke<PairView>('trust_set_permission', { node, capability, on });
export const trustRemove = (node: string) => invoke<PairView>('trust_remove', { node });

/** The words without a CODE: prefix. */
export function pairErrorText(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e);
  return raw.replace(/^[A-Z][A-Z0-9_]{2,}:\s*/, '').trim() || 'Something went wrong. Try again.';
}

function isView(v: unknown): v is PairView {
  return typeof v === 'object' && v !== null && Array.isArray((v as PairView).devices);
}

export interface PairApi {
  view: PairView | null;
  /** The last command that failed. Cleared by the next one. */
  error: string | null;
  nearby: PairNearby[];
  refreshNearby: () => void;
  /** Every call that can fail resolves with the error in plain words, or null when it worked. */
  hostStart: () => Promise<string | null>;
  hostCancel: () => Promise<void>;
  join: (target: string, code: string) => Promise<string | null>;
  answer: (id: number, allow: boolean) => Promise<void>;
  setPermission: (node: string, capability: string, on: boolean) => Promise<void>;
  remove: (node: string) => Promise<void>;
}

/** Follows the pairing state pushed by the desktop app. Reads nothing until `enabled`. */
export function usePair(enabled: boolean): PairApi {
  const [view, setView] = useState<PairView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nearby, setNearby] = useState<PairNearby[]>([]);
  const alive = useRef(true);

  const apply = useCallback((next: unknown) => {
    if (alive.current && isView(next)) setView(next);
  }, []);

  useEffect(() => {
    alive.current = true;
    if (!enabled) return () => void (alive.current = false);
    let off: UnlistenFn | undefined;
    let disposed = false;
    listen<PairView>('pair://changed', (e) => apply(e.payload))
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    Promise.resolve(pairView())
      .then(apply)
      .catch(() => {});
    return () => {
      disposed = true;
      alive.current = false;
      off?.();
    };
  }, [enabled, apply]);

  const call = useCallback(
    async (f: () => Promise<unknown>): Promise<string | null> => {
      setError(null);
      try {
        apply(await f());
        return null;
      } catch (e) {
        const text = pairErrorText(e);
        if (alive.current) setError(text);
        return text;
      }
    },
    [apply],
  );

  return {
    view,
    error,
    nearby,
    refreshNearby: () => {
      Promise.resolve(pairNearby())
        .then((n) => alive.current && Array.isArray(n) && setNearby(n))
        .catch(() => {});
    },
    hostStart: () => call(pairHostStart),
    hostCancel: async () => void (await call(pairHostCancel)),
    join: (target, code) => call(() => pairJoin(target, code)),
    answer: async (id, allow) => {
      // A failed answer means the question is gone (timed out, other side left): re-read the state so the sheet closes.
      const failed = await call(() => pairAnswer(id, allow));
      if (failed) Promise.resolve(pairView()).then(apply).catch(() => {});
    },
    setPermission: async (node, capability, on) => void (await call(() => trustSetPermission(node, capability, on))),
    remove: async (node) => void (await call(() => trustRemove(node))),
  };
}
