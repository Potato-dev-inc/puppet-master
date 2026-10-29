// What broke it: when the tests went red, find the agent turn that broke them.
//
// The Rust side (checkpoints/bisect/* and commands/what_broke_it.rs) holds everything; this file is the
// typed command surface, the view shapes and the event name. Field names mirror the serde output exactly.
// `Limits`, `DeclaredView` and `SuggestionView` are snake_case (as the spec writes them); the other views
// are camelCase. Tauri maps the Rust argument names to camelCase (`pane_id` is `paneId`).
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { FileChange } from './cockpit-api';

/** Test limits (Rust `Limits`). The backend clamps them: runs 1..=30, minutes 1..=60. */
export interface Limits {
  max_runs: number;
  max_minutes: number;
}

/** One test command the person declared (Rust `DeclaredView`). */
export interface DeclaredView {
  index: number;
  name: string;
  command_text: string;
  timeout_min: number;
  warnings: string[];
}

/** A test command the backend found for the folder and the person can use (Rust `SuggestionView`). */
export interface SuggestionView {
  name: string;
  command_text: string;
  source: string;
}

/** One save point in the picker for "Tests last passed" (Rust `PointView`). Newest first. */
export interface PointView {
  checkpointId: string;
  label: string;
  atMs: number;
}

/** What the sheet shows before a run (Rust `Overview`). `whyNot` says why `can` is false. */
export interface Overview {
  tests: DeclaredView[];
  suggestions: SuggestionView[];
  points: PointView[];
  limits: Limits;
  can: boolean;
  whyNot: string | null;
  busy: string | null;
}

export type JobState = 'preparing' | 'running' | 'finishing' | 'done';

/** One search job as the sheet sees it (Rust `JobView`). `line` is the plain-words progress text. */
export interface JobView {
  id: string;
  state: JobState;
  pointNo: number;
  points: number;
  run: number;
  maxRuns: number;
  elapsedMs: number;
  maxMs: number;
  line: string;
  findingId: string | null;
}

/** File counts of one suspect turn (Rust `FileStatsLite`). */
export interface FileStats {
  count: number;
  added: number;
  deleted: number;
}

/**
 * One agent turn that could have broken the tests (Rust `Suspect`, camelCase here). `agentLabel` is
 * "An agent" when the turn summary is gone. `sentence` is already scrubbed.
 */
export interface SuspectView {
  paneKey: string;
  turn: number;
  agentLabel: string;
  agentId: string | null;
  account: string | null;
  atMs: number;
  sentence: string | null;
  files: FileStats | null;
}

/**
 * The kinds of result the sheet knows (3.7). The backend `kind` is the snake_case tag of `Outcome`.
 * `copy_differs` is the sanity gate (3.5 step 5).
 */
export type FindingKind =
  | 'culprit'
  | 'several'
  | 'between_turns'
  | 'not_sure'
  | 'always_red'
  | 'good_was_red'
  | 'tests_pass'
  | 'copy_differs'
  | 'failed';

/** One finished search (Rust `FindingView`). Never the folder path or argv beyond `command_text`. */
export interface FindingView {
  id: string;
  title: string;
  subtitle: string;
  /** The backend text for the kind. `kindOf` turns it into a known `FindingKind`. */
  kind: string;
  suspects: SuspectView[];
  failing: string[];
  /** Last lines of one failing run, already redacted. */
  tail: string;
  canUndo: boolean;
  undoneAtMs: number | null;
  /** When the tests last passed (unix ms), for a culprit, several suspects or a gap between turns. */
  lastGoodMs?: number | null;
}

/** The file list and, when one file was asked for, its unified diff (Rust `DiffView`). */
export interface DiffView {
  files: FileChange[];
  patch: string | null;
  truncated: boolean;
}

/** A dry run of "undo only this turn" (Rust `UndoPlanView`). Nothing is written by a plan. */
export interface UndoPlanView {
  writes: string[];
  removes: string[];
  conflicts: { path: string; why: string }[];
  blockedBy: string | null;
}

/** The result of an undo that was written (Rust `UndoReportView`). */
export interface UndoReportView {
  undoneAtMs: number;
  written: number;
  removed: number;
}

/** Payload of the `what-broke-it://changed` event. `findingId` is set when a result is ready. */
export interface WhatBrokeItChanged {
  jobId: string;
  findingId?: string | null;
}

export const WHAT_BROKE_IT_CHANGED_EVENT = 'what-broke-it://changed';

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const KNOWN_KINDS: readonly FindingKind[] = [
  'culprit',
  'several',
  'between_turns',
  'not_sure',
  'always_red',
  'good_was_red',
  'tests_pass',
  'copy_differs',
  'failed',
];

/** The sheet, a row or a result: what the backend kind means. Unknown text falls back to `failed`. */
export function kindOf(finding: Pick<FindingView, 'kind'>): FindingKind {
  return (KNOWN_KINDS as readonly string[]).includes(finding.kind) ? (finding.kind as FindingKind) : 'failed';
}

/** The sheet's first view for a pane: the declared tests, suggestions, save points and limits. */
export function wbOverview(paneId: string): Promise<Overview> {
  return invoke<Overview>('wb_overview', { paneId });
}

/** Save a test command the person typed or picked. The backend refuses shell operators. */
export function wbDeclare(paneId: string, name: string, commandText: string, timeoutMin: number): Promise<DeclaredView> {
  return invoke<DeclaredView>('wb_declare', { paneId, name, commandText, timeoutMin });
}

/** Start a search. `checkIndex` is an index into the declared list, never command text. */
export function wbStart(
  paneId: string,
  checkIndex: number,
  goodCheckpointId: string | null = null,
  limits: Limits | null = null,
  continueFrom: string | null = null,
): Promise<JobView> {
  return invoke<JobView>('wb_start', { paneId, checkIndex, goodCheckpointId, limits, continueFrom });
}

/** The job's current state. Call it again on each change event. */
export function wbStatus(jobId: string): Promise<JobView> {
  return invoke<JobView>('wb_status', { jobId });
}

/** Cancel a running job. The backend stops the test's process tree. */
export function wbStop(jobId: string): Promise<void> {
  return invoke<void>('wb_stop', { jobId });
}

/** One finding by id. */
export function wbFinding(findingId: string): Promise<FindingView> {
  return invoke<FindingView>('wb_finding', { findingId });
}

/** Every finding of one pane, newest first. */
export function wbFindings(paneId: string): Promise<FindingView[]> {
  return invoke<FindingView[]>('wb_findings', { paneId });
}

/** The file list of the culprit turn. With `path`, that file's unified diff (text only, capped by the backend). */
export function wbDiff(findingId: string, path: string | null = null): Promise<DiffView> {
  return invoke<DiffView>('wb_diff', { findingId, path });
}

/** A dry run of the undo. Writes nothing. */
export function wbUndoPlan(findingId: string): Promise<UndoPlanView> {
  return invoke<UndoPlanView>('wb_undo_plan', { findingId });
}

/** Undo only the culprit turn. All-or-nothing: a conflict rejects and writes nothing. */
export function wbUndo(findingId: string): Promise<UndoReportView> {
  return invoke<UndoReportView>('wb_undo', { findingId });
}

/** One run after an undo, to check that the tests pass now. */
export function wbRecheck(findingId: string): Promise<JobView> {
  return invoke<JobView>('wb_recheck', { findingId });
}

/** The saved limits (runs and minutes). */
export function wbSettingsGet(): Promise<Limits> {
  return invoke<Limits>('wb_settings_get');
}

/** Save new limits. The backend clamps them and returns what it kept. */
export function wbSettingsSet(limits: Limits): Promise<Limits> {
  return invoke<Limits>('wb_settings_set', { limits });
}

/** Calls `cb` when a job or a finding changes. Outside the desktop window nothing is listened to. */
export function onWhatBrokeItChanged(cb: (event: WhatBrokeItChanged) => void): Promise<UnlistenFn> {
  if (!inTauri()) return Promise.resolve(() => {});
  return listen<WhatBrokeItChanged>(WHAT_BROKE_IT_CHANGED_EVENT, (event) => cb(event.payload));
}
