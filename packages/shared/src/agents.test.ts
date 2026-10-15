import { describe, expect, it } from 'vitest';
import {
  AgentTypeSchema,
  BUILTIN_AGENT_TYPES,
  getDefaultTerminalAgentType,
  getPreset,
  listLaunchPresets,
  listPresets,
  presetFor,
} from './agents';

describe('agent presets', () => {
  it('includes a first-class command prompt preset', () => {
    expect(BUILTIN_AGENT_TYPES).toContain('cmd');
    expect(listPresets('windows').map((preset) => preset.type)).toContain('cmd');
  });

  it('uses cmd.exe for the Windows command prompt preset', () => {
    const preset = getPreset('cmd', 'windows');
    expect(preset.label).toBe('Command Prompt');
    expect(preset.command).toBe('cmd.exe');
    expect(preset.baseArgs).toEqual(['/K']);
  });

  it('keeps Windows PowerShell open after spawn', () => {
    const preset = getPreset('powershell', 'windows');
    expect(preset.command).toBe('powershell.exe');
    expect(preset.baseArgs).toEqual(['-NoLogo', '-NoExit']);
  });

  it('uses bare Windows agent commands so PowerShell resolves shims', () => {
    expect(getPreset('claude', 'windows').command).toBe('claude');
    expect(getPreset('codex', 'windows').command).toBe('codex');
    expect(getPreset('opencode', 'windows').command).toBe('opencode');
  });

  it('offers OpenCode API and PowerShell in the spawn list', () => {
    expect(listLaunchPresets('windows').map((preset) => preset.type)).toEqual([
      'opencode_native',
      'powershell',
    ]);
    expect(listLaunchPresets('linux').map((preset) => preset.type)).toEqual([
      'opencode_native',
      'powershell',
    ]);
    expect(listLaunchPresets('windows').map((preset) => preset.label)).toEqual([
      'OpenCode (API)',
      'PowerShell',
    ]);
  });

  it('defaults new workers to opencode_native', () => {
    expect(getDefaultTerminalAgentType('windows')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('macos')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('linux')).toBe('opencode_native');
  });

  it('keeps Cursor IDE and the headless Cursor Agent CLI distinct', () => {
    expect(getPreset('cursor', 'windows').command).toBe('cursor.cmd');
    expect(getPreset('cursor_agent', 'windows').command).toBe('cursor-agent');
    expect(getPreset('cursor_agent', 'windows').isTui).toBe(true);
  });
});

describe('agent id schema', () => {
  it.each([
    ['claude', true],
    ['opencode_native', true],
    ['cursor_agent', true],
    ['kilo', true],
    ['k2', true],
    ['a'.repeat(32), true],
    ['Bad Id', false],
    ['Kilo', false],
    ['k', false],
    ['1kilo', false],
    ['_kilo', false],
    ['kilo-code', false],
    ['a'.repeat(33), false],
    ['', false],
  ])('%j valid=%s', (id, valid) => {
    expect(AgentTypeSchema.safeParse(id).success).toBe(valid);
  });

  it('keeps the nine built-in ids as known presets, in the existing order', () => {
    expect(BUILTIN_AGENT_TYPES).toEqual([
      'claude',
      'codex',
      'opencode',
      'opencode_native',
      'cmd',
      'powershell',
      'bash',
      'cursor',
      'cursor_agent',
    ]);
    expect(listPresets('windows').map((preset) => preset.type)).toEqual([...BUILTIN_AGENT_TYPES]);
  });
});

describe('presetFor', () => {
  it('returns the built-in preset for a known id', () => {
    expect(presetFor('codex', 'windows')).toEqual(getPreset('codex', 'windows'));
    expect(presetFor('claude', 'windows').label).toBe('Claude Code');
  });

  it('falls back to a neutral preset for an unknown id', () => {
    const preset = presetFor('kilo', 'windows');
    expect(preset.type).toBe('kilo');
    expect(preset.label).toBe('kilo');
    expect(preset.icon).toBe('KI');
    expect(preset.isTui).toBe(true);
    expect(preset.baseArgs).toEqual([]);
  });

  it('getPreset accepts any id through the same fallback', () => {
    expect(getPreset('kilo', 'linux').label).toBe('kilo');
  });
});
