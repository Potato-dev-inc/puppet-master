// Settings window model: the six sections, the row contract every section implements, and search.
//
// A section is a list of rows (`RowDef`). The panel renders them grouped, filters them when you search
// (label + hint + keywords), and hands every row a `SettingsEnv`: the fresh settings, a partial-patch
// `patch()` that saves one control, and a few helpers. Nothing in a row ever writes the whole settings
// object back.
import type { ReactNode } from 'react';
import type { Settings } from '@puppet-master/shared';
import type { UpdateCheckResult } from '../../lib/app-update';
import type { SettingsPatch } from '../../lib/settings';

export const SECTION_IDS = ['general', 'agents', 'models', 'keys', 'bridge', 'phone'] as const;
export type SectionId = (typeof SECTION_IDS)[number];

/** `later` is the read-only "Coming later" page; it is not a section of settings. */
export type SettingsPageId = SectionId | 'later';

export interface ConfirmOptions {
  title: string;
  body: string;
  /** Label of the confirm button, for example "Uninstall". */
  confirmLabel: string;
}

export interface UpdateInfo {
  result?: UpdateCheckResult | null;
  checking?: boolean;
  onCheck?: () => void;
  onOpenRelease?: () => void;
}

/** What the panel gives a row. Rows never touch storage themselves. */
export interface SettingsEnv {
  /** The current saved settings (reloaded when they change anywhere). */
  settings: Settings;
  /**
   * Save only the keys in `patch` (merged into the file on the Rust side) and flash the row's
   * "Saved" tick. Rejects when the write fails; the panel shows the error on the row.
   */
  patch: (patch: SettingsPatch) => Promise<void>;
  /** Flash the "Saved" tick for a control that saved through its own command. */
  markSaved: () => void;
  /** Show the row's error line (and clear it with `null`). */
  setError: (message: string | null) => void;
  /** Ask in-app before something destructive. Resolves true when the person confirms. */
  confirm: (options: ConfirmOptions) => Promise<boolean>;
  /** Jump to another row (even one in another section): a "Now in General › Labs" link. Absent in tests. */
  openRow?: (rowId: string) => void;
  /** Opens the Agent Store in the cockpit. Absent in the separate Settings window (the Store lives in the cockpit). */
  openStore?: () => void;
  projectPath: string | null;
  onProjectPathChange?: (path: string) => Promise<void>;
  /** Discovered local bridge URL, or null before discovery. */
  bridgeUrl: string | null;
  update: UpdateInfo;
}

export interface RowDef {
  /** Unique across all sections. Used for the "Saved" tick, tests and screenshots. */
  id: string;
  /** What the row is called. Searched. */
  label: string;
  /** The one-line explanation under the label. Searched. */
  hint?: string;
  /** Extra words that should find this row (labels of controls inside a composite row). */
  keywords?: string;
  /** Heading of the group this row belongs to. Consecutive rows with the same group share a list. */
  group?: string;
  /** Destructive: shown in the section's own last group, and always behind a confirm sheet. */
  danger?: boolean;
  /**
   * An early (off-by-default) feature: its short name and one plain line about it. `arrangeSections` collects
   * every row that has one into General > Labs, wherever the row is defined, and lists it there.
   */
  labs?: { name: string; about: string };
  /** A link left where a row used to be. Never searched (the real row is), and has no setting of its own. */
  pointer?: boolean;
  render: (env: SettingsEnv) => ReactNode;
}

export interface SectionDef {
  id: SectionId;
  title: string;
  /** One quiet line under the page title. */
  blurb: string;
  rows: RowDef[];
}

export interface RowGroup {
  /** Group heading, if any. */
  head?: string;
  danger: boolean;
  rows: RowDef[];
}

/** Rows of a section as lists: consecutive same-group rows together, destructive rows last in their own list. */
export function groupRows(rows: RowDef[]): RowGroup[] {
  const groups: RowGroup[] = [];
  for (const row of rows.filter((r) => !r.danger)) {
    const last = groups[groups.length - 1];
    if (last && !last.danger && last.head === row.group) last.rows.push(row);
    else groups.push({ head: row.group, danger: false, rows: [row] });
  }
  const danger = rows.filter((r) => r.danger);
  if (danger.length > 0) groups.push({ head: undefined, danger: true, rows: danger });
  return groups;
}

/** Lower-case words of a text: letters and digits only, so "claude.md" is "claude" and "md". */
function words(text: string): string[] {
  return text.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

/**
 * How well `row` answers one typed word, or 0 for no match. The label and the curated keywords match from the
 * start of a word, so "key" finds "API key" and "keychain". The hint only matches a whole word, so "key" no
 * longer finds "keyboard" in an unrelated hint.
 */
function wordScore(row: RowDef, typed: string): number {
  // "keys" also finds "key": try the word without a plural s.
  const candidates = typed.length > 3 && typed.endsWith('s') ? [typed, typed.slice(0, -1)] : [typed];
  return Math.max(...candidates.map((word) => wordScoreExact(row, word)));
}

function wordScoreExact(row: RowDef, word: string): number {
  const label = words(row.label);
  if (label.includes(word)) return 4;
  if (label.some((w) => w.startsWith(word))) return 3;
  const keys = words(row.keywords ?? '');
  if (keys.some((w) => w.startsWith(word))) return 2;
  if (words(row.hint ?? '').includes(word)) return 1;
  return 0;
}

/** Every word in the query must match the row's label, keywords or hint. Pointers never match. */
export function rowMatches(row: RowDef, query: string): boolean {
  return rowScore(row, query) > 0;
}

/** The sum of the word scores (0 when any word finds nothing). Label matches outrank hint matches. */
export function rowScore(row: RowDef, query: string): number {
  const typed = words(query);
  if (typed.length === 0) return 1;
  if (row.pointer) return 0;
  let total = 0;
  for (const word of typed) {
    const score = wordScore(row, word);
    if (score === 0) return 0;
    total += score;
  }
  return total;
}

export interface SearchHit {
  section: SectionDef;
  rows: RowDef[];
}

/**
 * Rows that match `query`, grouped under their section in nav order. Empty sections are left out. Inside a
 * section the best-matching group comes first, and inside a group the best-matching row, so a label match
 * is never buried under hint matches. Rows of one group stay together (the panel shows the group's title).
 */
export function searchSections(sections: SectionDef[], query: string): SearchHit[] {
  if (!query.trim()) return [];
  return sections
    .map((section) => {
      const scored = section.rows
        .map((row, index) => ({ row, index, score: rowScore(row, query) }))
        .filter((r) => r.score > 0);
      const best = new Map<string | undefined, number>();
      const first = new Map<string | undefined, number>();
      for (const r of scored) {
        best.set(r.row.group, Math.max(best.get(r.row.group) ?? 0, r.score));
        if (!first.has(r.row.group)) first.set(r.row.group, r.index);
      }
      scored.sort(
        (a, b) =>
          (best.get(b.row.group) ?? 0) - (best.get(a.row.group) ?? 0) ||
          (first.get(a.row.group) ?? 0) - (first.get(b.row.group) ?? 0) ||
          b.score - a.score ||
          a.index - b.index,
      );
      return { section, rows: scored.map((r) => r.row) };
    })
    .filter((hit) => hit.rows.length > 0);
}

/** The section a row lives in, by its id (ids are unique across all sections). */
export function findRowSection(sections: SectionDef[], rowId: string): SectionDef | undefined {
  return sections.find((s) => s.rows.some((r) => r.id === rowId));
}

/** One entry of the read-only "Coming later" list. */
export interface LaterItem {
  title: string;
  note: string;
}
