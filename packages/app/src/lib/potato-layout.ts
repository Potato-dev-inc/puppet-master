// The Potato Bot page's three regions (bot list / thread / right bot panel): their limits, the clamps, and what is
// remembered between runs. Pure functions plus guarded storage, so tests need no window. Storage can be blocked, so
// every read and write is guarded: a blocked store just means nothing is remembered.

/** The keys used in localStorage. */
export const LAYOUT_KEYS = {
  listWidth: 'pm:potato:list-w',
  listMini: 'pm:potato:list-mini',
  panelWidth: 'pm:potato:panel-w',
  panelOpen: 'pm:potato:panel-open',
} as const;

/** The bot list: 240 to 480 px, 240 by default. */
export const LIST_MIN = 240;
export const LIST_MAX = 480;
export const LIST_DEFAULT = 240;
/** The collapsed list is a strip of faces this wide. */
export const LIST_MINI_WIDTH = 56;
/** Dragging the list's edge narrower than this snaps it to the strip. */
export const LIST_SNAP = 160;

/** The right bot panel: 320 to 1000 px, 384 by default. */
export const PANEL_MIN = 320;
export const PANEL_MAX = 1000;
export const PANEL_DEFAULT = 384;
/** The thread never gets narrower than this because of the panel. */
export const THREAD_MIN = 320;
/** One arrow press on a resize handle moves it this far. */
export const KEY_STEP = 32;

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

export function clampListWidth(width: number): number {
  return Number.isFinite(width) ? Math.round(clamp(width, LIST_MIN, LIST_MAX)) : LIST_DEFAULT;
}

/**
 * The panel width, kept inside its limits and, when the room is known, leaving the thread its minimum. `room` is the
 * width of the whole page; 0 or absent means "unknown" (no room limit). The minimum wins over the room: a very narrow
 * window gets a 320 px panel rather than none.
 */
export function clampPanelWidth(width: number, room = 0, listWidth = LIST_DEFAULT): number {
  if (!Number.isFinite(width)) return PANEL_DEFAULT;
  const fits = room > 0 ? room - listWidth - THREAD_MIN : PANEL_MAX;
  return Math.round(clamp(width, PANEL_MIN, Math.max(PANEL_MIN, Math.min(PANEL_MAX, fits))));
}

/** What dragging the list's edge to `raw` px means: the strip (below the snap point) or a width. */
export function listFromDrag(raw: number): { mini: true } | { mini: false; width: number } {
  return raw < LIST_SNAP ? { mini: true } : { mini: false, width: clampListWidth(raw) };
}

function readRaw(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeRaw(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // The choice just is not remembered.
  }
}

function readNumber(key: string): number | null {
  const raw = readRaw(key);
  if (raw === null || raw.trim() === '') return null;
  const n = Number(raw);
  return Number.isFinite(n) ? n : null;
}

export interface PotatoLayoutState {
  listWidth: number;
  listMini: boolean;
  panelWidth: number;
  panelOpen: boolean;
}

export const DEFAULT_LAYOUT: PotatoLayoutState = { listWidth: LIST_DEFAULT, listMini: false, panelWidth: PANEL_DEFAULT, panelOpen: false };

/** What was remembered (limits applied again, in case they changed), else the defaults. */
export function readLayout(): PotatoLayoutState {
  const list = readNumber(LAYOUT_KEYS.listWidth);
  const panel = readNumber(LAYOUT_KEYS.panelWidth);
  return {
    listWidth: list === null ? LIST_DEFAULT : clampListWidth(list),
    listMini: readRaw(LAYOUT_KEYS.listMini) === '1',
    panelWidth: panel === null ? PANEL_DEFAULT : clampPanelWidth(panel),
    panelOpen: readRaw(LAYOUT_KEYS.panelOpen) === '1',
  };
}

export function writeLayout(layout: PotatoLayoutState): void {
  writeRaw(LAYOUT_KEYS.listWidth, String(layout.listWidth));
  writeRaw(LAYOUT_KEYS.listMini, layout.listMini ? '1' : '0');
  writeRaw(LAYOUT_KEYS.panelWidth, String(layout.panelWidth));
  writeRaw(LAYOUT_KEYS.panelOpen, layout.panelOpen ? '1' : '0');
}

/** The width the list takes on screen. */
export function listShownWidth(layout: Pick<PotatoLayoutState, 'listWidth' | 'listMini'>): number {
  return layout.listMini ? LIST_MINI_WIDTH : layout.listWidth;
}
