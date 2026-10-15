// Entry route of the standalone Settings window (`index.html?settings=1&tab=agents`), opened by the
// Rust side (tray, second launch, palette). Pure so it can be tested without a webview.
import { SECTION_IDS, type SettingsPageId } from '../components/settings/model';

export const DEFAULT_SETTINGS_TAB: SettingsPageId = 'agents';

/** The 22 old tab ids that callers may still send, and where each one lives now. */
export const LEGACY_TABS: Record<string, SettingsPageId> = {
  appearance: 'general',
  storage: 'general',
  advanced: 'general',
  preload: 'agents',
  session: 'models',
  orchestrator: 'models',
  api: 'keys',
  mcp: 'bridge',
  developer: 'bridge',
  mobile: 'phone',
  security: 'general',
  notifications: 'later',
  backup: 'later',
  marketplace: 'later',
  rules: 'later',
  costs: 'later',
  team: 'general',
  automation: 'later',
  observability: 'later',
  plugins: 'bridge',
};

export function isSettingsEntry(search: string): boolean {
  return new URLSearchParams(search).has('settings');
}

/** A page id from a section id or an old tab id; null when it names nothing. */
export function resolveSettingsTab(tab: string | null | undefined): SettingsPageId | null {
  if (!tab) return null;
  if (tab === 'later') return 'later';
  if ((SECTION_IDS as readonly string[]).includes(tab)) return tab as SettingsPageId;
  return LEGACY_TABS[tab] ?? null;
}

/** The page named by `?tab=`, or Agents when absent or unknown. */
export function settingsEntryTab(search: string): SettingsPageId {
  return resolveSettingsTab(new URLSearchParams(search).get('tab')) ?? DEFAULT_SETTINGS_TAB;
}
