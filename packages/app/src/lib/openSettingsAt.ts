// Open Settings on one section and one exact row: the row is scrolled into view, gets a grey glow for 2 s,
// and (when asked) its first text field takes the cursor. Every card that says "Open Settings" uses this.
//
// The page change itself goes through `open` (the cockpit's settings opener takes a tab, and a row id is a
// valid tab: see `lib/settings-target.ts`). Without one, a window event `pm:open-settings` carries the tab.
import type { SectionId } from '../components/settings/model';

export const SETTINGS_GLOW_MS = 2000;
const GLOW_ATTR = 'data-glow';
const FIND_TIMEOUT_MS = 1500;

let registered: ((tab: string) => void) | null = null;

/** The cockpit registers how to open Settings on a tab or row id (its settings opener). Pass null to unregister. */
export function registerSettingsOpener(open: ((tab: string) => void) | null): void {
  registered = open;
}

export interface OpenSettingsOptions {
  /** Opens Settings on a tab or row id. Defaults to the registered opener, else dispatching `pm:open-settings`. */
  open?: (tab: string) => void;
  /** Put the cursor in the row's first text field once it is shown. */
  focusField?: boolean;
}

/** Open Settings on `section`, or on `rowId` inside it when given. */
export function openSettingsAt(section: SectionId, rowId?: string, options: OpenSettingsOptions = {}): void {
  const tab = rowId ?? section;
  const open = options.open ?? registered;
  if (open) open(tab);
  else if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent('pm:open-settings', { detail: { tab } }));
  if (rowId) glowRow(rowId, options.focusField === true);
}

/** Find the row once the page has rendered (it may mount a moment after the call), then glow it. */
function glowRow(rowId: string, focusField: boolean): void {
  if (typeof document === 'undefined') return;
  const started = Date.now();
  const tryFind = () => {
    if (typeof document === 'undefined') return; // the page went away (a test ended, the window closed)
    const el = [...document.querySelectorAll<HTMLElement>('[data-row]')].find((r) => r.getAttribute('data-row') === rowId);
    if (!el) {
      if (Date.now() - started < FIND_TIMEOUT_MS) setTimeout(tryFind, 50);
      return;
    }
    const reduce = typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    el.scrollIntoView?.({ block: 'center', behavior: reduce ? 'auto' : 'smooth' });
    el.setAttribute(GLOW_ATTR, 'true');
    if (focusField) el.querySelector<HTMLInputElement>('input[type="text"], input[type="password"], input:not([type])')?.focus();
    setTimeout(() => el.removeAttribute(GLOW_ATTR), SETTINGS_GLOW_MS);
  };
  tryFind();
}

/** The row id of "Check my setup" (Settings > Agents). "Repair install" buttons open this row. */
export const SETUP_CHECK_ROW_ID = 'setup-check';
