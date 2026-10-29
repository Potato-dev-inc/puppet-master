// Review Desk on the desktop: the view shapes (mirroring `review_desk/view.rs` and the camelCase structs in
// `commands/review_desk.rs`), the 7 `review_desk_*` commands, the refresh events and the error helpers.
// Rust `Option<T>` arrives as `T | null`. Enum tags are snake_case (serde), view keys are camelCase.
// Names and argument keys follow SPEC n33 section 3.6 exactly. The words on screen are in the UI, not here.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** `Source` (snake_case): where the item came from. */
export type ReviewSource = 'copy' | 'mission' | 'night' | 'issue' | 'folder';

/** `Flag` (snake_case): one risk flag, in tier order (the riskiest first). */
export type ReviewFlag =
  | 'tests_weakened'
  | 'sensitive'
  | 'checks_failed'
  | 'tests_changed'
  | 'packages_changed'
  | 'big'
  | 'unknown';

/** `Level` (snake_case): High risk / Check first / Low risk. Shown as `●` / `◐` / `○`. */
export type ReviewLevel = 'high' | 'check' | 'low';

/** `FileRow.status`: added, modified or deleted. */
export type ReviewFileStatus = 'A' | 'M' | 'D';

/** `Keep` (tag `kind`, snake_case). `no` means Keep is off, and `why` says why. */
export type ReviewKeepKind = 'merge' | 'night_keep' | 'issue_push' | 'mark' | 'no';

/** `Throw` (tag `kind`, snake_case). `no` means Throw away is off, and `why` says why. */
export type ReviewThrowKind = 'discard' | 'night_throw' | 'issue_throw' | 'restore' | 'no';

/** The folder of a group: a root agent and the helpers it started. */
export interface ReviewGroup {
  rootPaneId: string;
  label: string;
}

/** One changed file in the list (at most 200 rows). */
export interface ReviewFileRow {
  path: string;
  status: ReviewFileStatus;
  added: number;
  deleted: number;
  binary: boolean;
}

export interface ReviewFiles {
  total: number;
  added: number;
  deleted: number;
  rows: ReviewFileRow[];
}

/** One plain reason that put the item where it is. */
export interface ReviewReason {
  flag: ReviewFlag;
  sentence: string;
}

/** What the primary action does for this item. */
export interface ReviewKeep {
  kind: ReviewKeepKind;
  /** `Merge...`, `Push as draft...`, `Looks good` or the disabled label. */
  label: string;
  into?: string | null;
  why?: string | null;
}

export interface ReviewThrow {
  kind: ReviewThrowKind;
  why?: string | null;
}

/** One row of the desk (`ItemView`). No keys of other items, no paths, no secrets. */
export interface ReviewItem {
  /** `copy:<handle>` | `night:<task id>` | `issue:<run id>` | `folder:<pane id>:<first>-<last>`. */
  key: string;
  source: ReviewSource;
  /** Sent back with every action; the backend refuses it when the item changed since the list. */
  fingerprint: string;
  title: string;
  agentLabel: string;
  whereLabel: string;
  paneId: string | null;
  finishedAtMs: number;
  turns: number;
  group: ReviewGroup | null;
  files: ReviewFiles;
  level: ReviewLevel;
  levelWord: string;
  tier: number;
  reasons: ReviewReason[];
  keep: ReviewKeep;
  throw: ReviewThrow;
  /** The pane Ask to fix writes into, when there is one. */
  fixPaneId?: string | null;
  /** Why Ask to fix is off, when it is. */
  fixWhyNot?: string | null;
}

export interface ReviewCounts {
  total: number;
  high: number;
  check: number;
  low: number;
}

/** `review_desk_list`: the ordered list, the counts and the plain notes of sources that failed. */
export interface ReviewDesk {
  items: ReviewItem[];
  counts: ReviewCounts;
  notes: string[];
}

/** `review_desk_detail`: one item with its patch (secrets masked, home folder as `~`). */
export interface ReviewDetail {
  item: ReviewItem;
  patch: string;
  patchTruncated: boolean;
  /** Night reason, issue push-check reasons, mission headline. */
  extra: string[];
}

/** The result of Keep or Throw away. `gone` is true when the item left the desk. */
export interface ReviewAct {
  sentence: string;
  gone: boolean;
}

/** The event names the desk refreshes on (SPEC n33 section 3.5 H). */
export const REVIEW_DESK_EVENT = 'review-desk://changed';
export const REVIEW_REFRESH_EVENTS = [
  REVIEW_DESK_EVENT,
  'worktree://changed',
  'night://changed',
  'issue-intake://changed',
  'turn://summary',
  'pty://panes-changed',
] as const;

/** The refresh waits this long after the last event before it reloads. */
export const REVIEW_REFRESH_DEBOUNCE_MS = 500;

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

/** The glyph for a risk level: `●` High risk, `◐` Check first, `○` Low risk. */
export function levelGlyph(level: ReviewLevel): string {
  if (level === 'high') return '●';
  if (level === 'check') return '◐';
  return '○';
}

/** `review_desk_list`: the full, ordered list. */
export const reviewDeskList = (): Promise<ReviewDesk> => call<ReviewDesk>('review_desk_list');

/** `review_desk_count`: the number for the sidebar button. */
export const reviewDeskCount = (): Promise<number> => call<number>('review_desk_count');

/** `review_desk_detail`: one item and its patch. */
export const reviewDeskDetail = (key: string): Promise<ReviewDetail> =>
  call<ReviewDetail>('review_desk_detail', { key });

/** `review_desk_keep`: Keep. Only called from the confirm sheet, or directly for "Looks good". */
export const reviewDeskKeep = (key: string, fingerprint: string): Promise<ReviewAct> =>
  call<ReviewAct>('review_desk_keep', { key, fingerprint });

/** `review_desk_throw_away`: called only when the 10 s Undo window ends. */
export const reviewDeskThrowAway = (key: string, fingerprint: string): Promise<ReviewAct> =>
  call<ReviewAct>('review_desk_throw_away', { key, fingerprint });

/** `review_desk_unmark`: undoes a "Looks good" on one pane. */
export const reviewDeskUnmark = (paneId: string): Promise<void> => call<void>('review_desk_unmark', { paneId });

/** `review_desk_fix`: puts the fixed draft in the pane's chat box. Nothing is sent. Resolves to the pane id. */
export const reviewDeskFix = (key: string, fingerprint: string, note: string): Promise<string> =>
  call<string>('review_desk_fix', { key, fingerprint, note });

/** The same calls as one object, so a component can take the slice it needs as a prop. */
export const reviewDeskApi = {
  list: reviewDeskList,
  count: reviewDeskCount,
  detail: reviewDeskDetail,
  keep: reviewDeskKeep,
  throwAway: reviewDeskThrowAway,
  unmark: reviewDeskUnmark,
  fix: reviewDeskFix,
};

/** A refusal: a code the UI can match, and the plain sentence to show. */
export interface ReviewDeskError {
  code: string;
  message: string;
}

const CODE_PREFIX = /^([A-Z][A-Z0-9_]{2,}):\s*/;
const FALLBACK_MESSAGE = 'Something went wrong. Try again.';

/**
 * The code and plain message of a failed Review Desk command. An `OperationError` arrives as the object
 * `{ code, message, recoverable, context }`; a plain error or string arrives as `CODE: message`.
 */
export function reviewDeskError(err: unknown): ReviewDeskError {
  const obj = typeof err === 'object' && err !== null ? (err as { code?: unknown; message?: unknown }) : null;
  let text = '';
  if (typeof err === 'string') text = err;
  else if (obj && typeof obj.message === 'string') text = obj.message;
  else if (err instanceof Error) text = err.message;
  const match = CODE_PREFIX.exec(text);
  const code = obj && typeof obj.code === 'string' ? obj.code : match ? match[1] : '';
  const message = match ? text.slice(match[0].length) : text;
  return { code, message: message || FALLBACK_MESSAGE };
}

/** Only the plain sentence of a failed command. */
export const reviewDeskErrorText = (err: unknown): string => reviewDeskError(err).message;

/**
 * Subscribes to the six refresh events of section 3.5 H. Calls `cb` once, 500 ms after the last event.
 * Returns a synchronous unsubscribe. Outside the desktop shell nothing is listened to.
 */
export function onReviewDeskRefresh(cb: () => void): () => void {
  if (!isTauri()) return () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  const offs: UnlistenFn[] = [];
  const fire = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      cb();
    }, REVIEW_REFRESH_DEBOUNCE_MS);
  };
  for (const name of REVIEW_REFRESH_EVENTS) {
    listen(name, () => fire())
      .then((off) => (disposed ? off() : offs.push(off)))
      .catch(() => {});
  }
  return () => {
    disposed = true;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    for (const off of offs.splice(0)) off();
  };
}
