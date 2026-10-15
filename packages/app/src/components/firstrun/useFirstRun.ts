// Decides when the first-run welcome is on screen and keeps its few settings: shown once on a fresh
// profile, shown again when asked (palette, Settings, from any window), closed for good by Finish or Skip.
import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { hasEverSpawned } from '../../lib/agent-health';
import { loadSettings, patchSettings } from '../../lib/settings';
import { firstRunApi, type FirstRunApi } from './first-run-api';
import { finishPatch, lookFromSettings, reopenPatch, resumeStep, shouldShowWelcome, type FirstRunState, type Look, type ProjectPick, type StepId } from './model';

export interface Seed {
  start: StepId;
  look: Look | null;
  project: ProjectPick | null;
  agentId: string | null;
}

export interface FirstRunHost {
  open: boolean;
  seed: Seed;
  /** Changes on every (re)open, so the welcome starts from a clean state. */
  openKey: number;
  reopen: () => void;
  finish: () => void;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export interface Deps {
  api?: FirstRunApi;
  load?: typeof loadSettings;
  patch?: typeof patchSettings;
}

async function seedFrom(state: FirstRunState | null, load: typeof loadSettings, asked: boolean): Promise<Seed> {
  const s = await load().catch(() => null);
  const path = s?.onboarding_project ?? null;
  const agentId = s?.onboarding_agent ?? null;
  const project: ProjectPick | null = path ? { path, name: path.split(/[\\/]/).filter(Boolean).pop() ?? path, state: 'repo' } : null;
  return {
    start: resumeStep(state?.step, project !== null, asked),
    look: lookFromSettings(s?.simple_mode),
    project,
    agentId,
  };
}

const EMPTY_SEED: Seed = { start: 'welcome', look: null, project: null, agentId: null };

/**
 * `paneCount` and `panesReady` come from the pane registry: an automatic show waits until the panes are
 * listed, so an existing user with panes never meets the welcome.
 */
export function useFirstRun(paneCount: number, panesReady: boolean, deps: Deps = {}): FirstRunHost {
  const { api = firstRunApi, load = loadSettings, patch = patchSettings } = deps;
  const [state, setState] = useState<FirstRunState | null>(null);
  const [open, setOpen] = useState(false);
  const [seed, setSeed] = useState<Seed>(EMPTY_SEED);
  const [openKey, setOpenKey] = useState(0);
  const decided = useRef(false);
  const openRef = useRef(false);
  openRef.current = open;

  const show = useCallback(
    (from: FirstRunState | null, asked: boolean) => {
      void seedFrom(from, load, asked).then((next) => {
        setSeed(next);
        setOpenKey((k) => k + 1);
        setOpen(true);
      });
    },
    [load],
  );

  // What the settings file says, once at start.
  useEffect(() => {
    let alive = true;
    api
      .state()
      .then((s) => alive && setState(s))
      .catch(() => undefined); // no desktop shell (a plain browser, a test): no welcome
    return () => {
      alive = false;
    };
  }, [api]);

  // The first decision waits for the pane list: it cannot see "this install has had panes" before that.
  useEffect(() => {
    if (decided.current || !state || !panesReady) return;
    decided.current = true;
    if (shouldShowWelcome(state, hasEverSpawned(), paneCount)) show(state, false);
  }, [state, panesReady, paneCount, show]);

  // Asked for again, from this window or another: the settings file says `onboarding_done: false`.
  useEffect(() => {
    if (!inTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    void listen<{ onboarding_done?: unknown } | null>('settings://changed', (event) => {
      if (event.payload?.onboarding_done !== false) return;
      decided.current = true;
      // This window already opened it (palette): the same patch comes back as an event.
      if (openRef.current) return;
      api
        .state()
        .then((s) => {
          setState(s);
          if (s.show) show(s, true);
        })
        .catch(() => undefined);
    })
      .then((un) => (disposed ? un() : (off = un)))
      .catch(() => undefined);
    return () => {
      disposed = true;
      off?.();
    };
  }, [api, show]);

  const reopen = useCallback(() => {
    decided.current = true;
    void patch(reopenPatch()).catch(() => undefined);
    show({ show: true, auto: false, step: 'welcome' }, true);
  }, [patch, show]);

  const finish = useCallback(() => {
    setOpen(false);
    void patch(finishPatch()).catch(() => undefined);
  }, [patch]);

  return { open, seed, openKey, reopen, finish };
}
