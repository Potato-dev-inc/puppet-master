// The split view remembered between starts: the two panes and the divider's ratio, kept in localStorage.
// Every storage call is wrapped in try/catch: a blocked or full storage just means no memory.
import { splitFits } from './main-view';

export const SPLIT_MEMORY_KEY = 'pm.cockpit.split';

export interface SplitMemory {
  /** The pane on the left (the selected pane when the split was open). */
  left: string;
  /** The pane on the right (the pane "to the side"). */
  right: string;
  /** The left side's share of the room, 0 to 1. */
  ratio: number;
}

/** Reads a saved split from its stored text. Anything that is not the expected shape is null. */
export function parseSplitMemory(raw: string | null): SplitMemory | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<SplitMemory> | null;
    if (!v || typeof v.left !== 'string' || typeof v.right !== 'string' || !v.left || !v.right) return null;
    const ratio = typeof v.ratio === 'number' && Number.isFinite(v.ratio) && v.ratio > 0 && v.ratio < 1 ? v.ratio : 0.5;
    return { left: v.left, right: v.right, ratio };
  } catch {
    return null;
  }
}

export function readSplitMemory(): SplitMemory | null {
  try {
    return parseSplitMemory(localStorage.getItem(SPLIT_MEMORY_KEY));
  } catch {
    return null;
  }
}

export function writeSplitMemory(memory: SplitMemory): void {
  try {
    localStorage.setItem(SPLIT_MEMORY_KEY, JSON.stringify(memory));
  } catch {
    // Storage is blocked or full: the split still works, it is just not remembered.
  }
}

export function clearSplitMemory(): void {
  try {
    localStorage.removeItem(SPLIT_MEMORY_KEY);
  } catch {
    // Nothing to clear when storage is blocked.
  }
}

/**
 * The saved split when it can come back on start: the window is wide enough, both panes still exist and
 * they are two different panes. Otherwise null (no split), and the saved text is left alone.
 */
export function restorableSplit(saved: SplitMemory | null, opts: { width: number; hasPane: (id: string) => boolean }): SplitMemory | null {
  if (!saved || !splitFits(opts.width)) return null;
  if (saved.left === saved.right) return null;
  if (!opts.hasPane(saved.left) || !opts.hasPane(saved.right)) return null;
  return saved;
}
