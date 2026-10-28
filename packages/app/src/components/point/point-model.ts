// Pure rules for the Point box: how a pick is named, how the target list is grouped, and which key does what.
// No React and no Tauri here, so the box stays a thin view over these.
import type { PickView, Target } from '../../lib/point-api';

/** The longest request the box accepts (characters). */
export const MAX_REQUEST = 2000;

/** "PricingCard · src/components/PricingCard.tsx:42", or whichever parts the page gave. Null when it gave neither. */
export function sourceLine(p: PickView): string | null {
  const parts: string[] = [];
  if (p.component) parts.push(p.component);
  if (p.source?.relPath) {
    const line = p.source.line;
    parts.push(typeof line === 'number' && line > 0 ? `${p.source.relPath}:${line}` : p.source.relPath);
  }
  return parts.length > 0 ? parts.join(' · ') : null;
}

/** Same-folder panes first, then the rest. Empty groups are left out. Keeps the order the backend sent. */
export function groupTargets(ts: Target[]): { label: string; items: Target[] }[] {
  const same = ts.filter((t) => t.sameFolder);
  const other = ts.filter((t) => !t.sameFolder);
  const groups: { label: string; items: Target[] }[] = [];
  if (same.length > 0) groups.push({ label: 'Same folder', items: same });
  if (other.length > 0) groups.push({ label: 'Other panes', items: other });
  return groups;
}

/**
 * Enter sends, Escape hides. Shift+Enter is a new line, and keys pressed while an input method is
 * composing text (Japanese, Chinese, ...) belong to that method, so they do nothing here.
 */
export function boxKey(e: { key: string; shiftKey: boolean; isComposing: boolean }): 'send' | 'hide' | null {
  if (e.isComposing) return null;
  if (e.key === 'Enter') return e.shiftKey ? null : 'send';
  if (e.key === 'Escape' || e.key === 'Esc') return 'hide';
  return null;
}
