// Global Ask bar. Backend: src-tauri askbar/ (window, shortcut, quick_ask wiring) and commands/askbar.rs.
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface AskbarSettings {
  enabled: boolean;
  /** One of `ASKBAR_SHORTCUTS` (the backend refuses anything else). */
  shortcut: string;
  default_agent: string | null;
  last_agent: string | null;
}

/** The fixed choice list the backend accepts. Keep in sync with `askbar::SHORTCUT_CHOICES`. */
export const ASKBAR_SHORTCUTS = ['Ctrl+Alt+Space', 'Ctrl+Shift+Space', 'Alt+Space', 'Ctrl+Alt+K'] as const;

/** An agent that can answer a headless read-only question. */
export interface AskbarAgent {
  id: string;
  label: string;
  /** A warm standby worker is waiting, so the answer starts at once. */
  ready: boolean;
}

export interface AskbarClipboard {
  /** Text to attach; already cut to at most 8000 characters. */
  text: string;
  /** Characters on the clipboard before cutting. */
  chars: number;
  truncated: boolean;
}

export interface AskError {
  code: string;
  message: string;
}

export interface AskResult {
  ask_id: string;
  agent: string;
  agent_label: string;
  question: string;
  ok: boolean;
  answer: string | null;
  error: AskError | null;
}

export interface AskRequest {
  ask_id: string;
  question: string;
  agent: string;
  /** Clipboard text to append as context, or null. */
  clipboard: string | null;
}

/** A finished answer nobody saw because the bar was hidden. */
export interface AskNotice {
  id: string;
  agent: string;
  agent_label: string;
  question: string;
  result: AskResult;
  at_ms: number;
}

export interface AskbarApi {
  settingsGet: () => Promise<AskbarSettings>;
  settingsSet: (settings: AskbarSettings) => Promise<AskbarSettings>;
  agents: () => Promise<AskbarAgent[]>;
  clipboard: () => Promise<AskbarClipboard | null>;
  ask: (request: AskRequest) => Promise<AskResult>;
  /** Hide the bar (Esc). */
  hide: () => Promise<unknown>;
  /** Grow the window for an answer (true) or shrink it to the input line (false). */
  resize: (expanded: boolean) => Promise<unknown>;
  continueInPane: (agent: string, question: string, clipboard: string | null) => Promise<unknown>;
  notices: () => Promise<AskNotice[]>;
  dismissNotice: (id: string) => Promise<unknown>;
  /** Show the bar with this notice's answer (and remove the notice). */
  openNotice: (id: string) => Promise<unknown>;
  /** The bar was just opened from the shortcut: refresh the clipboard and agents. */
  onShown: (cb: () => void) => Promise<() => void>;
  /** The bar was opened for an old answer. */
  onShowResult: (cb: (result: AskResult) => void) => Promise<() => void>;
  onNotice: (cb: (notice: AskNotice) => void) => Promise<() => void>;
  onNoticeRemoved: (cb: (id: string) => void) => Promise<() => void>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noop = () => {};

export const tauriAskbarApi: AskbarApi = {
  settingsGet: () => invoke<AskbarSettings>('askbar_settings_get'),
  settingsSet: (settings) => invoke<AskbarSettings>('askbar_settings_set', { settings }),
  agents: () => invoke<AskbarAgent[]>('askbar_agents'),
  clipboard: () => invoke<AskbarClipboard | null>('askbar_clipboard'),
  ask: (request) => invoke<AskResult>('askbar_ask', { request }),
  hide: () => invoke('askbar_hide'),
  resize: (expanded) => invoke('askbar_resize', { expanded }),
  continueInPane: (agent, question, clipboard) => invoke('askbar_continue', { agent, question, clipboard }),
  notices: () => (isTauri() ? invoke<AskNotice[]>('askbar_notices') : Promise.resolve([])),
  dismissNotice: (id) => invoke('askbar_notice_dismiss', { id }),
  openNotice: (id) => invoke('askbar_notice_open', { id }),
  onShown: async (cb) => (isTauri() ? listen('askbar://shown', () => cb()) : noop),
  onShowResult: async (cb) => (isTauri() ? listen<AskResult>('askbar://show-result', (e) => cb(e.payload)) : noop),
  onNotice: async (cb) => (isTauri() ? listen<AskNotice>('askbar://notice', (e) => cb(e.payload)) : noop),
  onNoticeRemoved: async (cb) => (isTauri() ? listen<string>('askbar://notice-removed', (e) => cb(e.payload)) : noop),
};

/** "1.2k chars" / "340 chars" for the clipboard chip. */
export function formatChars(n: number): string {
  if (n < 1000) return `${n} chars`;
  const k = n / 1000;
  return `${k >= 10 ? Math.round(k) : Math.round(k * 10) / 10}k chars`;
}

/** Text of the clipboard chip: "Clipboard · 1.2k chars", or "Clipboard · 8k of 52k chars" when cut. */
export function clipboardChipLabel(clip: AskbarClipboard): string {
  if (!clip.truncated) return `Clipboard · ${formatChars(clip.chars)}`;
  const kept = Array.from(clip.text).length;
  return `Clipboard · ${formatChars(kept).replace(' chars', '')} of ${formatChars(clip.chars)}`;
}

/** Live list of answers that arrived while the bar was hidden (Inbox cards). */
export function useAskNotices(api: AskbarApi = tauriAskbarApi): { notices: AskNotice[]; dismiss: (id: string) => void; open: (id: string) => void } {
  const [notices, setNotices] = useState<AskNotice[]>([]);
  useEffect(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    api.notices().then((list) => !disposed && list.length > 0 && setNotices((cur) => [...list, ...cur.filter((n) => !list.some((l) => l.id === n.id))])).catch(() => {});
    void api.onNotice((n) => setNotices((cur) => [n, ...cur.filter((c) => c.id !== n.id)])).then((off) => (disposed ? off() : offs.push(off)));
    void api.onNoticeRemoved((id) => setNotices((cur) => cur.filter((n) => n.id !== id))).then((off) => (disposed ? off() : offs.push(off)));
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [api]);
  const dismiss = useCallback((id: string) => {
    setNotices((cur) => cur.filter((n) => n.id !== id));
    void api.dismissNotice(id);
  }, [api]);
  const open = useCallback((id: string) => void api.openNotice(id), [api]);
  return { notices, dismiss, open };
}
