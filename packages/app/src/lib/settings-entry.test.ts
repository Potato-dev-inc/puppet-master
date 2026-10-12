import { describe, expect, it } from 'vitest';
import { isSettingsEntry, settingsEntryTab } from './settings-entry';

describe('settings entry route', () => {
  it('is recognised by the settings param only', () => {
    expect(isSettingsEntry('?settings=1&tab=agents')).toBe(true);
    expect(isSettingsEntry('?terminal=1&pane=x')).toBe(false);
    expect(isSettingsEntry('')).toBe(false);
  });

  it('opens on the Agents tab by default and for unknown tabs', () => {
    expect(settingsEntryTab('?settings=1')).toBe('agents');
    expect(settingsEntryTab('?settings=1&tab=nope')).toBe('agents');
  });

  it('honours a known tab', () => {
    expect(settingsEntryTab('?settings=1&tab=mcp')).toBe('mcp');
    expect(settingsEntryTab('?settings=1&tab=general')).toBe('general');
  });
});
