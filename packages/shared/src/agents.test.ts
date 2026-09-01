import { describe, expect, it } from 'vitest';
import {
  AgentTypeSchema,
  getDefaultTerminalAgentType,
  getPreset,
  listLaunchPresets,
  listPresets,
} from './agents';

describe('agent presets', () => {
  it('includes a first-class command prompt preset', () => {
    expect(AgentTypeSchema.options).toContain('cmd');
    expect(listPresets('windows').map((preset) => preset.type)).toContain('cmd');
  });

  it('uses cmd.exe for the Windows command prompt preset', () => {
    const preset = getPreset('cmd', 'windows');
    expect(preset.label).toBe('Command Prompt');
    expect(preset.command).toBe('cmd.exe');
    expect(preset.baseArgs).toEqual(['/K']);
  });

  it('uses bare Windows agent commands so PowerShell resolves shims', () => {
    expect(getPreset('claude', 'windows').command).toBe('claude');
    expect(getPreset('codex', 'windows').command).toBe('codex');
    expect(getPreset('opencode', 'windows').command).toBe('opencode');
  });

  it('only offers the OpenCode API worker in the spawn list', () => {
    expect(listLaunchPresets('windows').map((preset) => preset.type)).toEqual([
      'opencode_native',
    ]);
    expect(listLaunchPresets('linux').map((preset) => preset.type)).toEqual([
      'opencode_native',
    ]);
    expect(listLaunchPresets('windows')[0]?.label).toBe('OpenCode (API)');
  });

  it('defaults new workers to opencode_native', () => {
    expect(getDefaultTerminalAgentType('windows')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('macos')).toBe('opencode_native');
    expect(getDefaultTerminalAgentType('linux')).toBe('opencode_native');
  });
});
