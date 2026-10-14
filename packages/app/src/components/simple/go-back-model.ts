// "Go back" rows from the checkpoints PM already saves (History tab) and the turn summaries.
// A row is a version she can return to: the project as it was when a step finished, newest first.
// No turn numbers, hashes or the word for a checkpoint reach the screen.
import type { Checkpoint, TurnSummary } from '../../lib/cockpit-api';
import { F, T } from './copy';

export interface Version {
  /** The checkpoint to restore. */
  id: string;
  atMs: number;
  title: string;
  /** The newest version when nothing is running: the project as it is now. */
  current: boolean;
}

export function versionsFrom(
  items: Pick<Checkpoint, 'id' | 'turn' | 'kind' | 'atMs'>[],
  summaries: Pick<TurnSummary, 'turn' | 'files'>[],
  agent: string,
  working: boolean,
): Version[] {
  const byTurn = new Map<number, Pick<Checkpoint, 'id' | 'turn' | 'kind' | 'atMs'>>();
  let start: Pick<Checkpoint, 'id' | 'turn' | 'kind' | 'atMs'> | null = null;
  for (const item of items) {
    if (item.kind === 'after') {
      const seen = byTurn.get(item.turn);
      if (!seen || item.atMs > seen.atMs) byTurn.set(item.turn, item);
    } else if (item.kind === 'before') {
      if (!start || item.turn < start.turn || (item.turn === start.turn && item.atMs < start.atMs)) start = item;
    }
  }
  const rows: Version[] = [...byTurn.values()]
    .sort((a, b) => b.atMs - a.atMs)
    .map((item) => {
      const summary = summaries.find((s) => s.turn === item.turn);
      const count = summary?.files?.count;
      return {
        id: item.id,
        atMs: item.atMs,
        title: count === undefined ? F.madeChanges(agent) : F.changedSummary(agent, count),
        current: false,
      };
    });
  if (rows.length > 0 && !working) rows[0].current = true;
  if (start) rows.push({ id: start.id, atMs: start.atMs, title: T.goBackStart, current: false });
  return rows;
}

/** "14:05" in the person's own clock settings. */
export function clockText(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/** "just now", "5 minutes ago", "2 hours ago", "1 day ago". */
export function agoText(ms: number): string {
  const minutes = Math.floor(Math.max(0, ms) / 60_000);
  if (minutes < 60) return F.minutesAgo(minutes);
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? F.hoursAgo(hours) : F.daysAgo(Math.floor(hours / 24));
}
