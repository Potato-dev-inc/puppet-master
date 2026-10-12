// Entry route of the standalone Settings window (`index.html?settings=1&tab=agents`), opened by the
// Rust side (tray, second launch). Pure so it can be tested without a webview.
import { SETTINGS_TABS, type SettingsTabId } from '../components/settings/settings-ui';

export const DEFAULT_SETTINGS_TAB: SettingsTabId = 'agents';

export function isSettingsEntry(search: string): boolean {
  return new URLSearchParams(search).has('settings');
}

/** The tab named by `?tab=`, or Agents when absent or unknown. */
export function settingsEntryTab(search: string): SettingsTabId {
  const wanted = new URLSearchParams(search).get('tab');
  const match = SETTINGS_TABS.find((tab) => tab.id === wanted);
  return match ? match.id : DEFAULT_SETTINGS_TAB;
}
