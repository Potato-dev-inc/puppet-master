// The cockpit's main area shows exactly one thing. `CockpitApp` keeps that as ONE value, and every
// click is a transition on it, so "the last click wins" holds by construction (no stack of
// booleans whose render order decides).

export type MainView = 'inbox' | 'pane' | 'doctor' | 'memory' | 'plans' | 'morning' | 'scores' | 'review' | 'spawn' | 'settings' | 'companion' | 'store';

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

// Split view sizing. The ratio is the left side's share of the room both sides share (the window minus the divider).
// The CSS in cockpit-split.css uses the same minimum, so a side never gets narrower than this.

/** The narrowest a side may get, in px. */
export const SPLIT_SIDE_MIN_PX = 320;
/** The divider's hit area, in px (its line is drawn in the middle of it). */
export const SPLIT_DIVIDER_PX = 9;
/** One Left or Right arrow press on the focused divider moves it this far, in px. */
export const SPLIT_KEY_STEP_PX = 24;
/** The even split. A double-click on the divider goes back here. */
export const SPLIT_DEFAULT_RATIO = 0.5;

/**
 * Keeps `ratio` so both sides are at least SPLIT_SIDE_MIN_PX wide. `span` is the px both sides share (the
 * split's width minus the divider). Too narrow for two minimum sides: the even split.
 */
export function clampSplitRatio(ratio: number, span: number): number {
  if (!Number.isFinite(ratio) || !(span >= SPLIT_SIDE_MIN_PX * 2)) return SPLIT_DEFAULT_RATIO;
  const low = SPLIT_SIDE_MIN_PX / span;
  return Math.min(1 - low, Math.max(low, ratio));
}

/** The ratio after a Left (negative) or Right (positive) arrow: the divider moves `deltaPx` px. */
export function nudgeSplitRatio(ratio: number, deltaPx: number, span: number): number {
  if (!(span > 0)) return clampSplitRatio(ratio, span);
  return clampSplitRatio(ratio + deltaPx / span, span);
}

/** The ratio for a divider dragged so the left side is `leftPx` px wide. */
export function ratioFromLeftPx(leftPx: number, span: number): number {
  if (!(span > 0)) return SPLIT_DEFAULT_RATIO;
  return clampSplitRatio(leftPx / span, span);
}

/** Whether a platform string is macOS (Cmd is the main modifier there, Ctrl elsewhere). */
export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  return /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || '');
}

/**
 * Ctrl+Alt+Left / Ctrl+Alt+Right (Cmd+Alt on a Mac) move the focus to the left or right side. Any other
 * modifier, or any other key, is not ours: null. The caller only asks while the split is open.
 */
export function splitFocusKey(e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>, mac: boolean): SplitSide | null {
  const mainModifier = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!mainModifier || !e.altKey || e.shiftKey) return null;
  if (e.key === 'ArrowLeft') return 'left';
  if (e.key === 'ArrowRight') return 'right';
  return null;
}
