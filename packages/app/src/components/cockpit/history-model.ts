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

/**
 * Folder key for comparing two pane working directories: forward slashes, no trailing separator,
 * and lower case for Windows paths (drive letter or UNC), which are case-insensitive.
 */
export function folderKey(cwd: string | null | undefined): string | null {
  if (!cwd || !cwd.trim()) return null;
  const slashed = cwd.trim().replace(/\\/g, "/");
  const trimmed = slashed.replace(/\/+$/, "") || "/";
  const windows = /^[a-zA-Z]:/.test(trimmed) || trimmed.startsWith("//");
  return windows ? trimmed.toLowerCase() : trimmed;
}

/** Display labels of the other panes whose folder is the same as this pane's folder. */
export function panesSharingFolder(
  panes: { info: { id: string; cwd: string | null } }[],
  names: Map<string, string>,
  paneId: string,
): string[] {
  const self = panes.find((p) => p.info.id === paneId);
  const key = folderKey(self?.info.cwd);
  if (!key) return [];
  return panes
    .filter((p) => p.info.id !== paneId && folderKey(p.info.cwd) === key)
    .map((p) => names.get(p.info.id) ?? p.info.id);
}

/**
 * The restore warning line, or null when no other pane shares the folder. Names up to two
 * labels, then "and N more".
 */
export function sharedFolderWarning(labels: string[]): string | null {
  if (labels.length === 0) return null;
  const shown = labels.slice(0, 2);
  const rest = labels.length - shown.length;
  let names: string;
  if (rest > 0) names = `${shown.join(", ")} and ${rest} more`;
  else if (shown.length === 2) names = `${shown[0]} and ${shown[1]}`;
  else names = shown[0];
  return labels.length === 1
    ? `${names} also works in this folder. Its changes since then will be undone too.`
    : `${names} also work in this folder. Their changes since then will be undone too.`;
}
