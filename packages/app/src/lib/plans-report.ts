// "Your plans this month": one row per subscription (an agent under one account). Shape mirrors the Rust
// `plans_report::model::PlansReport` (src-tauri/src/plans_report/model.rs). Labels come from the backend,
// which takes them from the agent registry, so wizard agents read like built-in ones. The page never gets
// a folder, a pane id or chat text.
import { invoke } from '@tauri-apps/api/core';
import type { BridgeClient } from './bridge-types';
import { bridgeRefusalText } from './paneOutbox-bridge';

export interface PlanFiles {
  /** Turns whose summary told how many files changed. */
  knownTurns: number;
  changed: number;
  added: number;
  deleted: number;
}

export interface PlanRow {
  agentId: string;
  agentLabel: string;
  accountId?: string | null;
  accountLabel: string;
  turns: number;
  busyMs: number;
  files: PlanFiles;
  /** Runs whose own copy of the project was merged. */
  merged: number;
  limits: { hits: number; limitedMs: number };
}

export interface PlanTotals {
  turns: number;
  busyMs: number;
  filesChanged: number;
  added: number;
  deleted: number;
  merged: number;
  limitHits: number;
  limitedMs: number;
}

export interface PlanMonth {
  /** `2026-10` */
  id: string;
  /** `October 2026` */
  label: string;
  startMs: number;
  endMs: number;
}

export interface PlansReport {
  month: PlanMonth;
  /** Newest first. */
  months: PlanMonth[];
  rows: PlanRow[];
  totals: PlanTotals;
  notes: string[];
  limitsCountedSince?: string | null;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export interface PlansApi {
  /** `month` is `2026-10`; omit for this month. */
  report: (month?: string) => Promise<PlansReport>;
}

export const plansApi: PlansApi = {
  report: async (month) => {
    if (!inTauri()) throw new Error('Open Puppet Master on your computer to see your plans.');
    return invoke<PlansReport>('plans_report', { month: month ?? null });
  },
};

/**
 * The phone's copy: the `/plans` bridge route of the PC (read only, the same report). An old PC without the route
 * gets a plain sentence; a refused call reads as the PC's own sentence.
 */
export function bridgePlansApi(bridge: BridgeClient): PlansApi {
  return {
    report: async (month) => {
      if (!bridge.plansReport) throw new Error('This PC is too old to show your plans on the phone.');
      try {
        return await bridge.plansReport(month);
      } catch (error) {
        throw new Error(bridgeRefusalText(error, 'Could not load your plans.'));
      }
    },
  };
}

const MIN = 60_000;
const HOUR = 60 * MIN;

/** "42 min", "3 h 12 min", "3 h". Under a minute: "under 1 min". Nothing at all: "0 min". */
export function formatBusy(ms: number): string {
  if (ms <= 0) return '0 min';
  if (ms < MIN) return 'under 1 min';
  const minutes = Math.round(ms / MIN);
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** Waiting time in the words of the limits column: "45 min", "2.5 h", "7 h". */
export function formatWaiting(ms: number): string {
  if (ms < HOUR) return `${Math.max(1, Math.round(ms / MIN))} min`;
  const hours = ms / HOUR;
  const text = hours >= 10 ? String(Math.round(hours)) : String(Math.round(hours * 10) / 10);
  return `${text} h`;
}

function times(n: number): string {
  return n === 1 ? '1 time' : `${n} times`;
}

/** "Hit its limit 3 times · 7 h waiting". Nothing hit: "No limits hit". Hit with no recorded end: no waiting part. */
export function limitsText(limits: { hits: number; limitedMs: number }): string {
  if (limits.hits === 0 && limits.limitedMs === 0) return 'No limits hit';
  // A limit that began last month and ran into this one: no new hit, but this month's wait is real.
  if (limits.hits === 0) return `Waiting since last month · ${formatWaiting(limits.limitedMs)}`;
  const waiting = limits.limitedMs > 0 ? ` · ${formatWaiting(limits.limitedMs)} waiting` : '';
  return `Hit its limit ${times(limits.hits)}${waiting}`;
}

/** "12 files +340 −45", "No files changed", or a dash when no turn told. */
export function filesText(files: { knownTurns: number; changed: number; added: number; deleted: number }): string {
  if (files.knownTurns === 0) return '—';
  if (files.changed === 0) return 'No files changed';
  const noun = files.changed === 1 ? 'file' : 'files';
  return `${files.changed} ${noun} +${files.added} −${files.deleted}`;
}

export function turnsText(n: number): string {
  return String(n);
}

export function mergedText(n: number): string {
  return n === 0 ? '—' : String(n);
}

/** The month id before/after `id` in a newest-first list, or null at the ends. */
export function neighbour(months: readonly PlanMonth[], id: string, dir: 'older' | 'newer'): string | null {
  const at = months.findIndex((m) => m.id === id);
  if (at < 0) return null;
  const to = dir === 'older' ? at + 1 : at - 1;
  return months[to]?.id ?? null;
}
