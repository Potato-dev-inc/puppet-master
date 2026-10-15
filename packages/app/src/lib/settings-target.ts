// Where a Settings deep link lands: a page and, when it names one, a row to scroll to and mark.
//
// Anything that opens Settings may send any of these (the palette, the tray, an error card, `?tab=`):
//   - a section id (`agents`) or `later`
//   - a row id (`live-updates`, `budget-turns`, `security-ask-before-control`): the section that holds the row
//     opens, scrolled to it. Every row has one (ids are unique across sections).
//   - `labs`: General > Labs
//   - one of the old 22 tab ids (`preload`, `mobile`, `team`)
// This file imports the catalog, so the entry route in `settings-entry.ts` (read by main.tsx) does not.
import { LABS_INDEX_ID } from '../components/settings/arrange';
import { SECTIONS } from '../components/settings/catalog';
import { findRowSection, type SettingsPageId } from '../components/settings/model';
import { DEFAULT_SETTINGS_TAB, resolveSettingsTab } from './settings-entry';

export interface SettingsTarget {
  page: SettingsPageId;
  /** A row of that page to scroll to and mark. */
  row?: string;
}

/** Old or short names that open a row rather than just a page. */
const ROW_ALIASES: Record<string, string> = {
  labs: LABS_INDEX_ID,
  security: 'security-ask-before-control',
  plugins: 'agent-pack',
  costs: 'budget-turns',
};

/** The page and row a name stands for; null when it names nothing. */
export function resolveSettingsTarget(tab: string | null | undefined): SettingsTarget | null {
  if (!tab) return null;
  const direct = resolveSettingsTab(tab);
  const rowId = ROW_ALIASES[tab] ?? tab;
  const section = findRowSection(SECTIONS, rowId);
  // A section id wins over a row that happens to share its name.
  if (section && !(direct && direct === tab)) return { page: section.id, row: rowId };
  return direct ? { page: direct } : null;
}

/** The target named by `?tab=` and `?row=`, or the default page when neither names anything. */
export function settingsEntryTarget(search: string): SettingsTarget {
  const q = new URLSearchParams(search);
  const rowParam = q.get('row');
  const rowSection = rowParam ? findRowSection(SECTIONS, rowParam) : undefined;
  if (rowParam && rowSection) return { page: rowSection.id, row: rowParam };
  return resolveSettingsTarget(q.get('tab')) ?? { page: DEFAULT_SETTINGS_TAB };
}
