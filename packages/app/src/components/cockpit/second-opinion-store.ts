// Review runs per pane, outside React so a review keeps going (and its answer lands) while the user
// switches tabs. Both the Receipt "Second opinion" panel and the Changes diff read it.
import { useSyncExternalStore } from 'react';
import {
  plainError,
  secondOpinionApi,
  type ParsedReview,
  type ReviewStart,
  type SecondOpinionApi,
  type SentFollowup,
} from '../../lib/second-opinion';

export interface ReviewRun {
  phase: 'starting' | 'running' | 'done' | 'failed';
  reviewer: string;
  start?: ReviewStart;
  review?: ParsedReview;
  error?: string;
}

const runs = new Map<string, ReviewRun>();
const listeners = new Set<() => void>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
/** Bumped per pane on every begin/clear so a late answer from an old run is ignored. */
const epoch = new Map<string, number>();

/** How often a running review is asked for its answer. */
export const POLL_MS = 2000;

function put(paneId: string, run: ReviewRun | undefined): void {
  if (run) runs.set(paneId, run);
  else runs.delete(paneId);
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useReviewRun(paneId: string): ReviewRun | undefined {
  return useSyncExternalStore(subscribe, () => runs.get(paneId));
}

export function getReviewRun(paneId: string): ReviewRun | undefined {
  return runs.get(paneId);
}

function bump(paneId: string): number {
  const next = (epoch.get(paneId) ?? 0) + 1;
  epoch.set(paneId, next);
  const t = timers.get(paneId);
  if (t) clearTimeout(t);
  timers.delete(paneId);
  return next;
}

/** Start a review of `paneId` by `reviewer` and keep asking until it answers or fails. */
export async function beginReview(paneId: string, reviewer: string, api: SecondOpinionApi = secondOpinionApi): Promise<void> {
  const mine = bump(paneId);
  put(paneId, { phase: 'starting', reviewer });
  let start: ReviewStart;
  try {
    start = await api.start(paneId, reviewer);
  } catch (err) {
    if (epoch.get(paneId) === mine) put(paneId, { phase: 'failed', reviewer, error: plainError(err) });
    return;
  }
  if (epoch.get(paneId) !== mine) return;
  put(paneId, { phase: 'running', reviewer, start });
  const tick = async () => {
    if (epoch.get(paneId) !== mine) return;
    try {
      const p = await api.poll(start.project, start.handle);
      if (epoch.get(paneId) !== mine) return;
      if (p.state === 'running') {
        timers.set(paneId, setTimeout(() => void tick(), POLL_MS));
      } else if (p.state === 'done' && p.review) {
        put(paneId, { phase: 'done', reviewer, start, review: p.review });
      } else {
        put(paneId, { phase: 'failed', reviewer, start, error: p.error ?? 'The review did not finish.' });
      }
    } catch (err) {
      if (epoch.get(paneId) === mine) put(paneId, { phase: 'failed', reviewer, start, error: plainError(err) });
    }
  };
  timers.set(paneId, setTimeout(() => void tick(), POLL_MS));
}

/** Stop a running review (best effort on the backend) and forget it. */
export async function cancelReview(paneId: string, api: SecondOpinionApi = secondOpinionApi): Promise<void> {
  const run = runs.get(paneId);
  bump(paneId);
  put(paneId, undefined);
  if (run?.start && run.phase === 'running') {
    try {
      await api.cancel(run.start.project, run.start.handle);
    } catch {
      // Already finished or gone: nothing left to stop.
    }
  }
}

/** Forget a finished review (hides the notes). */
export function clearReview(paneId: string): void {
  bump(paneId);
  put(paneId, undefined);
}

/** Send a finding or a free comment to the agent that wrote the code. */
export function sendToAuthor(
  paneId: string,
  file: string,
  line: number | null,
  text: string,
  api: SecondOpinionApi = secondOpinionApi,
): Promise<SentFollowup> {
  return api.send(paneId, file, line, text).catch((err) => {
    throw new Error(plainError(err));
  });
}

/** Test helper: drop every run and timer. */
export function resetReviews(): void {
  timers.forEach((t) => clearTimeout(t));
  timers.clear();
  runs.clear();
  epoch.clear();
  listeners.forEach((l) => l());
}
