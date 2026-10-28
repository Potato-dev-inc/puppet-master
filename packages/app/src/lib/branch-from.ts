// Branch from here: a separate copy of the files at a save point, with an agent started in it.
//
// The Rust side (src-tauri/src/branch_from) holds everything; this file is the typed command surface,
// the view shapes and the event name (SPEC n33 section 3.8). Field names mirror the serde names
// exactly: `BranchFrom` is snake_case, the views and `Preview` are camelCase. Tauri maps the Rust
// argument names to camelCase (`pane_id` is `paneId`). Offers reuse the keep-going shape.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { KeepGoingOffer } from './keep-going';

/** Which side of a chat turn the branch starts from (Rust `When`). */
export type BranchWhen = 'before' | 'after';

/** Where the branch starts (Rust `BranchFrom`). The window sends it; the server checks it again. */
export type BranchFrom =
  | { kind: 'message'; user_item_id: string; when: BranchWhen }
  | { kind: 'save_point'; checkpoint_id: string };

/** One picker row for the new agent (Rust `Offer`). Same shape as a keep-going offer. */
export type BranchOffer = KeepGoingOffer;

/** The save point the branch starts from (Rust `PointView`). */
export interface BranchPointView {
  checkpointId: string;
  label: string;
  atMs: number;
  files: number;
}

/** The brief text the new agent would get, when the sheet asks for it. */
export interface BranchBriefPreview {
  text: string;
  bytes: number;
  trimmed: string[];
}

/** What the branch sheet shows before anything is written (Rust `Preview`). Never writes. */
export interface BranchPreview {
  fromLabel: string;
  point: BranchPointView | null;
  canBranch: boolean;
  /** Why the branch can't start, in plain words. Null when it can. */
  whyNot: string | null;
  offers: BranchOffer[];
  /** The message of the picked turn, for "Before this turn". Null otherwise. */
  prefill: string | null;
  turns: number;
  openBranches: number;
  brief: BranchBriefPreview | null;
}

/** Where a branch is in its life (Rust `State`, as the view names it). */
export type BranchState = 'preparing' | 'starting' | 'running' | 'failed';

/** Set once the person keeps or throws away the branch copy in Changes (Rust `closed`). */
export type BranchClosed = 'kept' | 'thrown_away';

/** One branch as the windows see it (Rust `BranchView`). No path, message or brief text. */
export interface BranchView {
  id: string;
  fromLabel: string;
  toLabel: string;
  fromPaneId: string;
  toPaneId: string;
  atMs: number;
  /** Example: "after turn 5 · 14:02". */
  pointLabel: string;
  state: BranchState;
  stateText: string;
  turns: number;
  briefBytes: number;
  branchName: string;
  closed: BranchClosed | null;
  failedFiles: number;
}

/** Both sides of one pane's branches (Rust `BranchLineage`). `from` = "Branched from", `children` = "Branches". */
export interface BranchLineage {
  from: BranchView | null;
  children: BranchView[];
}

/** Payload of the `branch://changed` event. */
export interface BranchChanged {
  paneIds: string[];
}

export const BRANCH_CHANGED_EVENT = 'branch://changed';

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** What the sheet shows for a save point or a chat turn. Writes nothing. Rejects for a pane that can't branch. */
export function branchPreview(
  paneId: string,
  from: BranchFrom,
  target?: string | null,
  message?: string | null,
): Promise<BranchPreview> {
  return invoke<BranchPreview>('branch_preview', { paneId, from, target: target ?? null, message: message ?? null });
}

/** Start the branch the person chose. `target` is the `agent` or `agent@account` text. Returns the `preparing` view. */
export function branchStart(paneId: string, from: BranchFrom, target: string, message: string): Promise<BranchView> {
  return invoke<BranchView>('branch_start', { paneId, from, target, message });
}

/** Both sides of a pane's branches. Outside the desktop window there are none. */
export function branchLineage(paneId: string): Promise<BranchLineage> {
  if (!inTauri()) return Promise.resolve({ from: null, children: [] });
  return invoke<BranchLineage>('branch_lineage', { paneId });
}

/** The stored brief of one branch (already redacted). Desktop window only. */
export function branchBrief(id: string): Promise<string> {
  return invoke<string>('branch_brief', { id });
}

/** Calls `cb` when branch records change. Outside the desktop window nothing is listened to. */
export function onBranchChanged(cb: (event: BranchChanged) => void): Promise<UnlistenFn> {
  if (!inTauri()) return Promise.resolve(() => {});
  return listen<BranchChanged>(BRANCH_CHANGED_EVENT, (event) => cb(event.payload));
}

/**
 * The line a branch shows in plain words. The server text wins; the fallback only covers an empty
 * string so the banner never goes blank.
 */
export function branchStateText(view: BranchView): string {
  const text = view.stateText.trim();
  if (text) return text;
  if (view.state === 'running') return 'Running in a separate copy';
  if (view.state === 'failed') return `${view.toLabel} couldn't start`;
  if (view.state === 'starting') return `${view.toLabel} is starting`;
  return 'Making a copy from the save point';
}
