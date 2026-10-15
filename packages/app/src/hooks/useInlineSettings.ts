// Settings as one more cockpit main view ('settings'), plus everything that sends a person there or back:
//   - `open(tab?)` from the sidebar, the palette and any deep link ("Settings > Agents > Preload")
//   - Back and Escape return to the view that was showing before
//   - Rust asks for it (`cockpit://settings`, or `?view=settings&tab=` when it had to create the window)
//   - the mode switches to Individual while it is open: it moves to the separate Settings window
// Individual mode keeps the separate window: there `open` just asks Rust for it.
//
// The view value and its setter stay in CockpitApp (its `useState`); this hook only borrows them, so it does
// not care whether that state started as booleans or one union.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { cockpitApi, type UiMode } from '../lib/cockpit-api';
import { DEFAULT_SETTINGS_TAB } from '../lib/settings-entry';
import { resolveSettingsTarget } from '../lib/settings-target';
import type { SettingsPageId } from '../components/settings/model';

interface Options<V extends string> {
  mode: UiMode;
  view: V;
  setView: (view: V) => void;
  /** The view to return to when there is no earlier one (the default view). */
  home: V;
  /** Opens the separate Settings window (Individual mode). */
  openWindow: (tab?: string) => Promise<void> | void;
}

export interface InlineSettings {
  /** Show Settings in the main area (Orchestration) or in its window (Individual), on `tab` when given. */
  open: (tab?: string) => void;
  /** Changes when a deep link names a page, so the panel re-opens on it. Use as the `key` of `SettingsView`. */
  viewKey: number;
  viewProps: {
    initialTab: SettingsPageId;
    /** The row a deep link named, to scroll to. */
    initialRow?: string;
    onBack: () => void;
    onPageChange: (page: SettingsPageId) => void;
  };
}

/** `?view=settings&tab=bridge` -> `{ tab: 'bridge' }`; null when the URL is not a settings link. */
export function settingsLinkFromSearch(search: string): { tab?: string } | null {
  const q = new URLSearchParams(search);
  if (q.get('view') !== 'settings') return null;
  return { tab: q.get('tab') ?? undefined };
}

export function useInlineSettings<V extends string>({ mode, view, setView, home, openWindow }: Options<V>): InlineSettings {
  const SETTINGS = 'settings' as V;
  const [tab, setTab] = useState<SettingsPageId>(DEFAULT_SETTINGS_TAB);
  const [row, setRow] = useState<string | undefined>(undefined);
  const [nonce, setNonce] = useState(0);
  const before = useRef<V>(home);
  // The page the panel shows now: a deep link without a page, and a hand-off to the window, keep it.
  const page = useRef<SettingsPageId>(DEFAULT_SETTINGS_TAB);
  // The latest of everything `open` reads, so `open` itself stays stable for the effects below.
  const live = useRef({ mode, view, openWindow });
  live.current = { mode, view, openWindow };

  const open = useCallback(
    (to?: string) => {
      const { mode: m, view: v, openWindow: win } = live.current;
      if (m === 'individual') {
        void win(to);
        return;
      }
      const resolved = resolveSettingsTarget(to);
      const target = resolved?.page;
      if (v !== SETTINGS) {
        before.current = v;
        setTab(target ?? page.current);
        setRow(resolved?.row);
        setNonce((n) => n + 1);
      } else if (target && (target !== page.current || resolved?.row)) {
        setTab(target);
        setRow(resolved?.row);
        setNonce((n) => n + 1);
      }
      setView(SETTINGS);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [setView],
  );

  const back = useCallback(() => setView(before.current), [setView]);

  // The mode switched to Individual with Settings open here: it moves to the Settings window, on the same page.
  useEffect(() => {
    if (mode !== 'individual' || live.current.view !== SETTINGS) return;
    void live.current.openWindow(page.current);
    setView(before.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, setView]);

  // Rust opens Settings in this window: the tray's "Settings" item, or a window it just created.
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    void cockpitApi
      .onSettingsOpen((e) => open(e?.tab ?? undefined))
      .then((u) => (disposed ? u() : (off = u)));
    const link = settingsLinkFromSearch(window.location.search);
    // After the mount effects that reset the view (a selection change returns to the pane view).
    const timer = link ? window.setTimeout(() => open(link.tab), 0) : undefined;
    return () => {
      disposed = true;
      off?.();
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [open]);

  return useMemo(
    () => ({
      open,
      viewKey: nonce,
      viewProps: {
        initialTab: tab,
        initialRow: row,
        onBack: back,
        onPageChange: (p: SettingsPageId) => {
          page.current = p;
        },
      },
    }),
    [open, nonce, tab, row, back],
  );
}
