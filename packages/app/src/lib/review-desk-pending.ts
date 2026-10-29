// Review Desk "Throw away" with a 10 s Undo window (SPEC n33 section 3.5 E). A module-level store, not a
// component, so the countdown keeps running when the person leaves the desk. Only one throw is pending at a
// time. Nothing is thrown away until the window ends; if the app closes first, nothing happens.
import { reviewDeskErrorText } from './review-desk';

/** How long the person has to press Undo. */
export const UNDO_MS = 10_000;

/** Shown when a second throw is started while one is still in its window. */
export const FINISH_FIRST_SENTENCE = 'Finish the one in progress first';

/** What the toast row needs. The callback that does the throw is kept inside the store. */
export interface PendingThrow {
  key: string;
  title: string;
  fingerprint: string;
  /** Epoch ms when the window ends. */
  endsAt: number;
}

export type StartThrowResult = { started: true } | { started: false; sentence: string };

interface Entry extends PendingThrow {
  run: () => Promise<unknown>;
  onError?: (sentence: string) => void;
  timer: ReturnType<typeof setTimeout>;
}

type Listener = (pending: PendingThrow | null) => void;

let current: Entry | null = null;
let snapshot: PendingThrow | null = null;
const listeners = new Set<Listener>();

function publish(): void {
  snapshot = current
    ? { key: current.key, title: current.title, fingerprint: current.fingerprint, endsAt: current.endsAt }
    : null;
  for (const listener of [...listeners]) listener(snapshot);
}

/** The throw in its Undo window, or null. The same object until the state changes. */
export function pending(): PendingThrow | null {
  return snapshot;
}

/** Calls `listener` with the new state on every change. Returns the unsubscribe function. */
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

async function commit(entry: Entry): Promise<void> {
  if (current !== entry) return;
  current = null;
  publish();
  try {
    await entry.run();
  } catch (err) {
    entry.onError?.(reviewDeskErrorText(err));
  }
}

/**
 * Starts the Undo window for one item. `run` (the real throw) is called once when the window ends. If `run`
 * fails, `onError` gets the plain sentence so the row can come back. A second start while one is pending is
 * refused and nothing is committed.
 */
export function startThrow(
  item: { key: string; title: string; fingerprint: string },
  run: () => Promise<unknown>,
  onError?: (sentence: string) => void,
): StartThrowResult {
  if (current) return { started: false, sentence: FINISH_FIRST_SENTENCE };
  const entry: Entry = {
    key: item.key,
    title: item.title,
    fingerprint: item.fingerprint,
    endsAt: Date.now() + UNDO_MS,
    run,
    onError,
    timer: setTimeout(() => void commit(entry), UNDO_MS),
  };
  current = entry;
  publish();
  return { started: true };
}

/** Cancels the pending throw (Z, the Undo button, or a second X on the same row). False when none is pending. */
export function undo(): boolean {
  if (!current) return false;
  clearTimeout(current.timer);
  current = null;
  publish();
  return true;
}
