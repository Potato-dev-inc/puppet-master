// "Scores on your code": which agent works best in this project, from your own history. Shape mirrors the Rust
// `scores::model` types (src-tauri/src/scores/model.rs, camelCase). The page never gets a folder path, a pane id,
// a prompt or chat text. Numbers are plain counts ("8 of 10"); there is no overall score and no ranking here.
import { invoke } from '@tauri-apps/api/core';
import { formatBusy } from './plans-report';

/** `none` = no turns, `not_enough` = 1 to 4, `early` = 5 to 9, `steady` = 10 or more. */
export type ScoreSample = 'none' | 'not_enough' | 'early' | 'steady';

export interface ScoreRate {
  count: number;
  total: number;
  sample: ScoreSample;
  /** 90% Wilson interval, 0 to 1. */
  low: number;
  high: number;
}

export interface ScorePerMerged {
  runs: number;
  turnsMedian: number;
  busyMsMedian: number;
}

/** Metrics that can carry the `ahead` chip. Asked you is never marked. */
export type ScoreAheadKey = 'endedOk' | 'checksGreen' | 'merged' | 'undone';

export interface ScoreRow {
  agentId: string;
  agentLabel: string;
  accountId?: string | null;
  accountLabel: string;
  turns: number;
  /** false -> the page shows "Not enough runs yet". */
  enough: boolean;
  endedOk: ScoreRate;
  checksGreen: ScoreRate;
  merged: ScoreRate;
  undone: ScoreRate;
  askedYou: ScoreRate;
  /** Only with 3 or more merged runs. */
  perMerged?: ScorePerMerged | null;
  waitingReview: number;
  turnMsMedian?: number | null;
  linesMedian?: number | null;
  ahead: ScoreAheadKey[];
}

export interface ScoreProject {
  id: string;
  /** The folder's last name only. */
  label: string;
  turns: number;
}

export interface ScoresReport {
  /** `project` or `all`. */
  scope: string;
  project?: ScoreProject | null;
  /** Newest activity first, at most 50. */
  projects: ScoreProject[];
  /** 30, 90, or null for all time. */
  days?: number | null;
  sinceMs: number;
  minSample: number;
  rows: ScoreRow[];
  notes: string[];
}

export interface ScoresApi {
  /** `project` is a project id from the last answer, or a folder path. Omit for all projects. */
  report: (project?: string, days?: number | null) => Promise<ScoresReport>;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const scoresApi: ScoresApi = {
  report: async (project, days) => {
    if (!inTauri()) throw new Error('Open Puppet Master on your computer to see your scores.');
    return invoke<ScoresReport>('agent_scores', { project: project ?? null, days: days ?? null });
  },
};

/** The one-line explanation of the `ahead` chip (3.3). Shown as the chip's title. */
export const AHEAD_TITLE = 'With this many runs the gap is unlikely to be luck.';

/** The window choices of the page, in the order of the select. */
export const SCORE_WINDOWS: readonly (number | null)[] = [30, 90, null];

/** "Last 30 days", "Last 90 days", "All time". */
export function windowLabel(days: number | null | undefined): string {
  if (days === null || days === undefined) return 'All time';
  return `Last ${days} days`;
}

function clampedCount(rate: ScoreRate): number {
  return Math.min(Math.max(rate.count, 0), Math.max(rate.total, 0));
}

/** "8 of 10" for early and steady samples. A dash when there are too few runs to show a number. */
export function rateText(rate: ScoreRate): string {
  if (rate.sample === 'none' || rate.sample === 'not_enough') return '-';
  return `${clampedCount(rate)} of ${rate.total}`;
}

/** "80%", only for a steady sample (10 or more). Null otherwise. */
export function percentText(rate: ScoreRate): string | null {
  if (rate.sample !== 'steady' || rate.total <= 0) return null;
  return `${Math.round((clampedCount(rate) / rate.total) * 100)}%`;
}

/** Why a cell shows a dash: "Only 3 so far. A number needs 5." Null once there is a number to show. */
export function sampleHint(rate: ScoreRate, minSample: number): string | null {
  if (rate.sample !== 'none' && rate.sample !== 'not_enough') return null;
  return `Only ${rate.total} so far. A number needs ${minSample}.`;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** "2 turns · 14 min" for the typical merged run. Null when there is no typical value (under 3 merged runs). */
export function perMergedText(pm: ScorePerMerged | null | undefined): string | null {
  if (!pm) return null;
  return `${plural(pm.turnsMedian, 'turn', 'turns')} · ${formatBusy(pm.busyMsMedian)}`;
}

/** "Not enough runs yet. 3 turns so far." */
export function notEnoughText(row: Pick<ScoreRow, 'turns'>): string {
  return `Not enough runs yet. ${plural(row.turns, 'turn', 'turns')} so far.`;
}

/**
 * The descriptive line under an agent's name: "Typical turn 4 min · 38 lines · 2 waiting for review".
 * Parts without a value are left out. Empty when there is nothing to say.
 */
export function detailLine(row: Pick<ScoreRow, 'turnMsMedian' | 'linesMedian' | 'waitingReview'>): string {
  const parts: string[] = [];
  if (row.turnMsMedian !== null && row.turnMsMedian !== undefined) {
    parts.push(`Typical turn ${formatBusy(row.turnMsMedian)}`);
  }
  if (row.linesMedian !== null && row.linesMedian !== undefined) {
    parts.push(plural(row.linesMedian, 'line', 'lines'));
  }
  if (row.waitingReview > 0) parts.push(`${row.waitingReview} waiting for review`);
  return parts.join(' · ');
}
