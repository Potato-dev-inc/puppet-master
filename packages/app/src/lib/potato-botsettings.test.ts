// The per-bot settings client: the Model row's choices, the tool rows, the delete rule, the version preview, the command
// names the engine answers to, and the fake that the page tests use.
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  brainChoices,
  brainFromValue,
  brainValue,
  fakeBotSettingsTransport,
  INSTRUCTIONS_MAX,
  nameTypedRight,
  tauriBotSettingsTransport,
  toolRows,
  versionPreview,
} from './potato-botsettings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));

beforeEach(() => {
  vi.mocked(invoke).mockClear();
});

describe('the Model row', () => {
  it('offers the default first, named after the Settings brain, then the three brains', () => {
    expect(brainChoices('go').map((c) => c.label)).toEqual(['Use the default (OpenCode Go)', 'DeepSeek', 'OpenCode Go', 'Through my agents']);
    expect(brainChoices('deepseek')[0].label).toBe('Use the default (DeepSeek)');
  });

  it('maps "Use the default" to null and back, and refuses a value that is not a brain', () => {
    expect(brainFromValue('default')).toBeNull();
    expect(brainFromValue('agents')).toBe('agents');
    expect(brainFromValue('wizard')).toBeNull();
    expect(brainValue(null)).toBe('default');
    expect(brainValue('go')).toBe('go');
  });
});

describe('the tool rows', () => {
  it('show the computer switch only when the sandbox Labs switch is on', () => {
    expect(toolRows(false).map((r) => r.id)).toEqual(['helpers', 'web']);
    expect(toolRows(true).map((r) => r.id)).toEqual(['helpers', 'computer', 'web']);
  });
});

describe('the delete rule', () => {
  it('needs the name typed exactly, ends ignored', () => {
    expect(nameTypedRight(' Fish ', 'Fish')).toBe(true);
    expect(nameTypedRight('fish', 'Fish')).toBe(false);
    expect(nameTypedRight('', 'Fish')).toBe(false);
    expect(nameTypedRight('', '')).toBe(false);
  });
});

describe('a version preview', () => {
  it('is the first line that has words, cut to 80 characters', () => {
    expect(versionPreview('\n\nHello there\nmore')).toContain('Hello there');
    expect(versionPreview('x'.repeat(200))).toHaveLength(80);
    expect(versionPreview('')).toBe('');
  });
});

describe('the Tauri commands', () => {
  it('call the engine commands by the names the Rust side registers', async () => {
    await tauriBotSettingsTransport.options('fish');
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_options_get', { botId: 'fish' });
    await tauriBotSettingsTransport.setBrain('fish', null);
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_brain_set', { botId: 'fish', brain: null });
    await tauriBotSettingsTransport.setTools('fish', { helpers: true, computer: false, web: true });
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_tools_set', { botId: 'fish', tools: { helpers: true, computer: false, web: true } });
    await tauriBotSettingsTransport.saveInstructions('fish', 'Be brief.');
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_instructions_save', { botId: 'fish', text: 'Be brief.' });
    await tauriBotSettingsTransport.restoreInstructions('fish', 42);
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_instructions_restore', { botId: 'fish', at: 42 });
    await tauriBotSettingsTransport.rename('fish', 'Nimbus');
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_rename', { botId: 'fish', name: 'Nimbus' });
    await tauriBotSettingsTransport.remove('fish', 'Nimbus');
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_delete', { botId: 'fish', typedName: 'Nimbus' });
    await tauriBotSettingsTransport.exportChat('fish');
    expect(invoke).toHaveBeenLastCalledWith('potato_bot_export_markdown', { botId: 'fish' });
  });
});

describe('the fake', () => {
  it('keeps the versions newest first and adds none for the same text', async () => {
    let clock = 1_000;
    const fake = fakeBotSettingsTransport({ now: () => clock });
    await fake.saveInstructions('fish', 'Be brief.');
    clock += 5;
    const same = await fake.saveInstructions('fish', '  Be brief.  ');
    expect(same.versions).toHaveLength(1);
    clock += 5;
    const next = await fake.saveInstructions('fish', 'Be kind.');
    expect(next.versions.map((v) => v.text)).toEqual(['Be kind.', 'Be brief.']);
    expect(next.instructions).toBe('Be kind.');
  });

  it('restores an old version as the newest one, and refuses an unknown one', async () => {
    let clock = 1_000;
    const fake = fakeBotSettingsTransport({ now: () => clock });
    await fake.saveInstructions('fish', 'one');
    clock += 1;
    await fake.saveInstructions('fish', 'two');
    const first = (await fake.options('fish')).versions[1];
    const restored = await fake.restoreInstructions('fish', first.at);
    expect(restored.instructions).toBe('one');
    expect(restored.versions).toHaveLength(3);
    await expect(fake.restoreInstructions('fish', 7)).rejects.toThrow('gone');
  });

  it('refuses instructions over the limit and a delete without the exact name', async () => {
    const fake = fakeBotSettingsTransport({ name: 'Nimbus' });
    await expect(fake.saveInstructions('fish', 'a'.repeat(INSTRUCTIONS_MAX + 1))).rejects.toThrow('4000');
    await expect(fake.remove('fish', 'nimbus')).rejects.toThrow('exactly');
    await expect(fake.remove('fish', 'Nimbus')).resolves.toBeUndefined();
  });

  it('can be told to fail one call, and records every call in order', async () => {
    const fake = fakeBotSettingsTransport({ fail: { setTools: 'Could not save that.' } });
    await expect(fake.setTools('fish', { helpers: false, computer: true, web: true })).rejects.toThrow('Could not save that.');
    await fake.setBrain('fish', 'go');
    expect(fake.calls.map((c) => c.method)).toEqual(['setTools', 'setBrain']);
  });
});
