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

function haystack(row: RowDef): string {
  return `${row.label} ${row.hint ?? ''} ${row.keywords ?? ''}`.toLowerCase();
}

/** Every word in the query must appear in the row's label, hint or keywords (case-insensitive). */
export function rowMatches(row: RowDef, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const text = haystack(row);
  return words.every((w) => text.includes(w));
}

export interface SearchHit {
  section: SectionDef;
  rows: RowDef[];
}

/** Rows that match `query`, grouped under their section in nav order. Empty sections are left out. */
export function searchSections(sections: SectionDef[], query: string): SearchHit[] {
  if (!query.trim()) return [];
  return sections
    .map((section) => ({ section, rows: section.rows.filter((row) => rowMatches(row, query)) }))
    .filter((hit) => hit.rows.length > 0);
}

/** One entry of the read-only "Coming later" list. */
export interface LaterItem {
  title: string;
  note: string;
}
