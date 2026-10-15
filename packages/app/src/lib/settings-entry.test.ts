import { describe, expect, it } from 'vitest';
import { isSettingsEntry, resolveSettingsTab, settingsEntryTab } from './settings-entry';

describe('settings entry', () => {
  it('detects the settings route', () => {
    expect(isSettingsEntry('?settings=1&tab=agents')).toBe(true);
    expect(isSettingsEntry('?cockpit=1')).toBe(false);
  });

  it('defaults to Agents when the tab is missing or unknown', () => {
    expect(settingsEntryTab('?settings=1')).toBe('agents');
    expect(settingsEntryTab('?settings=1&tab=nope')).toBe('agents');
  });

  it('opens each of the six sections and Coming later by id', () => {
    for (const id of ['general', 'agents', 'models', 'keys', 'bridge', 'phone', 'later']) {
      expect(settingsEntryTab(`?settings=1&tab=${id}`)).toBe(id);
    }
  });

  it('maps every old tab id to its new home', () => {
    const expected: Record<string, string> = {
      appearance: 'general', storage: 'general', advanced: 'general', preload: 'agents', session: 'models',
      orchestrator: 'models', api: 'keys', mcp: 'bridge', developer: 'bridge', mobile: 'phone',
      security: 'general', notifications: 'later', backup: 'later', marketplace: 'later', rules: 'later',
      costs: 'later', team: 'general', automation: 'later', observability: 'later', plugins: 'bridge',
    };
    for (const [old, now] of Object.entries(expected)) expect(resolveSettingsTab(old)).toBe(now);
    expect(resolveSettingsTab('')).toBeNull();
    expect(resolveSettingsTab(null)).toBeNull();
  });
});
