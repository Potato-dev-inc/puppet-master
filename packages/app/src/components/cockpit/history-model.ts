// Pure helpers for the History tab.
import type { Checkpoint } from '../../lib/cockpit-api';

/** "just now", "5 min ago", "3 h ago", "2 d ago". */
export function formatAgo(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 45) return 'just now';
  const m = Math.floor(s / 60);
  if (m < 60) return `${Math.max(1, m)} min ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} h ago`;
  return `${Math.floor(h / 24)} d ago`;
}

/** Row title. The backend label is plain English already ("Before turn 3"). */
export function checkpointTitle(c: Pick<Checkpoint, 'label'>): string {
  return c.label || 'Checkpoint';
}

/** "3 files +12 −4", or "No file changes". */
export function formatDelta(c: Pick<Checkpoint, 'files' | 'added' | 'deleted'>): string {
  if (c.files === 0) return 'No file changes';
  return `${c.files} ${c.files === 1 ? 'file' : 'files'} +${c.added} −${c.deleted}`;
}
