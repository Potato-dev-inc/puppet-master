// The cockpit's main area shows exactly one thing. `CockpitApp` keeps that as ONE value, and every
// click is a transition on it, so "the last click wins" holds by construction (no stack of
// booleans whose render order decides).

export type MainView = 'inbox' | 'pane' | 'doctor' | 'memory' | 'spawn' | 'settings' | 'companion';

/**
 * Where the main area rests when nothing was opened on top: an agent that is starting, else the
 * selected pane, else the inbox. Used after the selection or the spawn changes, and when "Set up
 * agents" or "Project memory" is closed.
 */
export function homeView(hasSpawn: boolean, selectedId: string | null): MainView {
  if (hasSpawn) return 'spawn';
  return selectedId ? 'pane' : 'inbox';
}

/**
 * The view right after the person picks `id` (a pane, or `null` for the inbox). A running spawn
 * stays (picking a pane does not cancel it); a failed one is cleared by the selection, so it
 * no longer counts.
 */
export function viewAfterSelect(spawn: { failed: string | null } | null, id: string | null): MainView {
  const spawnStays = spawn !== null && !(id !== null && spawn.failed);
  return homeView(spawnStays, id);
}

// Split view: two chats side by side. The left side is the selected pane; `split` is the pane on the
// right (or null when the view is not split). Focus is which side has the person's attention.

/** Below this window width the split view is off whatever the person chose. */
export const SPLIT_MIN_WIDTH = 900;

export type SplitSide = 'left' | 'right';

/** Whether the window is wide enough to hold two chats. */
export function splitFits(width: number): boolean {
  return width >= SPLIT_MIN_WIDTH;
}

/**
 * The pane that pairs with `selectedId` when the split opens: the one above it in the list, else the
 * one below it (the first pane has no one above). Null when there is no selection or no second pane.
 */
export function splitPartner(paneIds: readonly string[], selectedId: string | null): string | null {
  if (!selectedId) return null;
  const at = paneIds.indexOf(selectedId);
  if (at < 0) return null;
  if (at > 0) return paneIds[at - 1];
  return paneIds[at + 1] ?? null;
}

/**
 * The split after the selection moves from `prev` to `next`. Picking the inbox ends the split. Picking
 * the pane on the right swaps the two: the old left pane goes to the right. Any other pick keeps it.
 */
export function splitAfterSelect(split: string | null, prev: string | null, next: string | null): string | null {
  if (next === null) return null;
  if (next === split) return prev;
  return split;
}

/**
 * Whether a split can stay on: the window is wide enough, a pane is selected, the right pane still
 * exists and is not the selected pane itself. Any "no" turns the split off.
 */
export function splitStillOn(split: string | null, opts: { selectedId: string | null; width: number; hasPane: (id: string) => boolean }): boolean {
  if (split === null) return false;
  return splitFits(opts.width) && opts.selectedId !== null && split !== opts.selectedId && opts.hasPane(split);
}

/** Ctrl+\ with no other modifier: the split view toggles. */
export function isSplitShortcut(e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>): boolean {
  return e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && e.key === '\\';
}
