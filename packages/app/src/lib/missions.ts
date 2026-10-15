// Missions (Preview): a goal plus checks plus a budget, worked on in turns until the checks pass.
// The engine is Rust (src-tauri/src/missions); this file is the desktop's client: the view shapes
// (mirroring `missions::view::MissionView`), the Tauri commands, the start sheet's rules and the
// polling hook. Words follow UX-PHASE1 0.3: Mission, Preview, Separate copy, Checks, Budget,
// Backup agents, Hand off. Never "worktree", "round", "tamper" or an id.
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** One of the five glyphs: `~` working, `+` needs you, `v` done, `:` paused, `x` ended. */
export type Glyph = '~' | '+' | 'v' | ':' | 'x';

export type MissionState = 'planning' | 'working' | 'checking' | 'green' | 'needs_you' | 'paused' | 'handed_off' | 'stopped' | 'failed';
export type MissionNeeds = 'tamper' | 'budget' | 'setup' | 'stuck';

export interface CheckLine {
  name: string;
  passed: boolean;
  text: string;
}

export interface TimelineRow {
  kind: 'turn' | 'event';
  glyph: Glyph;
  turn?: number;
  agent?: string;
  counts?: string;
  delta?: string;
  text: string;
  action?: 'open_pane' | 'show_diff' | 'show_brief';
  pane_id?: string;
  at_ms: number;
  checks?: CheckLine[];
}

export interface MissionView {
  id: string;
  handle: string;
  preview: boolean;
  project: string;
  project_name: string;
  objective: string;
  title: string;
  state: MissionState;
  state_text: string;
  glyph: Glyph;
  needs?: MissionNeeds;
  /** The card's headline while it needs you ("The checks pass, but Codex changed the tests"). */
  card_title?: string;
  sentence?: string;
  pause_requested: boolean;
  turn: number;
  start?: string;
  now?: string;
  passed: number;
  total: number;
  budget: { turns_used: number; turns?: number; minutes_used: number; minutes?: number; text: string };
  agents: string[];
  agents_line: string;
  current_agent?: string;
  current_run?: string;
  current_pane?: string;
  copy?: { path: string; branch: string; base: string; base_commit: string; closed: boolean };
  checks: CheckLine[];
  may_change_tests: boolean;
  rollback: boolean;
  test_change_lines: string[];
  test_changes_text: string;
  timeline: TimelineRow[];
  started_by: string;
  work_text: string;
  created_at_ms: number;
  updated_at_ms: number;
  ended_at_ms?: number;
  rev: number;
  /** `mission_get` only: the agents' notes from MISSION.md. */
  notes?: string;
  ledger_path?: string;
}

export interface DetectedCheck {
  name: string;
  command: string[];
  source: string;
  on: boolean;
}

export type CardChoice = 'undo_tests' | 'accept_tests' | 'more_turns' | 'hand_to_me' | 'try_again' | 'stop';

export interface StartInput {
  project_path: string;
  objective: string;
  checks: { name: string; command: string[] }[];
  agents: string[];
  may_change_tests: boolean;
  rollback: boolean;
  budget_turns?: number;
  budget_minutes?: number;
  round_timeout_min?: number;
  check_timeout_min?: number;
}

export interface MissionsApi {
  list: (project?: string) => Promise<MissionView[]>;
  get: (id: string) => Promise<MissionView>;
  detect: (project: string) => Promise<DetectedCheck[]>;
  start: (input: StartInput) => Promise<MissionView>;
  stop: (id: string) => Promise<MissionView>;
  pause: (id: string) => Promise<MissionView>;
  resume: (id: string) => Promise<MissionView>;
  decide: (id: string, choice: CardChoice) => Promise<MissionView>;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const tauriMissionsApi: MissionsApi = {
  list: async (project) => (inTauri() ? (await invoke<{ missions: MissionView[] }>('mission_list', { project: project ?? null })).missions : []),
  get: (id) => invoke<MissionView>('mission_get', { id }),
  detect: async (project) => (await invoke<{ checks: DetectedCheck[] }>('mission_detect', { project })).checks,
  start: (input) => invoke<MissionView>('mission_start', { input }),
  stop: (id) => invoke<MissionView>('mission_stop', { id }),
  pause: (id) => invoke<MissionView>('mission_pause', { id }),
  resume: (id) => invoke<MissionView>('mission_resume', { id }),
  decide: (id, choice) => invoke<MissionView>('mission_decide', { id, choice }),
};

// ---- the start sheet -----------------------------------------------------------------------------

/** "Your goal mentions tests, so agents may change them." Mirrors Rust `missions::goal_names_tests`. */
export function goalNamesTests(objective: string): boolean {
  return /\b(tests?|specs?|snapshots?)\b/i.test(objective);
}

export interface SheetState {
  objective: string;
  checks: (DetectedCheck & { on: boolean })[];
  agents: string[];
  turns: number;
  hours: number;
  rollback: boolean;
  git: boolean;
  activeMissions: number;
}

/** Why "Start mission" is disabled, first match wins (DESIGN-MISSIONS 6.2), or null. */
export function startBlockedReason(s: SheetState): string | null {
  if (!s.objective.trim()) return "Say what should be true when it's done.";
  const on = s.checks.filter((c) => c.on);
  if (on.length === 0) return 'Pick at least one check. A mission ends only when its checks pass.';
  if (!on.some((c) => c.command.length > 0)) return 'Add a check that runs something, like Tests.';
  if (s.turns <= 0 && s.hours <= 0) return 'Set a budget in turns or time.';
  if (!s.git) return 'Missions need a git project, because they work in a separate copy.';
  if (s.agents.length === 0) return 'Pick an agent to work on it.';
  if (s.activeMissions >= 2) return 'Two missions are already running. Stop or finish one first.';
  return null;
}

/** "npm test" from an argv, as a person would type it. */
export function commandText(argv: string[]): string {
  return argv.map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(' ');
}

/** "npm run lint" typed into the sheet's "Add a check" field, back into an argv. */
export function parseCommand(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) out.push(m[1] ?? m[2]);
  return out;
}

// ---- lists -------------------------------------------------------------------------------------

export const isActive = (m: MissionView): boolean => !['green', 'handed_off', 'stopped', 'failed'].includes(m.state);

/** The mission a pane works for: its current agent's pane, or any pane that ran one of its turns. */
export function missionOfPane(missions: MissionView[], paneId: string): MissionView | null {
  return (
    missions.find((m) => m.current_pane === paneId) ??
    missions.find((m) => m.timeline.some((r) => r.pane_id === paneId)) ??
    null
  );
}

/** "9 of 12 checks pass" under the bar; the bar counts tests when they were counted. */
export function barText(m: MissionView): string {
  if (m.total === 0) return '';
  return m.now && !m.now.includes('checks') ? `${m.passed} of ${m.total} pass` : `${m.passed} of ${m.total} checks pass`;
}

/** Polls the mission list (every 2 s while any mission is active, else every 10 s) and reloads on
 *  `missions://changed`. */
export function useMissions(api: MissionsApi = tauriMissionsApi, project?: string): { missions: MissionView[]; reload: () => void } {
  const [missions, setMissions] = useState<MissionView[]>([]);
  const [nonce, setNonce] = useState(0);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  useEffect(() => {
    let cancelled = false;
    api
      .list(project)
      .then((list) => !cancelled && setMissions(list))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [api, project, nonce]);
  const anyActive = missions.some(isActive);
  useEffect(() => {
    const timer = setTimeout(reload, anyActive ? 2000 : 10000);
    return () => clearTimeout(timer);
  }, [reload, anyActive, nonce]);
  useEffect(() => {
    if (!inTauri()) return;
    let off: (() => void) | undefined;
    let disposed = false;
    listen('missions://changed', () => reload())
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, [reload]);
  return { missions, reload };
}

// ---- the start sheet's opener (the "+" menu and the palette share it) ---------------------------

type SheetListener = (project: string | null) => void;
const sheetListeners = new Set<SheetListener>();

export function openStartMission(project: string | null): void {
  sheetListeners.forEach((l) => l(project));
}

export function onOpenStartMission(listener: SheetListener): () => void {
  sheetListeners.add(listener);
  return () => sheetListeners.delete(listener);
}

/** The Labs switch (`missions_enabled`). Off by default. */
export function missionsEnabled(settings: { missions_enabled?: boolean } | null | undefined): boolean {
  return settings?.missions_enabled === true;
}

/** Whether the Labs switch is on, kept current when Settings change. */
export function useMissionsEnabled(): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!inTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const read = () =>
      import('./settings')
        .then(({ loadSettings }) => loadSettings())
        .then((s) => !disposed && setOn(missionsEnabled(s as { missions_enabled?: boolean })))
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
