// State for the cockpit window: panes (via usePaneRegistry), selection, prompts, mode, theme,
// warm-ready agents and the spawn flow. Backend data only; nothing is invented client-side
// except "status entered at" timestamps, which are measured from what this window observed.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { usePaneRegistry, type PaneRegistryApi } from './usePaneRegistry';
import { paneMenuEntries, useAgentRegistry, agentLabel, type LaunchEntry } from '../lib/agentRegistry';
import { usePanePreviews, type PreviewMap } from '../lib/pane-preview';
import { cockpitApi, type PanePrompt, type SpawnStageEvent, type UiMode } from '../lib/cockpit-api';
import { applyTheme, loadSettings, patchSettings, type AppTheme } from '../lib/settings';
import { subscribeSettingsTheme } from '../lib/settings-sync';
import { tauri } from '../lib/tauri';
import { useBridge } from './useBridge';
import { useQuestions, type QuestionsApi } from './useQuestions';
import { paneNames } from '../components/cockpit/model';
import { visibleSelection } from '../components/cockpit/selection';
import { usePaneActivity, type ActivityMap } from '../lib/pane-activity';

export interface SpawnState {
  agentId: string;
  paneId: string | null;
  log: { stage: SpawnStageEvent['stage']; message?: string; warm: boolean; at: number }[];
  failed: string | null;
}

export interface Toast {
  id: number;
  text: string;
  /** A button on the toast (for example "Undo"). The toast stays longer so it can be reached. */
  action?: { label: string; run: () => void };
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export interface CockpitApi {
  /** Questions agents asked the human through `ask_user`. */
  asked: QuestionsApi;
  registry: PaneRegistryApi;
  names: Map<string, string>;
  /** Plain-language activity phrase per pane id. */
  activities: ActivityMap;
  /** Dev-server URLs per pane id (newest first). */
  previews: PreviewMap;
  launchEntries: LaunchEntry[];
  /** Agent ids with at least one warm standby ready. */
  readyAgents: Set<string>;
  mode: UiMode;
  theme: AppTheme;
  selectedId: string | null;
  select: (paneId: string | null) => void;
  prompts: Record<string, PanePrompt | null>;
  statusSince: Map<string, number>;
  now: number;
  spawn: SpawnState | null;
  dismissSpawn: () => void;
  toasts: Toast[];
  notify: (text: string, action?: Toast['action']) => void;
  setMode: (mode: UiMode) => Promise<void>;
  toggleTheme: () => Promise<void>;
  newAgent: (agentId: string, accountId?: string, isolated?: boolean, projectPath?: string) => Promise<void>;
  answer: (paneId: string, promptId: string, choiceId: string, label: string) => Promise<void>;
  interrupt: (paneId: string) => Promise<void>;
  closePane: (paneId: string) => Promise<void>;
  /** Give a pane your own name (blank clears it). */
  rename: (paneId: string, title: string) => Promise<void>;
  /** Orchestration: Settings opens inside the cockpit (on `tab` when given). Individual: the Settings window. */
  openSettings: (tab?: string) => Promise<void>;
  bridgeLine: string;
}

export function useCockpit(): CockpitApi {
  const registry = usePaneRegistry();
  const activities = usePaneActivity();
  const previews = usePanePreviews();
  const listing = useAgentRegistry();
  const launchEntries = useMemo(() => paneMenuEntries(listing), [listing]);
  const { paneList, panes, initialReady } = registry;

  const [mode, setModeState] = useState<UiMode>('orchestration');
  const [theme, setTheme] = useState<AppTheme>('dark');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [prompts, setPrompts] = useState<Record<string, PanePrompt | null>>({});
  const [statusSince, setStatusSince] = useState<Map<string, number>>(new Map());
  const [now, setNow] = useState(() => Date.now());
  const [readyAgents, setReadyAgents] = useState<Set<string>>(new Set());
  const [spawn, setSpawn] = useState<SpawnState | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const bridge = useBridge();
  const bridgeLine = bridge.isReady
    ? `Bridge ${(bridge.client?.baseUrl ?? '').replace(/^https?:\/\//, '')}`
    : bridge.error
      ? 'Bridge unavailable'
      : 'Bridge connecting';

  const spawnRef = useRef(spawn);
  spawnRef.current = spawn;
  const toastId = useRef(0);
  const lastStatus = useRef(new Map<string, string>());

  const notify = useCallback((text: string, action?: Toast['action']) => {
    const id = ++toastId.current;
    const dismiss = () => setToasts((t) => t.filter((x) => x.id !== id));
    // Using the action closes its toast, so Undo cannot be pressed twice.
    const wrapped = action && { label: action.label, run: () => (dismiss(), action.run()) };
    setToasts((t) => [...t.slice(-2), { id, text, action: wrapped }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), action ? 12000 : 3200);
  }, []);

  const names = useMemo(() => paneNames(paneList, (id) => agentLabel(listing, id)), [paneList, listing]);

  // Clock for "waiting 2m" text.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(timer);
  }, []);

  // Status-entered timestamps, measured client-side from what this window observed.
  useEffect(() => {
    const prev = lastStatus.current;
    const changed: string[] = [];
    const seen = new Set<string>();
    for (const p of paneList) {
      seen.add(p.info.id);
      if (prev.get(p.info.id) !== p.status) changed.push(p.info.id);
      prev.set(p.info.id, p.status);
    }
    let gone = false;
    for (const id of [...prev.keys()]) {
      if (!seen.has(id)) {
        prev.delete(id);
        gone = true;
      }
    }
    if (!changed.length && !gone) return;
    const at = Date.now();
    setStatusSince((old) => {
      const next = new Map<string, number>();
      for (const p of paneList) {
        const id = p.info.id;
        next.set(id, changed.includes(id) ? at : (old.get(id) ?? at));
      }
      return next;
    });
  }, [paneList]);

  // Mode + theme at startup, then follow mode changes.
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    cockpitApi.getUiMode().then((m) => !disposed && setModeState(m)).catch(() => {});
    void cockpitApi.onModeChanged((e) => setModeState(e.mode)).then((u) => (disposed ? u() : (off = u)));
    loadSettings()
      .then((s) => {
        if (disposed) return;
        setTheme(s.theme ?? 'dark');
        applyTheme(s.theme ?? 'dark');
      })
      .catch(() => {});
    // A theme chosen in Settings (or any other window) applies here at once.
    const offTheme = subscribeSettingsTheme((next) => {
      setTheme(next);
      applyTheme(next);
    });
    return () => {
      disposed = true;
      off?.();
      offTheme();
    };
  }, []);

  // Warm standby readiness (existing status API).
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    const load = () =>
      tauri
        .standbyGetStatus()
        .then((s) => {
          if (disposed || !s) return;
          setReadyAgents(
            new Set(s.standbys.filter((x) => x.state === 'ready' && !x.headless && !x.pane_id).map((x) => x.agent_type)),
          );
        })
        .catch(() => {});
    void load();
    void cockpitApi.onStandbyChanged(() => void load()).then((u) => (disposed ? u() : (off = u)));
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  // Prompts: events keep them current; fetch for panes that are waiting and not yet known.
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    void cockpitApi
      .onPrompt((e) => setPrompts((p) => ({ ...p, [e.pane_id]: e.prompt })))
      .then((u) => (disposed ? u() : (off = u)));
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  const requested = useRef(new Set<string>());
  useEffect(() => {
    for (const p of paneList) {
      const id = p.info.id;
      if (p.status !== 'waiting_input') {
        requested.current.delete(id);
        continue;
      }
      if (id in prompts || requested.current.has(id)) continue;
      requested.current.add(id);
      cockpitApi
        .panePrompt(id)
        .then((prompt) => setPrompts((cur) => (id in cur ? cur : { ...cur, [id]: prompt })))
        .catch(() => setPrompts((cur) => (id in cur ? cur : { ...cur, [id]: null })));
    }
    // Drop prompts of panes that stopped waiting or vanished.
    setPrompts((cur) => {
      const keys = Object.keys(cur);
      const stale = keys.filter((k) => panes.get(k)?.status !== 'waiting_input');
      if (!stale.length) return cur;
      const next = { ...cur };
      for (const k of stale) delete next[k];
      return next;
    });
  }, [paneList, panes, prompts]);

  // `?select=<pane id>`: the window was created for a "show this pane" request.
  const initialSelect = useRef(new URLSearchParams(window.location.search).get('select'));
  useEffect(() => {
    const want = initialSelect.current;
    if (!want || !panes.has(want)) return;
    initialSelect.current = null;
    setSelectedId(want);
  }, [panes]);

  // cockpit://select
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    void cockpitApi
      .onSelect((e) => {
        setSelectedId(e.pane_id);
        if (spawnRef.current && spawnRef.current.paneId === e.pane_id) setSpawn(null);
      })
      .then((u) => (disposed ? u() : (off = u)));
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  // Spawn stage log for the spawn the user started from this window.
  useEffect(() => {
    let disposed = false;
    let off: (() => void) | undefined;
    void cockpitApi
      .onSpawnStage((e) => {
        const cur = spawnRef.current;
        if (!cur || cur.agentId !== e.agent_id) return;
        // Another origin (tray, agent) can emit stages for the same agent: once our spawn is bound
        // to a pane, only that pane's stages count.
        if (cur.paneId && e.pane_id && cur.paneId !== e.pane_id) return;
        const entry = { stage: e.stage, message: e.message, warm: e.warm, at: Date.now() };
        if (e.stage === 'ready') {
          const paneId = e.pane_id ?? cur.paneId;
          setSpawn(null);
          if (paneId) setSelectedId(paneId);
          return;
        }
        setSpawn({
          ...cur,
          paneId: e.pane_id ?? cur.paneId,
          log: [...cur.log, entry],
          failed: e.stage === 'failed' ? (e.message ?? 'Failed to start') : cur.failed,
        });
      })
      .then((u) => (disposed ? u() : (off = u)));
    return () => {
      disposed = true;
      off?.();
    };
  }, []);

  // Every pane this window has seen listed. A selected pane that was listed and is gone was
  // closed (a merge, a discard, Close): the view leaves it at once instead of flashing "Opening
  // pane". A pane that was never listed is still opening and gets the grace period below.
  const everListed = useRef(new Set<string>());
  for (const id of panes.keys()) everListed.current.add(id);
  const shownId = visibleSelection(selectedId, panes, everListed.current, initialReady, !!spawn);
  useEffect(() => {
    if (selectedId && shownId === null) setSelectedId(null);
  }, [selectedId, shownId]);

  // Selected pane never showed up in the list: back to the inbox after a short grace for the refresh.
  useEffect(() => {
    if (!selectedId || !initialReady || panes.has(selectedId) || spawn) return;
    const t = window.setTimeout(() => setSelectedId(null), 1500);
    return () => window.clearTimeout(t);
  }, [selectedId, initialReady, panes, spawn]);

  const select = useCallback((id: string | null) => {
    setSelectedId(id);
    if (id) setSpawn((s) => (s && !s.failed ? s : null));
  }, []);

  const setMode = useCallback(
    async (next: UiMode) => {
      const prev = mode;
      setModeState(next);
      try {
        await cockpitApi.setUiMode(next);
      } catch (err) {
        setModeState(prev);
        notify(errText(err));
      }
    },
    [mode, notify],
  );

  const toggleTheme = useCallback(async () => {
    const next: AppTheme = theme === 'dark' ? 'light' : 'dark';
    setTheme(next);
    applyTheme(next);
    try {
      await patchSettings({ theme: next });
    } catch (err) {
      notify(`Theme not saved: ${errText(err)}`);
    }
  }, [theme, notify]);

  const newAgent = useCallback(
    async (agentId: string, accountId?: string, isolated = false, projectPath?: string) => {
      setSelectedId(null);
      setSpawn({ agentId, paneId: null, log: [], failed: null });
      try {
        const paneId = await cockpitApi.newPane(agentId, accountId, isolated ? 'worktree' : undefined, projectPath);
        setSpawn((s) => (s && s.agentId === agentId ? { ...s, paneId } : s));
        // The pane may already be attached (ready event raced the command): select it once listed.
        if (spawnRef.current === null) setSelectedId(paneId);
      } catch (err) {
        setSpawn((s) => (s && s.agentId === agentId ? { ...s, failed: errText(err) } : s));
      }
    },
    [],
  );

  const answer = useCallback(
    async (paneId: string, promptId: string, choiceId: string, label: string) => {
      try {
        await cockpitApi.answerPanePrompt(paneId, promptId, choiceId);
        notify(`Answered ${names.get(paneId) ?? 'agent'}: ${label}`);
      } catch (err) {
        notify(errText(err));
      }
    },
    [names, notify],
  );

  const interrupt = useCallback(
    async (paneId: string) => {
      try {
        await cockpitApi.interruptPane(paneId);
        notify('Sent Ctrl+C');
      } catch (err) {
        notify(errText(err));
      }
    },
    [notify],
  );

  const closePane = useCallback(
    async (paneId: string) => {
      try {
        await registry.killPane(paneId);
        setSelectedId((cur) => (cur === paneId ? null : cur));
      } catch (err) {
        notify(errText(err));
      }
    },
    [registry, notify],
  );

  const rename = useCallback(
    async (paneId: string, title: string) => {
      try {
        await cockpitApi.paneSetTitle(paneId, title);
        await registry.refresh();
      } catch (err) {
        notify(errText(err));
      }
    },
    [registry, notify],
  );

  const openSettings = useCallback(async (tab?: string) => {
    try {
      await cockpitApi.openSettings(tab);
    } catch (err) {
      console.error('open_settings failed', err);
      notify(`Settings did not open: ${errText(err)}`);
    }
  }, [notify]);

  const asked = useQuestions(notify, now);

  return {
    asked,
    registry,
    names,
    activities,
    previews,
    launchEntries,
    readyAgents,
    mode,
    theme,
    selectedId: shownId,
    select,
    prompts,
    statusSince,
    now,
    spawn,
    dismissSpawn: () => setSpawn(null),
    toasts,
    notify,
    setMode,
    toggleTheme,
    newAgent,
    answer,
    interrupt,
    closePane,
    rename,
    openSettings,
    bridgeLine,
  };
}
