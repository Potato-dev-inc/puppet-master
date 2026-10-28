// The message box keeps what you typed: one unsent draft per pane, kept in localStorage so it survives switching
// panes, split view and closing the phone tab. Drafts are dropped after 7 days, at most 50 are kept, and a text over
// 20000 characters is not kept. Every storage call is wrapped: a storage that throws (private mode, blocked site
// data) just means no draft, and the box still works.

export const LS_COMPOSER_DRAFTS = 'pm.composerDrafts';
/** How long after the last change the draft is written (the box is also written when it goes away). */
export const DRAFT_SAVE_MS = 300;
/** A draft not written for this long is dropped. */
export const DRAFT_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
/** At most this many drafts are kept; the oldest go first. */
export const DRAFT_MAX_COUNT = 50;
/** A longer text is not kept as a draft. */
export const DRAFT_MAX_CHARS = 20000;

interface DraftEntry {
  text: string;
  /** When the draft was last written (ms since 1970). */
  at: number;
}
type DraftMap = Record<string, DraftEntry>;

function isEntry(value: unknown): value is DraftEntry {
  if (!value || typeof value !== 'object') return false;
  const entry = value as Partial<DraftEntry>;
  return typeof entry.text === 'string' && typeof entry.at === 'number' && Number.isFinite(entry.at);
}

/** The drafts still fresh, newest first, at most DRAFT_MAX_COUNT. Pure: `now` is passed in. */
export function keepFresh(map: DraftMap, now: number): DraftMap {
  const fresh = Object.entries(map)
    .filter(([, entry]) => now - entry.at <= DRAFT_MAX_AGE_MS)
    .sort(([, a], [, b]) => b.at - a.at)
    .slice(0, DRAFT_MAX_COUNT);
  return Object.fromEntries(fresh);
}

/** Every fresh draft. A stored value that is not readable counts as no drafts (so a write can start over). */
function readAll(now: number): DraftMap {
  let parsed: unknown = null;
  try {
    parsed = JSON.parse(localStorage.getItem(LS_COMPOSER_DRAFTS) ?? 'null');
  } catch {
    return {};
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
  const map: DraftMap = {};
  for (const [paneId, entry] of Object.entries(parsed)) if (isEntry(entry)) map[paneId] = entry;
  return keepFresh(map, now);
}

/** The unsent text for this pane, or '' when there is none (or storage cannot be read). */
export function readDraft(paneId: string, now = Date.now()): string {
  try {
    return readAll(now)[paneId]?.text ?? '';
  } catch {
    return '';
  }
}

/** Keeps the text for this pane. Blank text, or a text too long to keep, removes the pane's draft. */
export function writeDraft(paneId: string, text: string, now = Date.now()): void {
  try {
    const map = readAll(now);
    if (text.trim() === '' || Array.from(text).length > DRAFT_MAX_CHARS) delete map[paneId];
    else map[paneId] = { text, at: now };
    localStorage.setItem(LS_COMPOSER_DRAFTS, JSON.stringify(keepFresh(map, now)));
  } catch {
    /* private mode, full or blocked storage: no draft */
  }
}

/** Forgets the draft for this pane (a message was sent). */
export function clearDraft(paneId: string): void {
  writeDraft(paneId, '');
}
