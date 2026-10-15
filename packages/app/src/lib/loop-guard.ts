// Stuck-agent warnings and the panic button. Backend: src-tauri loop_guard.rs and panic.rs.
import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type LoopKind = 'repeated_error' | 'repeated_action' | 'no_progress' | 'stalled';

export interface LoopSignal {
  pane_id: string;
  kind: LoopKind;
  evidence_lines: string[];
  turns: number;
  since: number;
  /** Tail of the card sentence: "the same error 4 times". */
  summary: string;
}

export interface StoppedPane {
  pane_id: string;
  agent_type: string;
  cwd: string;
}

export interface StopReport {
  stopped: StoppedPane[];
  /** [pane_id, reason] */
  failed: [string, string][];
}

export interface LoopGuardSettings {
  enabled: boolean;
  turns: number;
  stalled_minutes: number;
  panic_shortcut: boolean;
}

export interface LoopGuardApi {
  signals: () => Promise<LoopSignal[]>;
  dismiss: (paneId: string) => Promise<unknown>;
  stopAll: () => Promise<StopReport>;
  settingsGet: () => Promise<LoopGuardSettings>;
  settingsSet: (s: LoopGuardSettings) => Promise<LoopGuardSettings>;
  onSignal: (cb: (s: LoopSignal) => void) => Promise<() => void>;
  onClear: (cb: (paneId: string) => void) => Promise<() => void>;
  onPanic: (cb: (r: StopReport) => void) => Promise<() => void>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noop = () => {};

export const tauriLoopApi: LoopGuardApi = {
  signals: () => (isTauri() ? invoke<LoopSignal[]>('loop_signals') : Promise.resolve([])),
  dismiss: (paneId) => invoke('loop_dismiss', { paneId }),
  stopAll: () => invoke<StopReport>('stop_all_agents'),
  settingsGet: () => invoke<LoopGuardSettings>('loop_settings_get'),
  settingsSet: (settings) => invoke<LoopGuardSettings>('loop_settings_set', { settings }),
  onSignal: async (cb) => (isTauri() ? listen<LoopSignal>('loop://signal', (e) => cb(e.payload)) : noop),
  onClear: async (cb) => (isTauri() ? listen<{ pane_id: string }>('loop://clear', (e) => cb(e.payload.pane_id)) : noop),
  onPanic: async (cb) => (isTauri() ? listen<StopReport>('panic://stopped', (e) => cb(e.payload)) : noop),
};

export interface LoopGuardState {
  signals: Record<string, LoopSignal>;
  dismiss: (paneId: string) => void;
}

export const LoopGuardContext = createContext<LoopGuardState>({ signals: {}, dismiss: noop });
export const useLoopGuard = (): LoopGuardState => useContext(LoopGuardContext);

/** Live map of panes that currently show a stuck warning. */
export function useLoopSignals(api: LoopGuardApi = tauriLoopApi): LoopGuardState {
  const [signals, setSignals] = useState<Record<string, LoopSignal>>({});
  useEffect(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    api
      .signals()
      .then((list) => {
        if (disposed) return;
        setSignals((cur) => ({ ...Object.fromEntries(list.map((s) => [s.pane_id, s])), ...cur }));
      })
      .catch(() => {});
    const keep = (p: Promise<() => void>) => p.then((off) => (disposed ? off() : offs.push(off))).catch(() => {});
    keep(api.onSignal((s) => setSignals((cur) => ({ ...cur, [s.pane_id]: s }))));
    keep(
      api.onClear((id) =>
        setSignals((cur) => {
          if (!(id in cur)) return cur;
          const { [id]: _gone, ...rest } = cur;
          return rest;
        }),
      ),
    );
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [api]);
  const dismiss = useCallback(
    (paneId: string) => {
      setSignals((cur) => {
        const { [paneId]: _gone, ...rest } = cur;
        return rest;
      });
      void api.dismiss(paneId);
    },
    [api],
  );
  return { signals, dismiss };
}

/** "Claude Code seems stuck: the same error 4 times." */
export function stuckTitle(name: string, signal: LoopSignal): string {
  return `${name} seems stuck: ${signal.summary}.`;
}
