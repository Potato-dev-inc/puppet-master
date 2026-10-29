// Package check: the typed command surface and the shapes the approval card reads.
//
// The Rust side (src-tauri/src/prompt_risk/packages/model.rs and src-tauri/src/package_check/model.rs)
// holds everything. The shapes below mirror its serde output exactly: enum values are the serde names
// (`list_file`, `look_alike_chars`, ...), and fields are camelCase (`runsNow`, `registryLabel`, ...).
// Tauri maps the Rust argument names to camelCase (`pane_id` is `paneId`).
//
// Nothing here talks to a registry. The only network call is `packageCheckLookup`, which the desktop
// makes once per card, and the backend re-reads the live prompt itself.
import { invoke } from '@tauri-apps/api/core';

/** Rust `Ecosystem` (lowercase serde). */
export type Ecosystem = 'npm' | 'pypi' | 'crates' | 'go' | 'gems' | 'winget' | 'choco' | 'scoop';

/** Rust `Source` (internally tagged by `kind`, snake_case). */
export type PackageSource =
  | { kind: 'registry' }
  | { kind: 'link'; host: string }
  | { kind: 'local' }
  | { kind: 'list_file'; file: string }
  | { kind: 'private_server' }
  | { kind: 'not_plain' };

/** Rust `How` (snake_case): how a name differs from the popular name it resembles. */
export type LookalikeHow = 'one_letter' | 'two_letters' | 'swapped' | 'separators' | 'look_alike_chars';

/** Rust `Lookalike`. */
export interface Lookalike {
  popular: string;
  how: LookalikeHow;
}

/** Rust `PackageItem` (camelCase). One package as the card first shows it, with no network. */
export interface PackageItem {
  ecosystem: Ecosystem;
  tool: string;
  name: string;
  /** Absent when the command gave no version. */
  spec?: string;
  source: PackageSource;
  global: boolean;
  runsNow: boolean;
  /** Absent when the name is not a look-alike. */
  lookalike?: Lookalike;
  /** In the offline top list. */
  popular: boolean;
  /** May be cut off at the edge of the screen. Such a name is not checked. */
  cut: boolean;
  /** A registry lookup will run for this item. */
  lookup: boolean;
  /** First-paint summary, e.g. "Looks like lodash (2 letters swapped) · Checking…". */
  first: string;
  /** The first paint already has a "Look closer" fact. */
  look: boolean;
}

/** Rust `PackageList` (camelCase). Every package one command installs, plus what was left out. */
export interface PackageList {
  items: PackageItem[];
  /** Packages past the per-command cap. */
  more: number;
  /** Names that looked like secrets: counted, never shown or checked. */
  hidden: number;
}

/** Rust `RowState` (snake_case). What the backend sends for a row. */
export type RowState = 'checked' | 'not_found' | 'failed' | 'not_checked';

/**
 * Client-only extension of `RowState`: a row whose lookup has not answered yet (first paint).
 * The backend never sends `checking`.
 */
export type PaintState = RowState | 'checking';

/** Rust `FactLine`. One sentence in a row's details. `look` marks a "Look closer" fact. */
export interface FactLine {
  look: boolean;
  text: string;
}

/** Rust `PackageRow` (camelCase). */
export interface PackageRow {
  name: string;
  ecosystem: Ecosystem;
  registryLabel: string;
  state: RowState;
  look: boolean;
  /** Up to three segments joined by " · ". */
  summary: string;
  lines: FactLine[];
  /** Present when the registry has a page for the package (`Open on npm`). */
  pageUrl: string | null;
}

/** A row as the card paints it: the backend's rows, or a first-paint row (`checking`). */
export type PaintRow = Omit<PackageRow, 'state'> & { state: PaintState };

/** Rust `PackageCheckView` (camelCase). */
export interface PackageCheckView {
  promptId: string;
  off: boolean;
  statusText: string;
  /** Number of rows with a "Look closer" fact. */
  look: number;
  rows: PackageRow[];
  more: number;
  hidden: number;
  footer: string;
  /** A row failed or was not found: the card shows "Check again". */
  canRetry: boolean;
}

/**
 * Rust `Settings` (the one setting: "Check packages before installs", on by default). The TypeScript
 * name is `PackageCheckSettings`; the field is the same.
 */
export interface PackageCheckSettings {
  lookups: boolean;
}

/** Rust `Ecosystem::registry_label`. Kept in step with the Rust file by a test. */
export const REGISTRY_LABEL: Record<Ecosystem, string> = {
  npm: 'npm',
  pypi: 'PyPI',
  crates: 'crates.io',
  go: 'the Go module mirror',
  gems: 'RubyGems',
  winget: 'winget',
  choco: 'Chocolatey',
  scoop: 'Scoop',
};

/** Rust `Ecosystem::normalize` for PyPI (PEP 503): runs of `-`, `_`, `.` become `-`, lowercase. */
function normalizePypi(name: string): string {
  return name.toLowerCase().replace(/[-_.]+/g, '-');
}

/** Rust `package_check::model::page_url`. Null where the registry has no page (winget, choco, scoop). */
export function pageUrlFor(ecosystem: Ecosystem, name: string): string | null {
  switch (ecosystem) {
    case 'npm':
      return `https://www.npmjs.com/package/${name}`;
    case 'pypi':
      return `https://pypi.org/project/${normalizePypi(name)}/`;
    case 'crates':
      return `https://crates.io/crates/${name}`;
    case 'gems':
      return `https://rubygems.org/gems/${name}`;
    case 'go':
      return `https://pkg.go.dev/${name}`;
    case 'winget':
    case 'choco':
    case 'scoop':
      return null;
  }
}

/**
 * The card's rows before any lookup answers. One row per item, in order, so the row count is fixed at
 * first paint. A row whose lookup will run is `checking`; the rest are `not_checked`. Its summary is
 * the item's `first` line. A link only shows for a plain registry name that is not cut off.
 */
export function firstPaintRows(list: PackageList): PaintRow[] {
  return list.items.map((item) => ({
    name: item.name,
    ecosystem: item.ecosystem,
    registryLabel: REGISTRY_LABEL[item.ecosystem],
    state: item.lookup ? 'checking' : 'not_checked',
    look: item.look,
    summary: item.first,
    lines: [],
    pageUrl: item.source.kind === 'registry' && !item.cut ? pageUrlFor(item.ecosystem, item.name) : null,
  }));
}

/**
 * Look up the public facts for the package names on a live approval card. The backend reads the
 * names from the pane's current prompt (by `promptId`), so the UI passes no names. `again` drops
 * unsettled cache entries first ("Check again").
 */
export function packageCheckLookup(paneId: string, promptId: string, again = false): Promise<PackageCheckView> {
  return invoke<PackageCheckView>('package_check_lookup', { paneId, promptId, again });
}

/** Settings > General > Safety > Check packages before installs. */
export function packageCheckSettingsGet(): Promise<PackageCheckSettings> {
  return invoke<PackageCheckSettings>('package_check_settings_get');
}

export function packageCheckSettingsSet(settings: PackageCheckSettings): Promise<PackageCheckSettings> {
  return invoke<PackageCheckSettings>('package_check_settings_set', { settings });
}
